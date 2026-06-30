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
    search::SearchIndex,
    server::{self, AppState, ImportState},
    sync,
    views::ViewManager,
};
use tokio::sync::{broadcast, RwLock};

/// Where Olympus keeps its own state (event log, search index, token).
fn olympus_home() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("OLYMPUS_HOME") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".olympus"))
}

/// Locate the Hermes state.db (override with `HERMES_STATE_DB`).
fn hermes_state_db() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("HERMES_STATE_DB") {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".hermes").join("state.db"))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let home = olympus_home()?;
    std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;

    let token = auth::load_or_create_token()?;
    let profile = std::env::var("HERMES_PROFILE").unwrap_or_else(|_| "default".to_string());

    // ---- import state.db → fresh event log ----
    let log_path = home.join("eventlog.redb");
    // Rebuild from scratch each boot (MVP): remove any prior log.
    let _ = std::fs::remove_file(&log_path);
    let log = Arc::new(Log::open(&log_path).context("opening event log")?);

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
                    let mut config =
                        olympus_control_plane::bridge::hermes::HermesRuntimeConfig::default();
                    // Route the chosen agent (Hermes profile) to the child via the
                    // standard `-p <profile>` flag; absent → server default profile.
                    if let Some(agent) = &spec.agent {
                        if !agent.is_empty() {
                            config.command =
                                vec!["hermes".into(), "-p".into(), agent.clone(), "acp".into()];
                        }
                    }
                    // Run the agent in its scoped session space, not the host cwd.
                    if let Some(cwd) = &spec.cwd {
                        if !cwd.is_empty() {
                            config.cwd = cwd.clone();
                        }
                    }
                    olympus_control_plane::bridge::hermes::HermesAgentRuntime::new_arc(config)
                },
            ),
        )
        .with_spaces_root(home.join("spaces"))
        .with_node_id(std::env::var("OLYMPUS_NODE_ID").unwrap_or_else(|_| "local".to_string())),
    );
    let sync_connected = Arc::new(AtomicBool::new(false));
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
