//! The core event types stored in the append-only log.
//!
//! See `docs/plans/2026-06-28-olympus-mvp.md` Task 1.1 for the exact spec.

use serde::{Deserialize, Serialize};

/// Events (v1 — MVP-scoped) that the log stores.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Event {
    /// A session was imported or created.
    SessionCreated {
        session_id: String,
        /// Hermes's session ID.
        hermes_id: String,
        /// "cli"|"telegram"|"discord"|"webui"|"cron"|"subagent"|"api_server"
        source: String,
        model: Option<String>,
        title: Option<String>,
        started_at: f64,
        message_count: u64,
        input_tokens: u64,
        output_tokens: u64,
        /// Agent (Hermes profile) bound to this session, if any. Olympus-created
        /// sessions can be created without one and have it assigned later.
        #[serde(default)]
        agent: Option<String>,
        /// Node the session's runtime runs on ("local" for now).
        #[serde(default)]
        node: Option<String>,
    },
    /// A message was appended to a session.
    MessageAppended {
        /// Olympus session ID.
        session_id: String,
        /// Hermes session ID.
        hermes_session_id: String,
        /// Monotonic within session.
        message_id: u64,
        /// "user"|"assistant"|"tool"|"system"
        role: String,
        /// Stored zstd-compressed in the log (decompressed by the log layer).
        content: Option<String>,
        tool_name: Option<String>,
        tool_calls: Option<String>,
        reasoning: Option<String>,
        timestamp: f64,
        token_count: Option<u64>,
        finish_reason: Option<String>,
    },
    /// A message was removed or tombstoned in Hermes.
    MessageRemoved {
        session_id: String,
        hermes_session_id: String,
        message_id: u64,
    },
    /// A session's metadata was updated (title, archived, model, etc).
    SessionUpdated {
        session_id: String,
        title: Option<String>,
        model: Option<String>,
        archived: Option<bool>,
        message_count: Option<u64>,
        /// Agent (Hermes profile) bound to this session. `None` = leave unchanged.
        agent: Option<String>,
        /// Node the session's runtime runs on ("local" for now). `None` = unchanged.
        node: Option<String>,
        /// Backfill the real Hermes session id once a lazily-spawned runtime
        /// captures it from `session/new`. `None` = leave unchanged.
        hermes_id: Option<String>,
        /// Manual pin flag (sidebar PINNED section). `None` = leave unchanged.
        #[serde(default)]
        pinned: Option<bool>,
    },
    // ---- Card lifecycle events (C1, ADR §6) ----
    /// A card was created on a board.
    CardCreated {
        card_id: String,
        board_id: String,
        title: String,
        created_at: f64,
    },
    /// A card was assigned to an agent or human, starting a session attempt.
    CardAssigned {
        card_id: String,
        assigned_id: String,
        /// "agent" | "user"
        assigned_kind: String,
        session_id: String,
        attempt_bookmark: String,
        assigned_at: f64,
    },
    /// A card was claimed (the assigned agent accepted it and began work).
    CardClaimed { card_id: String, claimed_at: f64 },
    /// A card was blocked by one or more dependencies.
    CardBlocked {
        card_id: String,
        blocked_by: Vec<String>,
        blocked_at: f64,
    },
    /// A card reached the done state.
    CardCompleted { card_id: String, completed_at: f64 },
    /// A card was reassigned to a new agent/session (previous attempt forwarded
    /// as a "previous attempt" block per ADR §6.2).
    CardReassigned {
        card_id: String,
        assigned_id: String,
        assigned_kind: String,
        session_id: String,
        attempt_bookmark: String,
        previous_session_id: String,
        reassigned_at: f64,
    },
    // ---- Session-tree events (ADR 0006 §7 footgun 3 resolution) ----
    /// A session was forked/branched from another. Records the parent→child
    /// relationship, fork type, and the message bookmark in the parent where
    /// the fork occurred. This is how the session tree is built: the event log
    /// is the authority on tree topology, and the SessionView projects it.
    ///
    /// Fork types:
    /// - "sub" — a sub-session (child of the same card, same agent, new leaf)
    /// - "fork" — a full fork (new session, may switch agent via handover)
    /// - "branch" — a branch from a specific message bookmark
    ///
    /// The card link: if the parent session has a card_id, the child inherits
    /// it (a card owns the whole tree, not just a leaf — ADR 0006 §7 footgun 3).
    SessionForked {
        parent_session_id: String,
        child_session_id: String,
        /// "sub" | "fork" | "branch"
        fork_type: String,
        /// Message bookmark in the parent where the fork occurred (the parent's
        /// message_id at the fork point). None = fork from session root.
        fork_point: Option<u64>,
        forked_at: f64,
    },
    /// A card was explicitly linked to a session (the tree root). This is how a
    /// card claims a session tree: the assigned session becomes the root, and
    /// all forks inherit the card_id via SessionForked propagation.
    ///
    /// A card owns the ENTIRE session tree. Forks/branches stay attached to the
    /// card. This is the resolution to footgun 3: the card↔session-tree link is
    /// a single root→tree ownership, not per-leaf.
    CardSessionLinked {
        card_id: String,
        session_id: String,
        linked_at: f64,
    },
    /// A session was handed over from one agent kind to another (ADR 0006 §9.1).
    /// This is the SOLE mechanism for switching harnesses mid-card: the source
    /// session is tombstoned (archived), and a new session is created with the
    /// target agent kind, inheriting the card_id + tree link. The history is
    /// translated to the target harness's context format.
    SessionHandover {
        source_session_id: String,
        target_session_id: String,
        /// Source agent kind (e.g. "Hermes").
        from_agent_kind: String,
        /// Target agent kind (e.g. "ClaudeCode").
        to_agent_kind: String,
        /// Number of messages translated from the source.
        translated_message_count: u64,
        handed_over_at: f64,
    },
    // ---- Declaration manifest events (ADR 0006 §3) ----
    /// A scope's agent-setup declaration was set or replaced. The `scope` is
    /// `"org:<org_slug>"` or `"project:<org_slug>/<project_slug>"`. This is the
    /// replicable unit: skills/mcp/plugins/hooks the envoy must materialize into
    /// every session under that scope. PUT semantics — a full replace of the
    /// scope's declared setup (idempotent).
    SetupDeclared {
        /// `"org:<org>"` | `"project:<org>/<project>"`.
        scope: String,
        /// Active skill slugs (refs into the skill library).
        skills: Vec<String>,
        /// Active MCP server slugs.
        mcp: Vec<String>,
        /// Active plugin slugs (LSP, codegraph, services, installers).
        plugins: Vec<String>,
        /// Active hook slugs.
        hooks: Vec<String>,
        declared_at: f64,
    },
    // ---- Registry events (ADR 0006 §9.4 — slug → definition) ----
    /// A registry entry was registered (or replaced). The registry is the
    /// authority: a slug → definition record that the adapter resolves before
    /// materializing. Syncs across nodes (the portable declaration). Drift
    /// detection warns when a node has a config not in the registry.
    ///
    /// `kind` = "skill" | "mcp" | "plugin" | "hook". `slug` is the immutable
    /// primary key within (kind, slug). `definition` is the harness-agnostic
    /// definition (JSON): MCP → `{command,args,env}`; skill → `{dir}` path or
    /// content ref; plugin → `{kind:install|service, ...}`; hook → harness-
    /// specific JSON. PUT semantics — full replace of the entry.
    EntryRegistered {
        kind: String,
        slug: String,
        definition: String, // JSON string (harness-agnostic)
        registered_at: f64,
    },
    // ---- Repo management events ----
    /// A git repo was registered in the repo registry.
    RepoRegistered {
        slug: String,
        url: String,
        default_branch: String,
        registered_at: f64,
    },
    /// A repo was removed from the registry.
    RepoRemoved { slug: String, removed_at: f64 },
    /// A repo was attached to a session's jj workspace.
    SessionRepoAttached {
        session_id: String,
        slug: String,
        attached_at: f64,
    },
    // ---- Project events (context container — vaults + repos + boards) ----
    /// A project was created.
    ProjectCreated {
        project_id: String,
        name: String,
        created_at: f64,
    },
    /// A project's metadata was updated (name, bound vaults/repos/boards).
    ProjectUpdated {
        project_id: String,
        name: Option<String>,
        vaults: Option<Vec<String>>,
        repos: Option<Vec<String>>,
        boards: Option<Vec<String>>,
    },
    /// A project was deleted (tombstoned).
    ProjectDeleted { project_id: String, deleted_at: f64 },
    /// A session was attached to a project.
    SessionProjectAttached {
        session_id: String,
        project_id: String,
        attached_at: f64,
    },
    /// Assigns a session to its hard tenancy boundary. Kept as an appended
    /// variant retained for compatibility with historical event meaning.
    SessionOrganizationAssigned {
        session_id: String,
        organization_id: String,
    },
    /// Assigns a project to its organization without reshaping the historical
    /// ProjectCreated payload while keeping schema evolution additive.
    ProjectOrganizationAssigned {
        project_id: String,
        organization_id: String,
    },
    /// Assigns a kanban card to its organization. Cards are independently
    /// scoped because board ids are currently labels, not durable resources.
    CardOrganizationAssigned {
        card_id: String,
        organization_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_created_json_roundtrips() {
        let e = Event::SessionCreated {
            session_id: "sess-1".into(),
            hermes_id: "h-1".into(),
            source: "cli".into(),
            model: Some("glm-5.2".into()),
            title: Some("hello".into()),
            started_at: 1_700_000_000.0,
            message_count: 0,
            input_tokens: 10,
            output_tokens: 20,
            agent: None,
            node: None,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn setup_declared_json_roundtrips() {
        let e = Event::SetupDeclared {
            scope: "project:acme/web".into(),
            skills: vec!["code-review".into(), "react-doctor".into()],
            mcp: vec!["gitnexus".into()],
            plugins: vec!["lsp-rust".into(), "codegraph".into()],
            hooks: vec!["pre-commit-verify".into()],
            declared_at: 1_782_900_000.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn entry_registered_json_roundtrips() {
        let e = Event::EntryRegistered {
            kind: "mcp".into(),
            slug: "gitnexus".into(),
            definition: r#"{"command":"gitnexus","args":["--stdio"],"env":{}}"#.into(),
            registered_at: 1_782_900_001.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn message_appended_json_roundtrips_with_none_fields() {
        let e = Event::MessageAppended {
            session_id: "sess-1".into(),
            hermes_session_id: "h-1".into(),
            message_id: 5,
            role: "user".into(),
            content: Some("hi there".into()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: 1_700_000_001.0,
            token_count: Some(3),
            finish_reason: None,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn session_updated_json_roundtrips() {
        let e = Event::SessionUpdated {
            session_id: "sess-1".into(),
            title: Some("renamed".into()),
            model: None,
            archived: Some(true),
            message_count: Some(42),
            agent: None,
            node: None,
            hermes_id: None,
            pinned: None,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn message_removed_json_roundtrips() {
        let e = Event::MessageRemoved {
            session_id: "sess-1".into(),
            hermes_session_id: "h-1".into(),
            message_id: 5,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    // ---- Card event roundtrips (C1) ----

    #[test]
    fn card_created_roundtrips() {
        let e = Event::CardCreated {
            card_id: "card-1".into(),
            board_id: "board-1".into(),
            title: "Implement cards".into(),
            created_at: 1_700_000_000.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_assigned_roundtrips() {
        let e = Event::CardAssigned {
            card_id: "card-1".into(),
            assigned_id: "agent-zephyr".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-1".into(),
            attempt_bookmark: "attempt-1".into(),
            assigned_at: 1_700_000_001.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_claimed_roundtrips() {
        let e = Event::CardClaimed {
            card_id: "card-1".into(),
            claimed_at: 1_700_000_002.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_blocked_roundtrips() {
        let e = Event::CardBlocked {
            card_id: "card-1".into(),
            blocked_by: vec!["card-0".into(), "card-2".into()],
            blocked_at: 1_700_000_003.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_completed_roundtrips() {
        let e = Event::CardCompleted {
            card_id: "card-1".into(),
            completed_at: 1_700_000_004.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_reassigned_roundtrips() {
        let e = Event::CardReassigned {
            card_id: "card-1".into(),
            assigned_id: "agent-talos".into(),
            assigned_kind: "agent".into(),
            session_id: "sess-2".into(),
            attempt_bookmark: "attempt-2".into(),
            previous_session_id: "sess-1".into(),
            reassigned_at: 1_700_000_005.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    // ---- Session-tree event roundtrips ----

    #[test]
    fn session_forked_roundtrips() {
        let e = Event::SessionForked {
            parent_session_id: "sess-1".into(),
            child_session_id: "sess-2".into(),
            fork_type: "fork".into(),
            fork_point: Some(5),
            forked_at: 1_700_000_010.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn session_forked_no_bookmark_roundtrips() {
        let e = Event::SessionForked {
            parent_session_id: "root".into(),
            child_session_id: "sub-1".into(),
            fork_type: "sub".into(),
            fork_point: None,
            forked_at: 1_700_000_011.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn card_session_linked_roundtrips() {
        let e = Event::CardSessionLinked {
            card_id: "card-1".into(),
            session_id: "sess-1".into(),
            linked_at: 1_700_000_012.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn session_handover_roundtrips() {
        let e = Event::SessionHandover {
            source_session_id: "sess-1".into(),
            target_session_id: "sess-2".into(),
            from_agent_kind: "Hermes".into(),
            to_agent_kind: "ClaudeCode".into(),
            translated_message_count: 15,
            handed_over_at: 1_700_000_020.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    // ---- Repo management event roundtrips ----

    #[test]
    fn repo_registered_roundtrips() {
        let e = Event::RepoRegistered {
            slug: "olympus".into(),
            url: "https://github.com/user/olympus".into(),
            default_branch: "main".into(),
            registered_at: 1_700_000_100.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn repo_removed_roundtrips() {
        let e = Event::RepoRemoved {
            slug: "old-repo".into(),
            removed_at: 1_700_000_200.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn session_repo_attached_roundtrips() {
        let e = Event::SessionRepoAttached {
            session_id: "sess-1".into(),
            slug: "olympus".into(),
            attached_at: 1_700_000_300.0,
        };
        let bytes = serde_json::to_vec(&e).unwrap();
        let back: Event = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn project_and_organization_variants_json_roundtrip() {
        let events = vec![
            Event::ProjectCreated {
                project_id: "p".into(),
                name: "Project".into(),
                created_at: 1.0,
            },
            Event::ProjectUpdated {
                project_id: "p".into(),
                name: Some("Renamed".into()),
                vaults: Some(vec!["v".into()]),
                repos: Some(vec!["r".into()]),
                boards: Some(vec!["b".into()]),
            },
            Event::ProjectDeleted {
                project_id: "p".into(),
                deleted_at: 2.0,
            },
            Event::SessionProjectAttached {
                session_id: "s".into(),
                project_id: "p".into(),
                attached_at: 3.0,
            },
            Event::SessionOrganizationAssigned {
                session_id: "s".into(),
                organization_id: "o".into(),
            },
            Event::ProjectOrganizationAssigned {
                project_id: "p".into(),
                organization_id: "o".into(),
            },
            Event::CardOrganizationAssigned {
                card_id: "c".into(),
                organization_id: "o".into(),
            },
        ];
        for event in events {
            let bytes = serde_json::to_vec(&event).unwrap();
            assert_eq!(serde_json::from_slice::<Event>(&bytes).unwrap(), event);
        }
    }
}
