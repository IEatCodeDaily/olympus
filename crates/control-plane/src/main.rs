//! Olympus Hall — the control-plane entrypoint (ADR 0008 S6).
//!
//! On boot: import the operator's Hermes `state.db` into a fresh event log,
//! build the in-memory views + search index from that log, then serve the REST
//! + WSS API on `127.0.0.1:8787` behind the per-install token.
//!
//! Hall owns the event log, views, search, REST/WS, and the fleet node
//! registry. Agent runtimes (the actual `hermes acp` children) live in the
//! separate `olympus-envoy` binary — Hall drives them over UDS via the
//! `EnvoyFrame` wire protocol. The local node is `olympus-envoy@1` over UDS,
//! not an in-process pseudo-envoy.
//!
//! The event log is rebuilt from `state.db` on every boot for the MVP (cheap,
//! deterministic, no migration story needed yet). Live sync (ADR §6.7) lands
//! later; for now the snapshot is taken at startup.

use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::{Context, Result};
use olympus_control_plane::{
    auth, import,
    log::Log,
    node::NodeRegistry,
    search::SearchIndex,
    server::{self, AppState, ImportState},
    sync,
    vault::VaultStore,
    views::ViewManager,
};
use tokio::sync::{broadcast, RwLock};

/// Where Olympus keeps its own INTERNAL state (event log, search index, token).
/// This is the dotted `~/.olympus/` root from ADR 0005 §4, which ALSO holds the
/// org-scoped resource tree (`<org>/sessions/`, `<org>/repos/`, etc.). Internal
/// state files live directly under it; resources live under `<org>/`.
fn olympus_home() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("OLYMPUS_HOME") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".olympus"))
}

/// The default org slug for the single-operator case (ADR 0005 §3 — org replaces
/// context). Multi-org management is post-MVP; the MVP runs one org. Override
/// with `OLYMPUS_DEFAULT_ORG`.
fn default_org() -> String {
    std::env::var("OLYMPUS_DEFAULT_ORG").unwrap_or_else(|_| "default".to_string())
}

/// The on-disk root for an org's resources: `~/.olympus/<org_slug>/` per ADR
/// 0005 §4. Holds `sessions/`, `repos/`, `vaults/`, `projects/`, etc.
fn org_workspace_root(org: &str) -> Result<PathBuf> {
    Ok(olympus_home()?.join(org))
}

/// Locate the Hermes state.db (override with `HERMES_STATE_DB`).
fn hermes_state_db() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("HERMES_STATE_DB") {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".hermes").join("state.db"))
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let home = olympus_home()?;
    std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;

    let token = auth::load_or_create_token()?;
    let profile = std::env::var("HERMES_PROFILE").unwrap_or_else(|_| "default".to_string());

    // ---- open the durable event log; keep Olympus-native records, rebuild the
    // state.db mirror each boot ----
    let log_path = home.join("eventlog.redb");
    let log = Arc::new(Log::open(&log_path).context("opening event log")?);
    // Drop the previous boot's state.db-imported events (keeping Olympus-native
    // records: setup declarations, cards, olympus sessions), so the re-import
    // below is idempotent and the durable declarations survive a restart.
    log.retain_native().context("retaining native events")?;

    let state_db = hermes_state_db()?;
    // ---- NATIVE-ONLY boot: rebuild views + search from the retained event log
    // (Olympus sessions, cards, setup declarations) so the server can start
    // serving IMMEDIATELY. The Hermes state.db import (1829 sessions, 127K
    // messages) runs AFTER bind in a background thread — see below.
    let mut views = ViewManager::new();
    views
        .replay(&log)
        .context("replaying native log into views")?;

    let mut search =
        SearchIndex::open(&home.join("search-index")).context("opening search index")?;
    search
        .build_from_log(&log)
        .context("building search index (native only)")?;

    // Snapshot counts BEFORE the Hermes import adds observed sessions — these
    // reflect Olympus-native records only, so they stay stable across restarts.
    let snap_sessions: u64 = 0;
    let snap_messages: u64 = 0;

    // ---- assemble server state ----
    let (deltas, _rx) = broadcast::channel(1024);
    // `log` is already an Arc<Log> (opened at the top); reuse it directly.
    let log_arc = log;
    let bridge = std::sync::Arc::new(
        olympus_control_plane::server::bridge_mgr::BridgeManager::with_factory(
            log_arc.clone(),
            std::sync::Arc::new(
                |spec: &olympus_control_plane::server::bridge_mgr::RuntimeSpec|
                 -> std::sync::Arc<dyn olympus_control_plane::bridge::AgentRuntime> {
                    let cwd = spec
                        .cwd
                        .as_deref()
                        .filter(|c| !c.is_empty())
                        .map(String::from)
                        .unwrap_or_else(|| {
                            std::env::current_dir()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_else(|_| ".".into())
                        });
                    let env = spec.env.clone();
                    // Route the chosen agent to the correct ACP adapter: Hermes
                    // profiles use `hermes acp`, while local CLI harnesses
                    // (Claude Code / Codex) use the pinned Zed ACP adapters.
                    let command =
                        olympus_control_plane::bridge::hermes::acp_command_for_agent(
                            spec.agent.as_deref(),
                        );
                    // Select the ACP wire framing: Hermes uses newline-delimited
                    // JSON (the transport hermes acp actually uses), while
                    // Claude Code and Codex use Content-Length framing per the
                    // ACP specification.
                    let framing =
                        olympus_control_plane::bridge::hermes::acp_framing_for_agent(
                            spec.agent.as_deref(),
                        );
                    let config =
                        olympus_control_plane::bridge::hermes::HermesRuntimeConfig {
                            command,
                            cwd,
                            session_source: Some("olympus".into()),
                            event_buffer: 256,
                            start_timeout_secs: 30,
                            mcp_servers: spec.mcp_servers.clone(),
                            env,
                            framing,
                        };
                    olympus_control_plane::bridge::hermes::HermesAgentRuntime::new_arc(config)
                },
            ),
        )
        // Session spaces live at ~/.olympus/<org>/sessions/<session_id>/ (ADR 0005
        // §4). Seed the org root + sessions dir; spaces are created eagerly per
        // session in create_draft. Node is NOT in the session id (ADR 0005 §6) —
        // it's inferred from the chosen agent and stored as a session field.
        .with_spaces_root(org_workspace_root(&default_org())?.join("sessions")),
    );
    let sync_connected = Arc::new(AtomicBool::new(false));

    // ---- fleet node registry ----
    // ADR 0008 S6: the local node is NO LONGER an in-process pseudo-envoy.
    // It is olympus-envoy@1 over UDS — the envoy binary connects and
    // registers itself at boot. Hall does not pre-register any node.
    let node_registry = NodeRegistry::new();

    let mut state = AppState {
        views: Arc::new(RwLock::new(views)),
        search: Arc::new(RwLock::new(search)),
        token: Arc::new(token.clone()),
        import_state: ImportState::running(), // Hermes import runs after bind (below)
        hermes_profile: Arc::new(profile),
        deltas,
        snapshot_sessions: snap_sessions,
        snapshot_messages: snap_messages,
        log: log_arc.clone(),
        bridge,
        sync_connected: sync_connected.clone(),
        irc: olympus_control_plane::irc::IrcBus::new(),
        nodes: node_registry.clone(),
        envoy_conns: olympus_control_plane::server::envoy_conn::EnvoyConnections::new(),
        proxy: olympus_control_plane::proxy::ProxyTable::new(),
        vaults: Arc::new(VaultStore::new(org_workspace_root(&default_org())?)),
        projects: Arc::new(olympus_control_plane::projects::ProjectStore::new(
            org_workspace_root(&default_org())?,
        )),
        repos: Arc::new(olympus_control_plane::repos::RepoStore::new(
            &org_workspace_root(&default_org())?,
            &default_org(),
        )),
        hall_iroh_id: None, // set below after endpoint creation
    };

    let sync_log = Arc::clone(&log_arc);
    let sync_views = Arc::clone(&state.views);
    let sync_search = Arc::clone(&state.search);
    let sync_deltas = state.deltas.clone();
    let sync_state_db = state_db.clone();
    let sync_connected_flag = sync_connected.clone();
    std::thread::Builder::new()
        .name("olympus-live-sync".into())
        .spawn(move || {
            tracing::info!(db = %sync_state_db.display(), "live sync worker starting");
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sync::run_live_sync(
                    sync_state_db,
                    sync_log,
                    sync_views,
                    sync_search,
                    sync_deltas,
                    sync_connected_flag.clone(),
                )
            }));
            // The worker is no longer connected once the loop exits for any reason.
            sync_connected_flag.store(false, Ordering::SeqCst);
            match result {
                Ok(Ok(())) => tracing::warn!("live sync worker loop returned (unexpected)"),
                Ok(Err(err)) => tracing::error!(error = %err, "live sync worker errored"),
                Err(panic) => {
                    let msg = panic
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| panic.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".into());
                    tracing::error!(panic = %msg, "live sync worker PANICKED");
                }
            }
        })
        .expect("spawn live sync thread");

    let app = server::build_router(state.clone());

    // Spawn the UDS listener for node (envoy) registration.
    let uds_path = home.join("control.sock");
    {
        let reg = node_registry.clone();
        let conns = state.envoy_conns.clone();
        tokio::spawn(async move {
            olympus_control_plane::node::run_uds_listener(uds_path, reg, conns).await;
        });
    }

    // Spawn the iroh listener for REMOTE envoys (ADR 0008 §1, S7). Public n0
    // relays; peers are gated by the node-id allowlist in ~/.olympus/hall.toml
    // (`allowed_envoys = ["<node-id>", ...]`) — fail closed: no file or empty
    // list means no remote envoys can connect (the endpoint still binds and
    // prints its node id so the operator can set up the allowlist).
    {
        match olympus_control_plane::node::create_iroh_endpoint(&home).await {
            Ok((endpoint, node_id)) => {
                println!("hall iroh node id: {node_id}");
                state.hall_iroh_id = Some(Arc::new(node_id.to_string()));
                let reg = node_registry.clone();
                let conns = state.envoy_conns.clone();
                let hall_home = home.clone();
                tokio::spawn(async move {
                    if let Err(e) = olympus_control_plane::node::run_iroh_accept_loop(
                        hall_home, endpoint, reg, conns,
                    )
                    .await
                    {
                        tracing::error!(error = format!("{e:#}"), "iroh accept loop failed");
                    }
                });
            }
            Err(e) => {
                tracing::error!(
                    error = format!("{e:#}"),
                    "failed to bind iroh endpoint — remote envoys disabled (UDS still active)"
                );
            }
        }
    }

    let bind = std::env::var("OLYMPUS_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    tracing::info!(
        addr = %bind,
        token_file = %home.join("token").display(),
        "olympus control plane listening"
    );
    println!("olympus control plane listening on http://{bind}");
    println!("token: {token}");

    // ---- Background Hermes state.db import (deferred so bind is instant) ----
    // The server is now serving with native-only data (Olympus sessions,
    // cards, setup). The Hermes import (1829 sessions, 127K messages) runs
    // here in a tokio task, appending to the log and replaying into views +
    // search. The live-sync worker (started above) will also pick up
    // incremental changes. import_state flips to Done when this completes.
    //
    // Uses tokio::task::spawn_blocking for the SQLite-heavy import (blocking
    // I/O), then async blocks for the view/search rebuild (tokio RwLocks).
    {
        let bg_log = Arc::clone(&log_arc);
        let bg_views = Arc::clone(&state.views);
        let bg_search = Arc::clone(&state.search);
        let bg_deltas = state.deltas.clone();
        let bg_import = state.import_state.clone();
        let bg_state_db = state_db.clone();
        tokio::spawn(async move {
            if !bg_state_db.exists() {
                tracing::warn!(db = %bg_state_db.display(), "state.db not found — skipping import");
                bg_import.set_done();
                return;
            }
            tracing::info!(db = %bg_state_db.display(), "importing Hermes state.db (background)");
            // Import is blocking SQLite I/O — run on the blocking pool.
            let db = bg_state_db.clone();
            let log_clone = Arc::clone(&bg_log);
            let import_result = tokio::task::spawn_blocking(move || {
                let s = import::import_sessions(&db, &log_clone)?;
                let m = import::import_messages(&db, &log_clone)?;
                Ok::<_, anyhow::Error>((s, m))
            })
            .await;
            match import_result {
                Ok(Ok((s, m))) => {
                    tracing::info!(
                        sessions = s.session_count,
                        messages = m.message_count,
                        "background import complete — replaying into views + search"
                    );
                    // Rebuild views + search from the now-complete log.
                    {
                        let mut v = bg_views.write().await;
                        *v = ViewManager::new();
                        if let Err(e) = v.replay(&bg_log) {
                            tracing::error!(error = %e, "view replay after import failed");
                        }
                    }
                    {
                        let mut idx = bg_search.write().await;
                        if let Err(e) = idx.build_from_log(&bg_log) {
                            tracing::error!(error = %e, "search rebuild after import failed");
                        }
                    }
                }
                Ok(Err(e)) => tracing::error!(error = %e, "background import failed"),
                Err(e) => tracing::error!(error = %e, "background import task panicked"),
            }
            bg_import.set_done();
            use olympus_control_plane::server::ws::ServerFrame;
            let _ = bg_deltas.send(ServerFrame::SessionUpdated {
                session_id: "__import__".into(),
                changes: serde_json::json!({ "importState": "done" }),
            });
        });
    }

    axum::serve(listener, app).await.context("serving")?;
    Ok(())
}
