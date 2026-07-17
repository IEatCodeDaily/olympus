//! S7 integration test: two iroh endpoints (hall-side accept + envoy-side
//! connect) exchange EnvoyFrame/HallFrame JSON-lines over a QUIC bi-stream,
//! plus the allowlist rejection path.
//!
//! Uses real iroh endpoints with deterministic direct loopback addresses and
//! public relays disabled. Public endpoint discovery is deliberately excluded
//! from the canonical offline suite.

use iroh::endpoint::presets;
use iroh::{Endpoint, RelayMode, SecretKey};
use olympus_envoy::transport::{load_or_create_secret, OLYMPUS_ALPN};
use olympus_proto::frames::EnvoyFrame;
use olympus_proto::version::{BuildVersion, PROTOCOL_VERSION};

/// Hall-side accept loop for one connection: reads one line, parses an
/// EnvoyFrame, answers with a welcome-ish JSON line.
async fn hall_accept_once(ep: Endpoint, allowlist: Vec<iroh::PublicKey>) -> Option<EnvoyFrame> {
    let incoming = ep.accept().await?;
    let conn = incoming.await.ok()?;
    // Allowlist gate: reject peers not on the list (fail closed).
    let peer = conn.remote_id();
    if !allowlist.contains(&peer) {
        conn.close(1u32.into(), b"not allowlisted");
        return None;
    }
    let (mut send, mut recv) = conn.accept_bi().await.ok()?;
    let buf = recv.read_to_end(64 * 1024).await.ok()?;
    let line = String::from_utf8(buf).ok()?;
    let frame: EnvoyFrame = serde_json::from_str(line.trim()).ok()?;
    send.write_all(b"{\"kind\":\"ack\",\"status\":\"ok\"}\n")
        .await
        .ok()?;
    send.finish().ok()?;
    // Give the peer a moment to read before the connection drops.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    Some(frame)
}

async fn connect_direct(
    endpoint: &Endpoint,
    hall: &Endpoint,
) -> anyhow::Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream)> {
    let connection = endpoint.connect(hall.addr(), OLYMPUS_ALPN).await?;
    Ok(connection.open_bi().await?)
}

#[tokio::test]
async fn iroh_loopback_hello_round_trip() {
    // Hall endpoint.
    let hall_secret = SecretKey::generate();
    let hall_ep = Endpoint::builder(presets::N0)
        .secret_key(hall_secret)
        .alpns(vec![OLYMPUS_ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("hall endpoint binds");
    // Envoy endpoint with a persisted key (exercises load_or_create_secret).
    let dir = tempfile::tempdir().unwrap();
    let envoy_secret = load_or_create_secret(dir.path()).unwrap();
    let envoy_pub = envoy_secret.public();
    let envoy_ep = Endpoint::builder(presets::N0)
        .secret_key(envoy_secret)
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("envoy endpoint binds");

    // Hall accepts in the background, allowlisting the envoy.
    let hall_task = tokio::spawn(hall_accept_once(hall_ep.clone(), vec![envoy_pub]));

    let (mut send, mut recv) = connect_direct(&envoy_ep, &hall_ep)
        .await
        .expect("envoy connects to hall via iroh");

    let hello = EnvoyFrame::Hello {
        node_id: "envoy-test".into(),
        hostname: "loopback".into(),
        slots_total: 4,
        protocol_version: PROTOCOL_VERSION,
        version: BuildVersion::for_binary("0.0.0-test"),
        agents: None,
        runtimes: vec![],
        roles: vec![olympus_proto::frames::NodeRole::AgentRuntime],
        job_attempts: vec![],
    };
    let mut line = serde_json::to_string(&hello).unwrap();
    line.push('\n');
    send.write_all(line.as_bytes()).await.unwrap();
    send.finish().unwrap();

    let reply = recv.read_to_end(4096).await.unwrap();
    let reply = String::from_utf8(reply).unwrap();
    assert!(reply.contains("\"ack\""), "hall acked: {reply}");

    let received = tokio::time::timeout(std::time::Duration::from_secs(30), hall_task)
        .await
        .expect("hall accept did not hang")
        .expect("hall task ok")
        .expect("hall saw a frame");
    match received {
        EnvoyFrame::Hello {
            node_id,
            protocol_version,
            ..
        } => {
            assert_eq!(node_id, "envoy-test");
            assert_eq!(protocol_version, PROTOCOL_VERSION);
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    envoy_ep.close().await;
    hall_ep.close().await;
}

#[tokio::test]
async fn iroh_rejects_non_allowlisted_peer() {
    let hall_ep = Endpoint::builder(presets::N0)
        .secret_key(SecretKey::generate())
        .alpns(vec![OLYMPUS_ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("hall endpoint binds");

    // Empty allowlist — every peer must be rejected.
    let hall_task = tokio::spawn(hall_accept_once(hall_ep.clone(), vec![]));

    let envoy_ep = Endpoint::builder(presets::N0)
        .secret_key(SecretKey::generate())
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("envoy endpoint binds");

    // The QUIC connection itself may establish (allowlist is checked
    // post-handshake), but the hall must close it without processing frames.
    if let Ok((mut send, mut recv)) = connect_direct(&envoy_ep, &hall_ep).await {
        let _ = send
            .write_all(b"{\"kind\":\"heartbeat\",\"nodeId\":\"x\"}\n")
            .await;
        let _ = send.finish();
        // Read should yield nothing / error — the hall closed on us.
        let got = recv.read_to_end(4096).await.unwrap_or_default();
        assert!(
            got.is_empty(),
            "non-allowlisted peer must get no protocol reply, got: {}",
            String::from_utf8_lossy(&got)
        );
    }

    let seen = tokio::time::timeout(std::time::Duration::from_secs(30), hall_task)
        .await
        .expect("hall accept did not hang")
        .expect("hall task ok");
    assert!(
        seen.is_none(),
        "hall must not process frames from non-allowlisted peers"
    );

    envoy_ep.close().await;
    hall_ep.close().await;
}
