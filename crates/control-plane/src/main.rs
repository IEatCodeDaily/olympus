//! Olympus control plane — single-binary entrypoint.
//!
//! On boot: import the operator's Hermes `state.db` into a fresh event log,
//! build the in-memory views + search index from that log, then serve the REST
//! + WSS API on `127.0.0.1:8787` behind the per-install token.
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
    let (snap_sessions, snap_messages) = if state_db.exists() {
        tracing::info!(db = %state_db.display(), "importing Hermes state.db");
        let s = import::import_sessions(&state_db, &log).context("importing sessions")?;
        let m = import::import_messages(&state_db, &log).context("importing messages")?;
        tracing::info!(
            sessions = s.session_count,
            messages = m.message_count,
            "import complete"
        );
        (s.session_count, m.message_count)
    } else {
        tracing::warn!(db = %state_db.display(), "state.db not found — starting empty");
        (0, 0)
    };

    // ---- build views + search from the log ----
    let mut views = ViewManager::new();
    views.replay(&log).context("replaying log into views")?;

    let mut search =
        SearchIndex::open(&home.join("search-index")).context("opening search index")?;
    search
        .build_from_log(&log)
        .context("building search index")?;

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
    let node_registry = NodeRegistry::new();
    // Auto-register the local node (in-process pseudo-envoy per ADR 0005 §3).
    // The local node's envoy discovers its own agents (Hermes profiles + CLI
    // harnesses installed on THIS host) — that is the per-node source of truth,
    // not a global control-plane probe.
    let local_hostname = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "localhost".to_string());
    let local_agents = olympus_control_plane::server::agents::discover_local_agents();
    tracing::info!(count = local_agents.len(), "local envoy discovered agents");
    node_registry
        .register("local", &local_hostname, 4, "0.1", true, local_agents)
        .await;

    let state = AppState {
        views: Arc::new(RwLock::new(views)),
        search: Arc::new(RwLock::new(search)),
        token: Arc::new(token.clone()),
        import_state: ImportState::Done,
        hermes_profile: Arc::new(profile),
        deltas,
        snapshot_sessions: snap_sessions,
        snapshot_messages: snap_messages,
        log: log_arc.clone(),
        bridge,
        sync_connected: sync_connected.clone(),
        irc: olympus_control_plane::irc::IrcBus::new(),
        nodes: node_registry.clone(),
        proxy: olympus_control_plane::proxy::ProxyTable::new(),
        vaults: Arc::new(VaultStore::new(org_workspace_root(&default_org())?)),
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

    let app = server::build_router(state);

    // Spawn the UDS listener for node (envoy) registration.
    let uds_path = home.join("control.sock");
    {
        let reg = node_registry.clone();
        tokio::spawn(async move {
            olympus_control_plane::node::run_uds_listener(uds_path, reg).await;
        });
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

    axum::serve(listener, app).await.context("serving")?;
    Ok(())
}
