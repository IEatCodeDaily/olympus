//! Live smoke test for HermesAgentRuntime — run manually, not in CI.
//! `cargo run --example acp_smoke` from the control-plane crate dir.
//! Spawns real `hermes acp`, sends a prompt, prints streamed events, and
//! reports the session id (which should land in state.db tagged source=olympus).

use std::time::Duration;

use olympus_control_plane::bridge::hermes::{HermesAgentRuntime, HermesRuntimeConfig};
use olympus_control_plane::bridge::{AgentCommand, AgentRuntime};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .compact()
        .init();

    let cfg = HermesRuntimeConfig {
        command: vec!["hermes".into(), "acp".into()],
        cwd: std::env::current_dir()?.to_string_lossy().to_string(),
        session_source: Some("olympus".into()),
        event_buffer: 256,
        start_timeout_secs: 30,
    };
    let rt = HermesAgentRuntime::new_arc(cfg);

    eprintln!("[smoke] starting hermes acp (session/new, source=olympus)…");
    rt.start(None).await?;

    // Stream events in the background.
    let mut events = rt.events();
    let printer = tokio::spawn(async move {
        use futures::StreamExt;
        let mut got_text = false;
        while let Some(ev) = events.next().await {
            eprintln!("[event] {ev:?}");
            if matches!(ev, olympus_control_plane::bridge::AgentEvent::Text(_)) {
                got_text = true;
            }
            if matches!(ev, olympus_control_plane::bridge::AgentEvent::Done { .. }) {
                break;
            }
        }
        got_text
    });

    // Give the handshake + session/new a moment, then prompt.
    tokio::time::sleep(Duration::from_secs(3)).await;
    eprintln!("[smoke] sending prompt: say PONG");
    rt.send(AgentCommand::Prompt {
        text: "Reply with exactly the single word: PONG".into(),
        model: None,
    })
    .await?;

    // Wait up to 90s for the turn to finish.
    let got_text = tokio::time::timeout(Duration::from_secs(90), printer)
        .await
        .map(|r| r.unwrap_or(false))
        .unwrap_or(false);

    let _ = rt.stop().await;
    eprintln!("[smoke] done. got_text={got_text}");
    if got_text {
        eprintln!("[smoke] PASS — bridge drove hermes acp and streamed a response.");
        std::process::exit(0);
    } else {
        eprintln!("[smoke] FAIL — no text event received.");
        std::process::exit(1);
    }
}
