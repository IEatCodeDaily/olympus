//! S7 integration test: hall-side iroh endpoint (real node.rs dispatch) accepts
//! a remote envoy over iroh; full hello → ensure_runtime round-trip works with
//! allowlist enforcement.
//!
//! These tests exercise the transport-generic `handle_envoy_conn` dispatch over
//! real iroh QUIC streams (public n0 relays, loopback). The same dispatch code
//! runs over UDS for local envoys — no protocol fork (ADR 0008 §1).

use iroh::endpoint::presets;
use iroh::{Endpoint, PublicKey, SecretKey};
use olympus_control_plane::node::{self, NodeRegistry};
use olympus_control_plane::server::envoy_conn::EnvoyConnections;
use olympus_envoy::transport::{self, OLYMPUS_ALPN};
use olympus_proto::frames::{EnvoyFrame, HallFrame};
use olympus_proto::version::{BuildVersion, PROTOCOL_VERSION};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Spawn a hall-side iroh accept loop that delegates to `handle_envoy_conn`.
/// Peers not in `allowlist` are rejected at accept (fail closed). An empty
/// allowlist rejects ALL peers (fail-closed default).
async fn spawn_hall(
    allowlist: Vec<PublicKey>,
) -> (Endpoint, PublicKey, NodeRegistry, EnvoyConnections) {
    let secret = SecretKey::generate();
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret)
        .alpns(vec![OLYMPUS_ALPN.to_vec()])
        .bind()
        .await
        .expect("hall endpoint binds");
    let hall_key = endpoint.id();
    let registry = NodeRegistry::new();
    let conns = EnvoyConnections::new();

    let reg = registry.clone();
    let cs = conns.clone();
    let allow = allowlist.clone();
    let ep = endpoint.clone();
    tokio::spawn(async move {
        while let Some(incoming) = ep.accept().await {
            let conn = match incoming.await {
                Ok(c) => c,
                Err(_) => continue,
            };
            let peer = conn.remote_id();
            if !allow.contains(&peer) {
                conn.close(1u32.into(), b"not allowlisted");
                continue;
            }
            let r = reg.clone();
            let c = cs.clone();
            tokio::spawn(async move {
                if let Ok((send, recv)) = conn.accept_bi().await {
                    node::handle_envoy_conn(
                        recv,
                        send,
                        r,
                        c,
                        node::NodeTransport::Iroh,
                        Some(peer.to_string()),
                    )
                    .await;
                }
            });
        }
    });
    (endpoint, hall_key, registry, conns)
}

/// Poll the registry until the node appears, or timeout.
async fn wait_for_node(registry: &NodeRegistry, node_id: &str, timeout_secs: u64) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        let nodes = registry.list().await;
        if nodes.iter().any(|n| n.node_id == node_id) {
            return true;
        }
        if std::time::Instant::now() > deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

#[tokio::test]
async fn iroh_envoy_hello_registers_in_registry() {
    let envoy_secret = SecretKey::generate();
    let envoy_pub = envoy_secret.public();
    let (_hall_ep, hall_key, registry, _conns) = spawn_hall(vec![envoy_pub]).await;

    // The envoy endpoint MUST stay alive for the connection lifetime.
    let envoy_ep = Endpoint::builder(presets::N0)
        .secret_key(envoy_secret)
        .bind()
        .await
        .expect("envoy binds");
    let (mut send, _recv) = transport::connect_to_hall(&envoy_ep, &hall_key.to_string())
        .await
        .expect("envoy connects");

    let hello = EnvoyFrame::Hello {
        node_id: "envoy-iroh-1".into(),
        hostname: "iroh-test".into(),
        slots_total: 4,
        protocol_version: PROTOCOL_VERSION,
        version: BuildVersion::for_binary("0.0.0-test"),
        agents: None,
        runtimes: vec![],
    };
    let mut line = serde_json::to_string(&hello).unwrap();
    line.push('\n');
    send.write_all(line.as_bytes()).await.unwrap();
    send.flush().await.unwrap();

    assert!(
        wait_for_node(&registry, "envoy-iroh-1", 15).await,
        "envoy should register within 15s"
    );

    let _ = envoy_ep.close().await;
}

#[tokio::test]
async fn iroh_non_allowlisted_envoy_rejected() {
    let (_hall_ep, hall_key, registry, _conns) = spawn_hall(vec![]).await;

    let envoy_secret = SecretKey::generate();
    let envoy_ep = Endpoint::builder(presets::N0)
        .secret_key(envoy_secret)
        .bind()
        .await
        .unwrap();

    if let Ok((mut send, _recv)) =
        transport::connect_to_hall(&envoy_ep, &hall_key.to_string()).await
    {
        let hello = serde_json::json!({
            "kind": "hello",
            "nodeId": "rejected-envoy",
            "hostname": "evil",
            "slotsTotal": 4,
            "protocolVersion": PROTOCOL_VERSION,
        });
        let _ = send.write_all(format!("{hello}\n").as_bytes()).await;
        let _ = send.flush().await;
    }
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let _ = envoy_ep.close().await;

    let nodes = registry.list().await;
    assert!(
        !nodes.iter().any(|n| n.node_id == "rejected-envoy"),
        "non-allowlisted envoy must not register"
    );
}

#[tokio::test]
async fn iroh_ensure_runtime_round_trip() {
    // Full round-trip: hello → hall sends EnsureRuntime via RemoteRuntime →
    // envoy reads it and responds with Resp → RemoteRuntime resolves.
    let envoy_secret = SecretKey::generate();
    let envoy_pub = envoy_secret.public();
    let (_hall_ep, hall_key, registry, conns) = spawn_hall(vec![envoy_pub]).await;

    // Envoy endpoint stays alive for the whole test.
    let envoy_ep = Endpoint::builder(presets::N0)
        .secret_key(envoy_secret)
        .bind()
        .await
        .expect("envoy binds");
    let (mut send, recv) = transport::connect_to_hall(&envoy_ep, &hall_key.to_string())
        .await
        .expect("envoy connects");

    // Send hello.
    let hello = EnvoyFrame::Hello {
        node_id: "envoy-rt".into(),
        hostname: "iroh-host".into(),
        slots_total: 4,
        protocol_version: PROTOCOL_VERSION,
        version: BuildVersion::for_binary("0.0.0-test"),
        agents: None,
        runtimes: vec![],
    };
    let mut line = serde_json::to_string(&hello).unwrap();
    line.push('\n');
    send.write_all(line.as_bytes()).await.unwrap();
    send.flush().await.unwrap();

    // Wait for registration.
    assert!(
        wait_for_node(&registry, "envoy-rt", 15).await,
        "envoy registered before round-trip"
    );

    // Spawn a responder that reads HallFrames and replies to EnsureRuntime.
    // Keeps `send` alive so the connection doesn't close prematurely.
    let send_arc = std::sync::Arc::new(tokio::sync::Mutex::new(send));
    let send_for_responder = send_arc.clone();
    let responder = tokio::spawn(async move {
        let mut lines = BufReader::new(recv).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            if let Ok(HallFrame::EnsureRuntime { req_id, .. }) =
                serde_json::from_str::<HallFrame>(&line)
            {
                let resp = EnvoyFrame::Resp {
                    req_id,
                    ok: true,
                    error: None,
                    result: Some(serde_json::json!({"hermesId": "hermes-iroh-mock"})),
                };
                let mut resp_line = serde_json::to_string(&resp).unwrap();
                resp_line.push('\n');
                let mut w = send_for_responder.lock().await;
                let _ = w.write_all(resp_line.as_bytes()).await;
                let _ = w.flush().await;
                return;
            }
        }
    });

    // Use RemoteRuntime (hall-side) to drive ensure_runtime on the envoy.
    let conn = conns.get("envoy-rt").await.expect("conn exists");
    let rt =
        olympus_control_plane::server::envoy_conn::RemoteRuntime::new_arc(conn, "s-iroh-rt".into());

    let result = tokio::time::timeout(std::time::Duration::from_secs(15), rt.start(None)).await;

    assert!(result.is_ok(), "ensure_runtime timed out");
    let start_result = result.unwrap();
    assert!(
        start_result.is_ok(),
        "ensure_runtime succeeded over iroh: {:?}",
        start_result.err()
    );
    assert_eq!(
        rt.hermes_session_id().await.as_deref(),
        Some("hermes-iroh-mock"),
        "hermesId captured from iroh Resp"
    );

    let _ = responder.await;
    let _ = envoy_ep.close().await;
}
