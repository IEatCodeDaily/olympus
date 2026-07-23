use olympus_control_plane::event::Event;
use olympus_control_plane::log::{Log, StorageConfig};

fn postgres_log() -> Option<Log> {
    let dsn = std::env::var("OLYMPUS_TEST_POSTGRES_URL").ok()?;
    let mut client = postgres::Client::connect(&dsn, postgres::NoTls).unwrap();
    client
        .batch_execute(
            "DROP TABLE IF EXISTS hall_message_embeddings,hall_messages,hall_sessions,
             hall_events,hall_observed_messages,hall_observed_sessions,hall_envoy_watermarks,hall_schema_migrations CASCADE",
        )
        .unwrap();
    Some(Log::open_config(StorageConfig::Postgres(dsn)).unwrap())
}

fn message(session: &str, id: u64, content: &str) -> Event {
    Event::MessageAppended {
        session_id: session.into(),
        hermes_session_id: session.into(),
        message_id: id,
        role: "user".into(),
        content: Some(content.into()),
        tool_name: None,
        tool_calls: None,
        reasoning: None,
        timestamp: id as f64 + 2.0,
        token_count: None,
        finish_reason: None,
    }
}

#[test]
fn postgres_migrates_appends_projects_and_runs_lexical_plus_vector_search() {
    let Some(log) = postgres_log() else { return };
    log.append(&Event::SessionCreated {
        session_id: "s".into(),
        hermes_id: "h".into(),
        source: "olympus".into(),
        model: None,
        title: None,
        started_at: 1.0,
        message_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        agent: None,
        node: None,
    })
    .unwrap();
    log.append(&message("s", 0, "postgres lexical search"))
        .unwrap();
    log.append(&message("s", 1, "semantic neighbor")).unwrap();

    assert_eq!(log.event_count().unwrap(), 3);
    assert_eq!(log.read_all().unwrap().len(), 3);
    assert_eq!(log.list_sessions().unwrap()[0].session_id, "s");
    assert_eq!(log.search("lexical", 10).unwrap()[0].message_id, 0);
    assert!(log.accept_envoy_seq("runtime", 0).unwrap());
    assert!(!log.accept_envoy_seq("runtime", 0).unwrap());
    assert_eq!(log.envoy_watermark("runtime").unwrap(), Some(0));
    assert!(log
        .accept_observed("observed", 0, "s", Some(2), &message("s", 2, "buffered"))
        .unwrap());
    assert_eq!(log.envoy_watermark("observed").unwrap(), Some(0));
    assert_eq!(log.event_count().unwrap(), 4);

    log.set_embedding("s", 0, "test", &[1.0, 0.0, 0.0]).unwrap();
    log.set_embedding("s", 1, "test", &[0.0, 1.0, 0.0]).unwrap();
    assert_eq!(
        log.semantic_search("test", &[0.9, 0.1, 0.0], 10).unwrap()[0].message_id,
        0
    );
    assert_eq!(
        log.hybrid_search("neighbor", "test", &[0.0, 1.0, 0.0], 10)
            .unwrap()[0]
            .message_id,
        1
    );
}
