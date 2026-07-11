//! Server integration tests (moved out of `mod.rs` for ARCH-B; behavior unchanged).

use super::*;
use crate::event::Event;
use crate::log::Log;
use crate::server::dto::SessionDto;
use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt; // oneshot

    fn test_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::open(&dir.path().join("log.redb")).unwrap();
        log.append(&Event::SessionCreated {
            session_id: "s1".into(),
            hermes_id: "h1".into(),
            source: "telegram".into(),
            model: Some("glm-5.2".into()),
            title: Some("hello".into()),
            started_at: 100.0,
            message_count: 1,
            input_tokens: 2,
            output_tokens: 3,
            agent: None,
            node: None,
        })
        .unwrap();
        log.append(&Event::MessageAppended {
            session_id: "s1".into(),
            hermes_session_id: "h1".into(),
            message_id: 0,
            role: "user".into(),
            content: Some("hello world".into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 101.0,
            token_count: Some(2),
            finish_reason: None,
        })
        .unwrap();

        let mut views = ViewManager::new();
        views.replay(&log).unwrap();

        let (tx, _rx) = broadcast::channel(64);
        let log_arc = Arc::new(log);
        let mut search = SearchIndex::from_log(log_arc.clone());
        search.build_from_log(&log_arc).unwrap();
        let state = AppState {
            views: Arc::new(RwLock::new(views)),
            search: Arc::new(RwLock::new(search)),
            token: Arc::new("testtoken".to_string()),
            auth_store: Arc::new(crate::auth_store::AuthStore::open_in_memory().unwrap()),
            allow_installation_token: true,
            session_cookie_secure: true,
            import_state: ImportState::done(),
            hermes_profile: Arc::new("default".into()),
            deltas: tx,
            snapshot_sessions: 1,
            snapshot_messages: 1,
            log: log_arc.clone(),
            bridge: Arc::new(BridgeManager::with_factory(
                log_arc.clone(),
                test_support::mock_factory(),
            )),
            sync_connected: Arc::new(AtomicBool::new(true)),
            irc: crate::irc::IrcBus::new(),
            nodes: crate::node::NodeRegistry::new(),
            envoy_conns: crate::server::envoy_conn::EnvoyConnections::new(),
            hall_iroh_id: None,
            proxy: crate::proxy::ProxyTable::new(),
            vaults: Arc::new(crate::vault::VaultStore::with_jj_mode(
                dir.path().join("default"),
                crate::vault::JjMode::Disabled,
            )),
            state_db: None,
            projects: Arc::new(crate::projects::ProjectStore::new(
                dir.path().join("default"),
            )),
            repos: Arc::new(crate::repos::RepoStore::new(
                dir.path().join("default"),
                "default",
            )),
            enroll: crate::enroll::EnrollStore::new(),
            home: Arc::new(dir.path().to_path_buf()),
        };
        (state, dir)
    }

    #[tokio::test]
    async fn login_cookie_authenticates_session_and_organization_routes() {
        let (state, _dir) = test_state();
        state
            .auth_store
            .bootstrap_admin("admin", "password-123", "default", "Default")
            .unwrap();
        let app = build_router(state);

        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header("origin", "http://127.0.0.1:5173")
                    .header("host", "127.0.0.1:5173")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"username":"admin","password":"password-123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::OK);
        let set_cookie = login.headers().get("set-cookie").unwrap().to_str().unwrap();
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Strict"));
        assert!(set_cookie.contains("Secure"));
        let cookie = set_cookie.split(';').next().unwrap();

        let current_session = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/auth/session")
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(current_session.status(), StatusCode::OK);

        let organizations = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/organizations")
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(organizations.status(), StatusCode::OK);
        let body = axum::body::to_bytes(organizations.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let organization_id = body["organizations"][0]["id"].as_str().unwrap();

        let scoped_sessions = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/organizations/{organization_id}/sessions"))
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(scoped_sessions.status(), StatusCode::OK);

        let scoped_session = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/organizations/{organization_id}/sessions/s1"))
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(scoped_session.status(), StatusCode::NOT_FOUND);

        let created = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/organizations/{organization_id}/sessions"))
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(created.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let created_id = body["id"].as_str().unwrap();
        assert_eq!(body["orgId"], organization_id);

        let created_detail = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/organizations/{organization_id}/sessions/{created_id}"
                    ))
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(created_detail.status(), StatusCode::OK);

        let unscoped_cookie_access = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unscoped_cookie_access.status(), StatusCode::FORBIDDEN);

        let handover = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/organizations/{organization_id}/sessions/{created_id}/handover"
                    ))
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"toAgentKind":"codex"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(handover.status(), StatusCode::OK);
        let body = axum::body::to_bytes(handover.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["session"]["orgId"], organization_id);

        let scoped_vaults = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/organizations/{organization_id}/vaults"))
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(scoped_vaults.status(), StatusCode::OK);

        let unknown_organization = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/organizations/not-a-membership/sessions")
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unknown_organization.status(), StatusCode::NOT_FOUND);

        let legacy_operator_scoped = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/organizations/{organization_id}/sessions"))
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(legacy_operator_scoped.status(), StatusCode::OK);

        let logout = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/logout")
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::NO_CONTENT);
        assert!(logout
            .headers()
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("Max-Age=0"));

        let revoked_session = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/session")
                    .header("cookie", cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoked_session.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sessions_without_token_is_401() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn cookie_sessions_and_operator_flag_fail_closed() {
        let (mut state, _dir) = test_state();
        state
            .auth_store
            .bootstrap_admin("admin", "password-123", "default", "Default")
            .unwrap();
        let user = state
            .auth_store
            .authenticate("admin", "password-123")
            .unwrap()
            .unwrap();
        let now = identity::unix_timestamp();
        let live = state
            .auth_store
            .create_session(&user.user_id, now, 60)
            .unwrap();
        let live_cookie = format!("olympus_session={}", live.token);
        let app = build_router(state.clone());

        let absent_origin = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/auth/session")
                    .header("cookie", &live_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(absent_origin.status(), StatusCode::FORBIDDEN);

        state.auth_store.revoke_session(&live.token).unwrap();
        let revoked = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/auth/session")
                    .header("cookie", &live_cookie)
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);

        let expired = state
            .auth_store
            .create_session(&user.user_id, now - 10, 1)
            .unwrap();
        let expired = app
            .oneshot(
                Request::builder()
                    .uri("/api/auth/session")
                    .header("cookie", format!("olympus_session={}", expired.token))
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(expired.status(), StatusCode::UNAUTHORIZED);

        state.allow_installation_token = false;
        let disabled_operator = build_router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(disabled_operator.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sessions_with_token_is_200_and_lists() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["total"], 1);
        assert_eq!(v["sessions"][0]["hermesId"], "h1");
        assert_eq!(v["sessions"][0]["source"], "telegram");
    }

    #[tokio::test]
    async fn sort_by_message_count_orders_descending() {
        // Build a 3-session state where started_at order != messageCount order,
        // so a working sort is distinguishable from the view's default.
        let dir = tempfile::tempdir().unwrap();
        let log = Log::open(&dir.path().join("log.redb")).unwrap();
        let mk = |id: &str, started: f64, msgs: u64| Event::SessionCreated {
            session_id: id.into(),
            hermes_id: id.into(),
            source: "cli".into(),
            model: None,
            title: None,
            started_at: started,
            message_count: msgs,
            input_tokens: 0,
            output_tokens: 0,
            agent: None,
            node: None,
        };
        // newest started has FEWEST messages, so startedAt-desc != messageCount-desc.
        log.append(&mk("old_big", 100.0, 500)).unwrap();
        log.append(&mk("mid", 200.0, 50)).unwrap();
        log.append(&mk("new_small", 300.0, 5)).unwrap();
        let mut views = ViewManager::new();
        views.replay(&log).unwrap();
        let mut search = SearchIndex::open(&dir.path().join("idx")).unwrap();
        search.build_from_log(&log).unwrap();
        let (tx, _rx) = broadcast::channel(64);
        let state = AppState {
            views: Arc::new(RwLock::new(views)),
            search: Arc::new(RwLock::new(search)),
            token: Arc::new("testtoken".to_string()),
            auth_store: Arc::new(crate::auth_store::AuthStore::open_in_memory().unwrap()),
            allow_installation_token: true,
            session_cookie_secure: true,
            import_state: ImportState::done(),
            hermes_profile: Arc::new("default".to_string()),
            deltas: tx,
            snapshot_sessions: 3,
            snapshot_messages: 0,
            log: Arc::new(log),
            bridge: Arc::new(BridgeManager::with_factory(
                Arc::new(Log::open(&dir.path().join("bridge-log.redb")).unwrap()),
                test_support::mock_factory(),
            )),
            sync_connected: Arc::new(AtomicBool::new(true)),
            irc: crate::irc::IrcBus::new(),
            nodes: crate::node::NodeRegistry::new(),
            envoy_conns: crate::server::envoy_conn::EnvoyConnections::new(),
            hall_iroh_id: None,
            proxy: crate::proxy::ProxyTable::new(),
            vaults: Arc::new(crate::vault::VaultStore::with_jj_mode(
                dir.path().join("default"),
                crate::vault::JjMode::Disabled,
            )),
            state_db: None,
            projects: Arc::new(crate::projects::ProjectStore::new(
                dir.path().join("default"),
            )),
            repos: Arc::new(crate::repos::RepoStore::new(
                dir.path().join("default"),
                "default",
            )),
            enroll: crate::enroll::EnrollStore::new(),
            home: Arc::new(dir.path().to_path_buf()),
        };
        let app = build_router(state);

        let fetch = |app: axum::Router, q: &str| {
            let uri = format!("/api/sessions?{q}");
            async move {
                let res = app
                    .oneshot(
                        Request::builder()
                            .uri(&uri)
                            .header("authorization", "Bearer testtoken")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let body = axum::body::to_bytes(res.into_body(), usize::MAX)
                    .await
                    .unwrap();
                serde_json::from_slice::<serde_json::Value>(&body).unwrap()
            }
        };

        // sort=messageCount -> 500, 50, 5
        let v = fetch(app.clone(), "sort=messageCount").await;
        let ids: Vec<&str> = v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec!["old_big", "mid", "new_small"],
            "messageCount desc"
        );

        // sort=startedAt -> 300, 200, 100 (different order, proves sort is applied)
        let v = fetch(app.clone(), "sort=startedAt").await;
        let ids: Vec<&str> = v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["new_small", "mid", "old_big"], "startedAt desc");
    }

    #[tokio::test]
    async fn wrong_token_is_401() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header("authorization", "Bearer nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn foreign_origin_is_403() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .header("origin", "http://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn health_is_unauthenticated() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["importState"], "done");
        assert_eq!(v["hermesProfile"], "default");
        assert_eq!(v["syncConnected"], true);
    }

    #[tokio::test]
    async fn messages_endpoint_returns_camelcase() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions/s1/messages")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["messages"][0]["messageId"], 0);
        assert_eq!(v["messages"][0]["content"], "hello world");
    }

    #[tokio::test]
    async fn get_unknown_session_is_404() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/sessions/ghost")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_message_to_observed_session_is_409() {
        // s1 is a telegram (observed) session — posting must be rejected with 409.
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions/s1/messages")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"text":"hi"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"], "observed");
    }

    #[tokio::test]
    async fn post_message_to_unknown_session_is_404() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions/ghost/messages")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"text":"hi"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_fork_observed_session_returns_managed_fork_and_leaves_source() {
        let (mut state, _d) = test_state();
        state.bridge = Arc::new(BridgeManager::with_factory(
            state.log.clone(),
            test_support::mock_factory(),
        ));
        let app = build_router(state.clone());

        let source_before = {
            let views = state.views.read().await;
            SessionDto::from_row(views.sessions.get("s1").unwrap())
        };

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions/s1/fork")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"forkType":"sub"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(v["session"]["source"], "olympus");
        assert_eq!(v["session"]["managed"], true);
        assert_eq!(v["session"]["forkedFrom"], "s1");
        assert_eq!(v["session"]["forkType"], "sub");
        assert!(v["session"]["id"].as_str().unwrap() != "s1");

        let source_after = {
            let views = state.views.read().await;
            SessionDto::from_row(views.sessions.get("s1").unwrap())
        };
        assert_eq!(source_after, source_before);
    }

    #[tokio::test]
    async fn search_finds_indexed_message() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/search?q=hello")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(!v["hits"].as_array().unwrap().is_empty());
        assert_eq!(v["hits"][0]["sessionId"], "s1");
        assert_eq!(v["hits"][0]["source"], "telegram");
    }

    // ---- A2: POST /api/sessions (new managed Olympus chat) ----

    #[tokio::test]
    async fn post_sessions_creates_managed_olympus_session() {
        // POST /api/sessions with no body → creates a new Olympus-managed session
        // OPTIMISTICALLY (no runtime spawned), returns 201 with a Session DTO
        // where source="olympus", managed=true, and an empty hermesId (the real
        // id is backfilled lazily on the first send).
        let (mut state, _d) = test_state();
        state.bridge = Arc::new(BridgeManager::with_factory(
            Arc::new(Log::open(&_d.path().join("bridge-log-a.redb")).unwrap()),
            test_support::mock_factory(),
        ));
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["source"], "olympus");
        assert_eq!(v["managed"], true);
        // Optimistic: a durable id is allocated immediately (`<utc>-<hash>` per
        // ADR 0005 §6 — node is NOT in the id); hermesId is empty until the
        // first send spawns the runtime.
        let id = v["id"].as_str().unwrap();
        assert!(
            id.starts_with("20") || id.starts_with("19"),
            "id should start with a UTC datetime stamp: {id}"
        );
        assert_eq!(
            id.matches('-').count(),
            1,
            "id should be <utc>-<hash> with no node segment: {id}"
        );
        assert_eq!(v["hermesId"], "");
    }

    #[tokio::test]
    async fn post_sessions_with_agent_binds_it_at_creation() {
        // POST /api/sessions {agent, node} → the draft carries the binding.
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"agent":"coding-agent","node":"local"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["agent"], "coding-agent");
        assert_eq!(v["node"], "local");
    }

    #[tokio::test]
    async fn patch_session_assigns_agent_and_model() {
        // PATCH /api/sessions/:id sets agent/model on an existing managed draft.
        let (state, _d) = test_state();
        // Create a draft first.
        let ns = state
            .bridge
            .create_draft(&crate::server::bridge_mgr::RuntimeSpec::default(), None)
            .unwrap();
        {
            let mut views = state.views.write().await;
            if let Ok(events) = state.log.read_all() {
                for (_s, e) in events {
                    views.apply(&e);
                }
            }
            let _ = views.sessions.get(&ns.session_id); // ensure present
        }
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/sessions/{}", ns.session_id))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"agent":"glm52","model":"glm-5.2"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["agent"], "glm52");
        assert_eq!(v["model"], "glm-5.2");
    }

    #[tokio::test]
    async fn patch_session_pins_and_archives() {
        // PATCH /api/sessions/:id with pinned/archived persists both flags.
        let (state, _d) = test_state();
        let ns = state
            .bridge
            .create_draft(&crate::server::bridge_mgr::RuntimeSpec::default(), None)
            .unwrap();
        {
            let mut views = state.views.write().await;
            if let Ok(events) = state.log.read_all() {
                for (_s, e) in events {
                    views.apply(&e);
                }
            }
        }
        let app = build_router(state.clone());
        let res = app
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/sessions/{}", ns.session_id))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"pinned":true,"archived":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["pinned"], true);
        assert_eq!(v["archived"], true);
        // Unpin only — archived must be left unchanged.
        let app2 = build_router(state);
        let res2 = app2
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/sessions/{}", ns.session_id))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"pinned":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res2.status(), StatusCode::OK);
        let body2 = axum::body::to_bytes(res2.into_body(), usize::MAX)
            .await
            .unwrap();
        let v2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
        assert_eq!(v2["pinned"], false);
        assert_eq!(v2["archived"], true, "archived untouched by pin-only patch");
    }

    #[tokio::test]
    async fn post_message_lazily_spawns_runtime_for_draft_session() {
        // A draft (no runtime, empty hermesId) accepts a send: the handler
        // lazily spawns the runtime via the factory and returns 202 — it does
        // NOT 503 "bridge_unavailable" (the pre-fix regression).
        let (state, _d) = test_state();
        let ns = state
            .bridge
            .create_draft(&crate::server::bridge_mgr::RuntimeSpec::default(), None)
            .unwrap();
        {
            let mut views = state.views.write().await;
            if let Ok(events) = state.log.read_all() {
                for (_s, e) in events {
                    views.apply(&e);
                }
            }
        }
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sessions/{}/messages", ns.session_id))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"text":"hello"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn agent_error_event_is_persisted_as_system_message() {
        let (mut state, _d) = test_state();
        state.bridge = Arc::new(BridgeManager::with_factory(
            state.log.clone(),
            test_support::mock_factory(),
        ));
        let app = build_router(state.clone());

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = v["id"].as_str().unwrap().to_string();

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sessions/{session_id}/messages"))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"content":"trigger agent error"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::ACCEPTED);

        let mut messages = serde_json::Value::Null;
        for _ in 0..50 {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/sessions/{session_id}/messages"))
                        .header("authorization", "Bearer testtoken")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            let body = axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap();
            messages = serde_json::from_slice(&body).unwrap();
            if messages["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["role"] == "system" && m["content"] == "⚠ agent error: mock failure")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let system = messages["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "system")
            .expect("system error message should be persisted");
        assert_eq!(system["content"], "⚠ agent error: mock failure");
    }

    // ---- card CRUD tests (C1) ----

    #[tokio::test]
    async fn list_cards_empty_by_default() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/cards")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["cards"].is_array());
    }

    #[tokio::test]
    async fn post_message_to_managed_olympus_session_is_202() {
        // A managed olympus session should accept a prompt and return 202
        // (not 503 — the bridge is wired).
        let (mut state, _d) = test_state();
        // The bridge must use the SAME log as the AppState so create_session's
        // SessionCreated event is visible to post_message's view lookup.
        state.bridge = Arc::new(BridgeManager::with_factory(
            state.log.clone(),
            test_support::mock_factory(),
        ));
        // First create a managed session via the API so the bridge knows about it.
        let app = build_router(state.clone());
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let session_id = v["id"].as_str().unwrap().to_string();

        // Now POST a message to that session.
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/sessions/{session_id}/messages"))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"text":"say PONG"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn post_sessions_without_token_is_401() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn vault_routes_create_write_read_and_list_notes() {
        let (state, _d) = test_state();
        let app = build_router(state);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/vaults")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "Ops Vault",
                            "backend": {
                                "kind": "github",
                                "repository": "IEatCodeDaily/ops-vault",
                                "branch": "main",
                                "syncEngine": "jj-git"
                            }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(created["id"], "ops-vault");
        assert_eq!(created["backend"]["kind"], "github");
        assert_eq!(created["backend"]["repository"], "IEatCodeDaily/ops-vault");

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/vaults/ops-vault/note?path=runbooks/boot.md")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "markdown": "---\ntitle: Boot\n---\n# Ignored\nSee [[Incident Guide]]"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let note: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(note["path"], "runbooks/boot.md");
        assert_eq!(note["title"], "Boot");
        assert_eq!(note["linkedNotes"][0], "Incident Guide");

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/vaults/ops-vault/note?path=runbooks/boot.md")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "markdown": "# Replacement",
                            "createOnly": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/vaults/ops-vault/note?path=runbooks/boot.md")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/vaults/ops-vault/notes")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let tree: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(tree["notes"][0]["kind"], "folder");
        assert_eq!(tree["notes"][0]["children"][0]["path"], "runbooks/boot.md");

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/vaults/ops-vault/documents")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let documents: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(documents["documents"][0]["path"], "runbooks/boot.md");
        assert_eq!(documents["documents"][0]["frontmatter"]["title"], "Boot");

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/vaults")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(list["vaults"][0]["noteCount"], 1);
    }

    #[tokio::test]
    async fn create_card_returns_camelcase_dto() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/cards")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"boardId":"b1","title":"Do stuff"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["id"].as_str().unwrap().starts_with("card-"));
        assert_eq!(v["boardId"], "b1");
        assert_eq!(v["title"], "Do stuff");
        assert_eq!(v["status"], "todo");
        // snake_case keys must NOT be present
        assert!(v.get("board_id").is_none());
    }

    #[tokio::test]
    async fn assign_card_transitions_to_assigned() {
        let (state, _d) = test_state();
        let app = build_router(state);

        // Create first
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/cards")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"boardId":"b1","title":"T"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let card_id = v["id"].as_str().unwrap().to_string();

        // Assign
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/cards/{card_id}/assign"))
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"assignedId":"zephyr","assignedKind":"agent","sessionId":"s1","attemptBookmark":"bm-1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], "assigned");
        assert_eq!(v["assignedId"], "zephyr");
        assert_eq!(v["currentSessionId"], "s1");
    }

    #[tokio::test]
    async fn complete_card_transitions_to_done() {
        let (state, _d) = test_state();
        let app = build_router(state);

        // Create + claim
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/cards")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"boardId":"b1","title":"T"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let card_id = v["id"].as_str().unwrap().to_string();

        // Complete
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/cards/{card_id}/complete"))
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], "done");
    }

    #[tokio::test]
    async fn get_unknown_card_is_404() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/cards/ghost")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    // ---- setup declaration endpoints (ADR 0006 §3) ----

    #[tokio::test]
    async fn put_setup_then_get_roundtrips() {
        let (state, _d) = test_state();
        let app = build_router(state);
        // PUT an org-scope declaration.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/setup")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"scope":"org:acme","skills":["code-review"],"plugins":["gitnexus"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["scope"], "org:acme");
        assert_eq!(v["skills"][0], "code-review");
        assert_eq!(v["plugins"][0], "gitnexus");
        // mcp/hooks default to empty arrays (camelCase contract).
        assert_eq!(v["mcp"].as_array().unwrap().len(), 0);

        // GET it back by scope.
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/setup?scope=org:acme")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["skills"][0], "code-review");
    }

    #[tokio::test]
    async fn get_setup_effective_merges_org_and_project() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let put = |scope: &str, skills: &str| {
            let body = format!(r#"{{"scope":"{scope}","skills":{skills}}}"#);
            Request::builder()
                .method("PUT")
                .uri("/api/setup")
                .header("authorization", "Bearer testtoken")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap()
        };
        app.clone()
            .oneshot(put("org:acme", r#"["code-review"]"#))
            .await
            .unwrap();
        app.clone()
            .oneshot(put("project:acme/web", r#"["react-doctor"]"#))
            .await
            .unwrap();

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/setup?org=acme&project=web")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let skills: Vec<String> = v["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect();
        assert_eq!(skills, vec!["code-review", "react-doctor"]);
    }

    #[tokio::test]
    async fn put_setup_rejects_invalid_scope() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/setup")
                    .header("authorization", "Bearer testtoken")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"scope":"nonsense"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_undeclared_setup_is_empty_not_404() {
        let (state, _d) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/setup?scope=org:ghost")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["scope"], "org:ghost");
        assert_eq!(v["skills"].as_array().unwrap().len(), 0);
    }

    // ---- restart test: cards survive replay from the log (C1 gate) ----

    #[test]
    fn cards_survive_restart_via_replay() {
        // Simulate the full lifecycle: append card events to a log, replay into
        // a fresh ViewManager, verify the card state is fully reconstructed.
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("log.redb");
        let log = Log::open(&log_path).unwrap();

        // Card 1: create → assign → claim → complete
        log.append(&Event::CardCreated {
            card_id: "c1".into(),
            board_id: "b1".into(),
            title: "First card".into(),
            created_at: 100.0,
        })
        .unwrap();
        log.append(&Event::CardAssigned {
            card_id: "c1".into(),
            assigned_id: "zephyr".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-1".into(),
            attempt_bookmark: "bm-1".into(),
            assigned_at: 101.0,
        })
        .unwrap();
        log.append(&Event::CardClaimed {
            card_id: "c1".into(),
            claimed_at: 102.0,
        })
        .unwrap();
        log.append(&Event::CardCompleted {
            card_id: "c1".into(),
            completed_at: 105.0,
        })
        .unwrap();

        // Card 2: create → assign → reassign (previous attempt forwarded)
        log.append(&Event::CardCreated {
            card_id: "c2".into(),
            board_id: "b1".into(),
            title: "Second card".into(),
            created_at: 200.0,
        })
        .unwrap();
        log.append(&Event::CardAssigned {
            card_id: "c2".into(),
            assigned_id: "zephyr".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-2a".into(),
            attempt_bookmark: "bm-2a".into(),
            assigned_at: 201.0,
        })
        .unwrap();
        log.append(&Event::CardReassigned {
            card_id: "c2".into(),
            assigned_id: "talos".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-2b".into(),
            attempt_bookmark: "bm-2b".into(),
            previous_session_id: "sess-2a".into(),
            reassigned_at: 210.0,
        })
        .unwrap();

        // Card 3: create → blocked
        log.append(&Event::CardCreated {
            card_id: "c3".into(),
            board_id: "b1".into(),
            title: "Third card".into(),
            created_at: 300.0,
        })
        .unwrap();
        log.append(&Event::CardBlocked {
            card_id: "c3".into(),
            blocked_by: vec!["c1".into(), "c2".into()],
            blocked_at: 301.0,
        })
        .unwrap();

        // Drop the log, reopen it (simulating restart), replay.
        drop(log);
        let reopened = Log::open(&log_path).unwrap();
        let mut views = ViewManager::new();
        views.replay(&reopened).unwrap();

        // Card 1: done, one completed attempt
        let c1 = views.cards.get("c1").expect("c1 must exist after replay");
        assert_eq!(c1.status, "done");
        assert_eq!(c1.title, "First card");
        assert_eq!(c1.attempts.len(), 1);
        assert_eq!(c1.attempts[0].outcome, "done");
        assert_eq!(c1.attempts[0].ended_at, Some(105.0));

        // Card 2: assigned (reassigned), two attempts, first closed
        let c2 = views.cards.get("c2").expect("c2 must exist after replay");
        assert_eq!(c2.status, "assigned");
        assert_eq!(c2.assigned_id.as_deref(), Some("talos"));
        assert_eq!(c2.current_session_id.as_deref(), Some("sess-2b"));
        assert_eq!(c2.attempts.len(), 2);
        assert_eq!(c2.attempts[0].session_id, "sess-2a");
        assert_eq!(c2.attempts[0].outcome, "reassigned");
        assert_eq!(c2.attempts[1].session_id, "sess-2b");
        assert!(c2.attempts[1].ended_at.is_none());

        // Card 3: blocked with deps
        let c3 = views.cards.get("c3").expect("c3 must exist after replay");
        assert_eq!(c3.status, "blocked");
        assert_eq!(c3.blocked_by, vec!["c1", "c2"]);

        // The board has 3 cards total
        let all = views.cards.list(&crate::views::CardFilters {
            board_id: Some("b1".into()),
            status: None,
            organization_id: None,
        });
        assert_eq!(all.len(), 3);
    }

    /// B-3 route-contract guard: every expected route must be reachable.
    /// This test exists because a prior manual merge (6549616) silently
    /// dropped the entire /api/repos surface while keeping the store + views
    /// intact — dead code that compiled fine. Walking the route table via
    /// HTTP requests catches that class of regression at build time.
    ///
    /// To add a route: add it here AND to build_router. If you forget
    /// either, this test fails.
    #[tokio::test]
    async fn route_contract_all_expected_routes_exist() {
        let (state, _dir) = test_state();
        let app = build_router(state);

        // (method, path, expected_status_range). We use 400/404 to confirm
        // the route exists (matched) without needing valid bodies.
        // NOTE: session "s1" exists in the test fixture, so /sessions/s1/*
        // routes return 200 for GET/POST.
        let cases: &[(&str, &str, &[u16])] = &[
            ("GET", "/api/sessions", &[200]),
            ("POST", "/api/sessions", &[200, 201, 400, 422]),
            ("GET", "/api/sessions/s1", &[200]),
            ("GET", "/api/sessions/nonexistent", &[404]),
            ("PATCH", "/api/sessions/s1", &[200]),
            ("POST", "/api/sessions/s1/fork", &[200, 409]),
            ("POST", "/api/sessions/s1/cancel", &[200, 409]),
            ("GET", "/api/sessions/s1/messages", &[200]),
            ("GET", "/api/search", &[200]),
            ("GET", "/api/models", &[200]),
            ("GET", "/api/agents", &[200]),
            ("GET", "/api/cards", &[200]),
            ("POST", "/api/cards", &[400, 422]),
            ("GET", "/api/cards/nonexistent", &[404]),
            ("POST", "/api/cards/nonexistent/assign", &[404]),
            ("POST", "/api/cards/nonexistent/claim", &[404, 500]), // TODO: should be 404
            ("POST", "/api/cards/nonexistent/block", &[404]),
            ("POST", "/api/cards/nonexistent/complete", &[404, 500]), // TODO: should be 404
            ("POST", "/api/cards/nonexistent/reassign", &[404]),
            ("GET", "/api/nodes", &[200]),
            ("GET", "/api/nodes/nonexistent/agents", &[200, 404]),
            (
                "POST",
                "/api/nodes/nonexistent/agents/refresh",
                &[200, 404, 501],
            ),
            ("POST", "/api/nodes/nonexistent/drain", &[404]),
            ("DELETE", "/api/nodes/nonexistent", &[404]),
            ("POST", "/api/enroll", &[200, 503]),
            ("GET", "/api/enroll/badtoken/install.sh", &[403]),
            ("GET", "/api/enroll/badtoken/binary", &[403]),
            ("GET", "/api/enroll/badtoken/status", &[403]),
            ("POST", "/api/enroll/badtoken", &[400, 403, 415, 422]),
            ("GET", "/api/vaults", &[200]),
            ("POST", "/api/vaults", &[400, 422]),
            ("GET", "/api/vaults/nonexistent/notes", &[404]),
            ("GET", "/api/vaults/nonexistent/documents", &[404]),
            ("GET", "/api/vaults/nonexistent/note", &[400, 404]),
            ("PUT", "/api/vaults/nonexistent/note", &[400, 404]),
            ("DELETE", "/api/vaults/nonexistent/note", &[400, 404]),
            ("GET", "/api/vaults/nonexistent/graph", &[404]),
            ("GET", "/api/vaults/nonexistent/collections", &[404]),
            ("GET", "/api/vaults/nonexistent/collections/p", &[404]),
            ("GET", "/api/projects", &[200]),
            ("POST", "/api/projects", &[400, 422]),
            ("GET", "/api/projects/nonexistent", &[404]),
            ("PATCH", "/api/projects/nonexistent", &[404]),
            ("DELETE", "/api/projects/nonexistent", &[404]),
            ("POST", "/api/sessions/s1/project", &[400, 404, 422]),
            // ── The regression class: repos were dropped once before ──
            ("GET", "/api/repos", &[200]),
            ("POST", "/api/repos", &[400, 422]),
            ("GET", "/api/repos/nonexistent", &[404]),
            ("DELETE", "/api/repos/nonexistent", &[404]),
            ("POST", "/api/sessions/s1/repos", &[400, 404, 422]),
            // ── Subsessions (B-2) ──
            ("GET", "/api/sessions/s1/subsessions", &[200]),
            (
                "POST",
                "/api/sessions/s1/subsessions",
                &[200, 201, 400, 422],
            ),
            ("POST", "/api/sessions/s1/complete", &[200, 400, 409]),
            ("GET", "/api/health", &[200]),
            ("GET", "/api/setup", &[200]),
            ("PUT", "/api/setup", &[400, 422]),
            ("GET", "/api/registry", &[200]),
        ];

        let mut missing: Vec<String> = Vec::new();
        for (method, path, acceptable) in cases {
            let req_method = match *method {
                "GET" => axum::http::Method::GET,
                "POST" => axum::http::Method::POST,
                "PATCH" => axum::http::Method::PATCH,
                "PUT" => axum::http::Method::PUT,
                "DELETE" => axum::http::Method::DELETE,
                _ => unreachable!(),
            };
            let req = axum::http::Request::builder()
                .method(req_method)
                .uri(*path)
                .header("authorization", "Bearer testtoken")
                .header("x-forwarded-for", "127.0.0.1")
                .body(axum::body::Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            let status = resp.status().as_u16();
            // 405 = route exists but method not allowed (also confirms match)
            if !acceptable.contains(&status) && status != 405 && status != 400 && status != 415 {
                missing.push(format!(
                    "{} {} → {} (expected {:?})",
                    method, path, status, acceptable
                ));
            }
        }

        assert!(
            missing.is_empty(),
            "route contract violations:\n  {}",
            missing.join("\n  ")
        );
    }

    /// The full enrollment flow against the router: mint (authed) → fetch the
    /// install script (token-gated, placeholders replaced) → register a node
    /// id (consumes the token, lands in hall.toml) → second registration with
    /// a different id is rejected. mint requires the hall iroh identity.
    #[tokio::test]
    async fn enroll_flow_mint_script_register() {
        let (mut state, dir) = test_state();
        state.hall_iroh_id = Some(Arc::new(
            "83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320".to_string(),
        ));
        let app = build_router(state.clone());

        // 1. Mint (operator-authed).
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/enroll")
                    .header("authorization", "Bearer testtoken")
                    .header("host", "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let token = v["token"].as_str().unwrap().to_string();
        assert!(v["command"]
            .as_str()
            .unwrap()
            .contains(&format!("/api/enroll/{token}/install.sh")));

        // Mint without auth is rejected (the mint endpoint is operator-only).
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/enroll")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // 2. Fetch the install script UNAUTHED (token in path is the capability).
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/enroll/{token}/install.sh"))
                    .header("host", "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let script = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let script = String::from_utf8(script.to_vec()).unwrap();
        assert!(script.contains(&token), "token baked into the script");
        assert!(
            !script.contains("{{HALL_URL}}"),
            "placeholders must be replaced"
        );
        assert!(script.contains("83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320"));

        // 3. Register a node id — lands in hall.toml.
        let envoy_id = "93141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499321";
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/enroll/{token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        "{{\"irohNodeId\":\"{envoy_id}\",\"nodeId\":\"test-node\"}}"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            crate::enroll::allowlist_list(dir.path()),
            vec![envoy_id.to_string()]
        );

        // 4. A DIFFERENT node id on the same token is rejected (single-use).
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/enroll/{token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        "{\"irohNodeId\":\"a3141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499322\"}",
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert_eq!(crate::enroll::allowlist_list(dir.path()).len(), 1);
    }

    /// mint_enroll fails honestly (503) when the hall has no iroh identity —
    /// a remote envoy couldn't connect anyway.
    #[tokio::test]
    async fn enroll_mint_without_iroh_is_503() {
        let (state, _dir) = test_state();
        let app = build_router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/enroll")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn organization_owner_can_mint_enrollment_with_login_cookie() {
        let (mut state, _dir) = test_state();
        state.hall_iroh_id = Some(Arc::new(
            "83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320".to_string(),
        ));
        state
            .auth_store
            .bootstrap_admin("owner", "correct-horse", "default", "Default")
            .unwrap();
        let principal = state
            .auth_store
            .authenticate("owner", "correct-horse")
            .unwrap()
            .unwrap();
        let session = state
            .auth_store
            .create_session(&principal.user_id, identity::unix_timestamp(), 60)
            .unwrap();

        let response = build_router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/enroll")
                    .header("cookie", format!("olympus_session={}", session.token))
                    .header("host", "127.0.0.1:8787")
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    /// DELETE /api/nodes/:id revokes the node's iroh key from hall.toml and
    /// deregisters it; the local node refuses removal.
    #[tokio::test]
    async fn remove_node_revokes_allowlist() {
        let (state, dir) = test_state();
        let iroh_id = "83141ef93390a387aec148672f7ae44a9ee4c02a0f23f82c0bb80fcc2e499320";
        crate::enroll::allowlist_add(dir.path(), iroh_id).unwrap();
        state
            .nodes
            .register(
                "remote-1",
                "remote-host",
                4,
                "0.1",
                false,
                crate::node::NodeTransport::Iroh,
                Some(iroh_id.to_string()),
                vec![],
            )
            .await;
        state
            .nodes
            .register(
                "local",
                "localhost",
                4,
                "0.1",
                true,
                crate::node::NodeTransport::Local,
                None,
                vec![],
            )
            .await;
        let app = build_router(state.clone());

        // Local node cannot be removed.
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/nodes/local")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // Remote node removal deregisters + revokes.
        let res = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/nodes/remote-1")
                    .header("authorization", "Bearer testtoken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(state.nodes.get("remote-1").await.is_none());
        assert!(crate::enroll::allowlist_list(dir.path()).is_empty());
    }
