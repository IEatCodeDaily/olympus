//! Session lifecycle + messaging routes (`/api/sessions/**`).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use super::support::{append_and_apply, now_epoch};
use crate::bridge::{AgentCommand, AgentEvent};
use crate::server::capability::CapabilitySet;
use crate::server::dto::{MessageDto, SessionDto};
use crate::server::principal::{OrgScope, Principal};
use crate::server::ws::ServerFrame;
use crate::server::AppState;
use crate::views::Filters;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", get(get_session).patch(patch_session))
        .route("/api/sessions/{id}/fork", post(fork_session))
        .route("/api/sessions/{id}/handover", post(handover_session))
        .route(
            "/api/sessions/{id}/messages",
            get(get_messages).post(post_message),
        )
        .route("/api/sessions/{id}/cancel", post(cancel_session))
        .route("/api/sessions/{id}/steer", post(steer_session))
        .route(
            "/api/sessions/{id}/permission",
            post(respond_permission_handler),
        )
        .route("/api/sessions/{id}/project", post(attach_session_project))
        .route("/api/sessions/{id}/repos", post(attach_repo))
        .route(
            "/api/sessions/{id}/subsessions",
            get(list_subsessions).post(create_subsession),
        )
        .route("/api/sessions/{id}/complete", post(complete_session))
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionsQuery {
    source: Option<String>,
    archived: Option<bool>,
    pinned: Option<bool>,
    /// Filter by managed status: `true` → Olympus-driven sessions (your active
    /// workspace), `false` → imported agent history (read-only, fork-to-use).
    /// Absent → both. This is the basis of the Sessions/History nav split.
    managed: Option<bool>,
    /// `lastActivity` (default) | `startedAt` | `messageCount`, all descending.
    sort: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MessagesQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PostMessageBody {
    #[serde(alias = "content")]
    text: String,
    #[serde(default)]
    model: Option<String>,
    /// Thinking/reasoning effort level ("low" | "medium" | "high"). Delivered
    /// to the Hermes ACP adapter as a /thinking slash command before the
    /// prompt. Absent/None = leave the session's current setting alone.
    #[serde(default)]
    thinking: Option<String>,
}

/// Body for `POST /api/sessions` — optional agent/node binding at creation. All
/// fields optional so a bare `{}` creates an unbound draft.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateSessionBody {
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    capabilities: Option<CapabilitySet>,
}

/// Body for `PATCH /api/sessions/:id` — bind/rebind agent, node, model, or title
/// before the first send. All fields optional; only present fields are changed.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PatchSessionBody {
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    capabilities: Option<CapabilitySet>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForkSessionBody {
    #[serde(default)]
    fork_type: Option<String>,
    #[serde(default)]
    capabilities: Option<CapabilitySet>,
}

/// Request body for POST /api/sessions/:id/handover.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoverBody {
    /// Target agent kind: "claude-code", "codex", or "hermes".
    to_agent_kind: String,
    /// Optional model override for the target session.
    #[serde(default)]
    model: Option<String>,
}

/// Body for POST /api/sessions/:id/steer — inject guidance into a RUNNING turn.
#[derive(Debug, Deserialize)]
pub(crate) struct SteerBody {
    text: String,
}

/// Body for POST /api/sessions/:id/permission — the user's decision on a
/// pending `session/request_permission`. `optionId` selects an option; omit it
/// (or send null) to cancel the request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionBody {
    option_id: Option<String>,
}

/// Body for `POST /api/sessions/:id/subsessions`.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct CreateSubsessionBody {
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    capabilities: Option<CapabilitySet>,
}

fn assigned_by(principal: &Principal) -> String {
    match principal {
        Principal::Operator => "operator".into(),
        Principal::User { user_id, .. } => format!("user:{user_id}"),
    }
}

fn capability_error(error: &'static str, message: impl Into<String>) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({ "error": error, "message": message.into() })),
    )
        .into_response()
}

#[allow(clippy::result_large_err)]
fn signed_capability_event(
    state: &AppState,
    session_id: String,
    mut capabilities: CapabilitySet,
    principal: &Principal,
    parent_session_id: Option<String>,
) -> Result<crate::event::Event, Response> {
    state
        .capability_signer
        .sign(&mut capabilities)
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid_capabilities",
                    "message": error.to_string(),
                })),
            )
                .into_response()
        })?;
    Ok(crate::event::Event::SessionCapabilitiesAssigned {
        session_id,
        capabilities,
        assigned_by: assigned_by(principal),
        parent_session_id,
    })
}

/// Body for `POST /api/sessions/:id/complete` — the check gate.
#[derive(Debug, Deserialize)]
pub(crate) struct CompleteBody {
    verdict: String,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AttachRepoBody {
    slug: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachProjectBody {
    project_id: String,
}

pub(crate) async fn list_sessions(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
    Query(q): Query<SessionsQuery>,
) -> impl IntoResponse {
    let views = state.views.read().await;
    // `source` may be a comma-separated multi-select; the view filter takes one
    // value, so we filter the post-list set for multi.
    let sources: Option<Vec<String>> = q
        .source
        .as_ref()
        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect());

    let filters = Filters {
        source: None,
        archived: q.archived,
        pinned: q.pinned,
    };
    let mut rows: Vec<SessionDto> = views
        .sessions
        .list(&filters)
        .into_iter()
        .filter(|row| {
            scope
                .as_ref()
                .is_none_or(|scope| row.org_id == scope.0.organization_id)
        })
        .filter(|r| match &sources {
            Some(list) if !list.is_empty() => list.iter().any(|s| s == &r.source),
            _ => true,
        })
        .map(SessionDto::from_row)
        // Apply the managed filter (Sessions vs History nav split). Within
        // managed, hide phantom duplicates: legacy re-imported sessions that are
        // tagged source=olympus but were never driven by Olympus (agent unset and
        // hermes_id == id — the pre-dedup signature). They read as managed but
        // aren't real workspaces; the History view is their honest home.
        .filter(|dto| {
            let is_managed = dto.source == "acp" || dto.source == "olympus";
            let is_phantom = is_managed
                && dto.agent.is_none()
                && !dto.hermes_id.is_empty()
                && dto.hermes_id == dto.id;
            match q.managed {
                Some(true) => is_managed && !is_phantom,
                Some(false) => !is_managed || is_phantom,
                None => true,
            }
        })
        .collect();
    drop(views);

    // Stamp derived liveness. Managed sessions use the authoritative in-flight
    // + awaiting-input flags; observed sessions fall back to activity recency.
    let in_flight = state.bridge.in_flight_set().await;
    let awaiting = state.bridge.awaiting_input_set().await;
    let now = now_epoch();
    for r in rows.iter_mut() {
        let managed = r.source == "acp" || r.source == "olympus";
        r.liveness = crate::server::dto::compute_liveness(
            r.last_activity,
            now,
            in_flight.contains(&r.id),
            managed,
            awaiting.contains(&r.id),
        )
        .to_string();
    }

    // Apply the requested sort (all descending). Default = lastActivity.
    // The view returns started_at-desc order; we re-sort here so the UI's
    // sort selector (lastActivity | startedAt | messageCount) takes effect.
    match q.sort.as_deref() {
        Some("startedAt") => rows.sort_by(|a, b| {
            b.started_at
                .partial_cmp(&a.started_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        }),
        Some("messageCount") => rows.sort_by(|a, b| {
            b.message_count
                .cmp(&a.message_count)
                .then_with(|| a.id.cmp(&b.id))
        }),
        // "lastActivity" and anything unrecognized (incl. None) -> lastActivity desc.
        _ => rows.sort_by(|a, b| {
            b.last_activity
                .partial_cmp(&a.last_activity)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        }),
    }

    let total = rows.len();
    if let Some(limit) = q.limit {
        rows.truncate(limit);
    }

    Json(json!({ "sessions": rows, "nextCursor": serde_json::Value::Null, "total": total }))
}

pub(crate) async fn get_session(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
    Path(id): Path<String>,
) -> Response {
    let views = state.views.read().await;
    match views.sessions.get(&id) {
        Some(row)
            if scope
                .as_ref()
                .is_none_or(|scope| row.org_id == scope.0.organization_id) =>
        {
            let mut dto = SessionDto::from_row(row);
            drop(views);
            let in_flight = state.bridge.in_flight_set().await;
            let awaiting = state.bridge.awaiting_input_set().await;
            let managed = dto.source == "acp" || dto.source == "olympus";
            dto.liveness = crate::server::dto::compute_liveness(
                dto.last_activity,
                now_epoch(),
                in_flight.contains(&dto.id),
                managed,
                awaiting.contains(&dto.id),
            )
            .to_string();
            Json(dto).into_response()
        }
        _ => (StatusCode::NOT_FOUND, "session not found").into_response(),
    }
}

/// Extract the timestamp from a MessageAppended event (for DTO building).
pub(crate) fn event_timestamp(event: &crate::event::Event) -> f64 {
    match event {
        crate::event::Event::MessageAppended { timestamp, .. } => *timestamp,
        _ => now_epoch(),
    }
}

/// Derive a short human title from the first user message. First non-empty
/// line, collapsed whitespace, trimmed to ~60 chars on a word boundary. This
/// is a cheap heuristic (no LLM round-trip) so titles appear instantly instead
/// of "Untitled". A nicer LLM-summarized title can replace this later.
pub(crate) fn derive_title(text: &str) -> String {
    let first_line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let collapsed = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 60;
    if collapsed.chars().count() <= MAX {
        return collapsed;
    }
    // Trim to MAX chars, then back off to the last word boundary for cleanliness.
    let truncated: String = collapsed.chars().take(MAX).collect();
    let cut = truncated.rfind(' ').unwrap_or(truncated.len());
    format!("{}…", truncated[..cut].trim_end())
}

pub(crate) async fn get_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MessagesQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50);

    // Lazy history (ADR 0009): for non-managed (observed) sessions, read
    // messages on-demand from the Hermes state.db instead of the in-memory
    // MessageView. Managed (olympus/acp) sessions still use the in-memory
    // view because their messages are live (streamed from the bridge).
    if let Some(ref reader) = state.state_db {
        let views = state.views.read().await;
        let is_managed = views.sessions.is_managed(&id);
        drop(views);
        if !is_managed {
            match reader.recent_messages(&id, limit) {
                Ok(rows) => {
                    let messages: Vec<MessageDto> = rows
                        .iter()
                        .map(|row| MessageDto::from_row(&id, row))
                        .collect();
                    return Json(
                        json!({ "messages": messages, "nextCursor": serde_json::Value::Null }),
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, session = %id, "state.db message read failed, falling back to views");
                }
            }
        }
    }

    let views = state.views.read().await;
    let messages: Vec<MessageDto> = views
        .messages
        .recent(&id, limit)
        .into_iter()
        .map(|row| MessageDto::from_row(&id, row))
        .collect();
    Json(json!({ "messages": messages, "nextCursor": serde_json::Value::Null }))
}

/// POST /api/sessions — create a new Olympus-managed chat session **optimistically**.
///
/// Returns instantly with the new Session DTO (201). No agent runtime is
/// spawned — the expensive ACP handshake is deferred to the first send
/// (`ensure_runtime`). The session can be assigned an agent/node at creation
/// (via the body) or later via PATCH, any time before the first send.
pub(crate) async fn create_session(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
    body: Option<Json<CreateSessionBody>>,
) -> Response {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    if let Some(node_id) = body.node.as_deref() {
        match state.nodes.get(node_id).await {
            Some(node) if node.status == crate::node::NodeStatus::Online => {}
            Some(node) => {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "node_unavailable",
                        "message": format!("Node {node_id} is {:?}; choose an online node", node.status),
                    })),
                )
                    .into_response();
            }
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({
                        "error": "unknown_node",
                        "message": format!("Unknown node {node_id}"),
                    })),
                )
                    .into_response();
            }
        }
    }
    let spec = crate::server::bridge_mgr::RuntimeSpec {
        agent: body.agent.clone(),
        node: body.node.clone(),
        cwd: None,
        mcp_servers: vec![],
        env: vec![],
    };
    let organization_id = scope.as_ref().map(|scope| scope.0.organization_id.as_str());
    match state.bridge.create_draft(&spec, organization_id) {
        Ok(ns) => {
            // Apply the one SessionCreated event directly into the view — do NOT
            // re-scan the whole log (that's O(all events) and made create slow).
            let created = crate::event::Event::SessionCreated {
                session_id: ns.session_id.clone(),
                hermes_id: ns.hermes_id.clone(),
                source: "olympus".into(),
                model: None,
                title: None,
                started_at: ns.started_at,
                message_count: 0,
                input_tokens: 0,
                output_tokens: 0,
                agent: body.agent.clone(),
                node: body.node.clone(),
            };
            let organization =
                scope.map(|scope| crate::event::Event::SessionOrganizationAssigned {
                    session_id: ns.session_id.clone(),
                    organization_id: scope.0.organization_id,
                });
            let capability_event = match body.capabilities.clone() {
                Some(capabilities) => match signed_capability_event(
                    &state,
                    ns.session_id.clone(),
                    capabilities,
                    &Principal::Operator,
                    None,
                ) {
                    Ok(event) => Some(event),
                    Err(response) => return response,
                },
                None => None,
            };
            if let Some(event) = capability_event.as_ref() {
                if let Err(error) = state.log.append(event) {
                    tracing::error!(%error, "persisting session capabilities");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                }
            }
            let dto = {
                let mut views = state.views.write().await;
                views.apply(&created);
                if let Some(event) = organization.as_ref() {
                    views.apply(event);
                }
                if let Some(event) = capability_event.as_ref() {
                    views.apply(event);
                }
                views
                    .sessions
                    .get(&ns.session_id)
                    .map(SessionDto::from_row)
                    .unwrap_or_else(|| SessionDto {
                        id: ns.session_id.clone(),
                        hermes_id: ns.hermes_id.clone(),
                        org_id: organization
                            .as_ref()
                            .and_then(|event| match event {
                                crate::event::Event::SessionOrganizationAssigned {
                                    organization_id,
                                    ..
                                } => Some(organization_id.clone()),
                                _ => None,
                            })
                            .unwrap_or_else(|| "personal".into()),
                        owner_id: "rpw".into(),
                        context_id: None,
                        source: "olympus".into(),
                        model: None,
                        title: None,
                        started_at: ns.started_at,
                        last_activity: ns.started_at,
                        message_count: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        archived: false,
                        pinned: false,
                        forked_from: None,
                        fork_point: None,
                        fork_type: None,
                        managed: true,
                        agent: body.agent.clone(),
                        node: body.node.clone(),
                        liveness: "active".to_string(),
                        parent_session_id: None,
                        card_id: None,
                        capabilities: None,
                    })
            };

            // A freshly-created managed draft has no in-flight turn yet → idle.
            let mut dto = dto;
            let managed = dto.source == "acp" || dto.source == "olympus";
            dto.liveness = crate::server::dto::compute_liveness(
                dto.last_activity,
                now_epoch(),
                false,
                managed,
                false,
            )
            .to_string();

            let _ = state.deltas.send(ServerFrame::SessionAdded {
                session: dto.clone(),
            });

            (
                StatusCode::CREATED,
                Json(serde_json::to_value(&dto).unwrap()),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "bridge create_draft failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "bridge_error",
                    "message": format!("Failed to create session: {e:#}"),
                })),
            )
                .into_response()
        }
    }
}

/// PATCH /api/sessions/:id — bind/rebind agent, node, model, or title.
///
/// Appends a `SessionUpdated` event and broadcasts the change. Intended to be
/// called before the first send (the typical optimistic-create flow: create
/// instantly, pick agent/model, then send). Rebinding the agent after a runtime
/// has spawned takes effect on the next runtime (not yet hot-swapped).
pub(crate) async fn patch_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<PatchSessionBody>>,
) -> Response {
    let body = body.map(|Json(b)| b).unwrap_or_default();

    // The session must exist. Runtime rebinds (agent/node/model) are managed-
    // only; pin/archive/title are metadata and work on ANY session (observed
    // sessions can be pinned or archived without being steerable).
    let capability_event = {
        let views = state.views.read().await;
        let Some(row) = views.sessions.get(&id) else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let wants_rebind = body.agent.is_some() || body.node.is_some() || body.model.is_some();
        if wants_rebind && !(row.source == "olympus" || row.source == "acp") {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "observed",
                    "message": "Observed sessions can't be reassigned. Fork it first.",
                })),
            )
                .into_response();
        }
        match body.capabilities.clone() {
            None => None,
            Some(requested) => {
                if let Some(existing) = row.capabilities.as_ref() {
                    if !requested.is_narrower_than(existing) {
                        return capability_error(
                            "capability_expansion",
                            "session capabilities may only be narrowed",
                        );
                    }
                } else if !views.messages.recent(&id, 1).is_empty() || !row.hermes_id.is_empty() {
                    return capability_error(
                        "capabilities_locked",
                        "capabilities may be first assigned only before the agent starts",
                    );
                }
                match signed_capability_event(
                    &state,
                    id.clone(),
                    requested,
                    &Principal::Operator,
                    None,
                ) {
                    Ok(event) => Some(event),
                    Err(response) => return response,
                }
            }
        }
    };

    let event = crate::event::Event::SessionUpdated {
        session_id: id.clone(),
        title: body.title.clone(),
        model: body.model.clone(),
        archived: body.archived,
        message_count: None,
        agent: body.agent.clone(),
        node: body.node.clone(),
        hermes_id: None,
        pinned: body.pinned,
    };
    if let Err(e) = state.log.append(&event) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "log_error", "message": format!("{e:#}") })),
        )
            .into_response();
    }
    if let Some(capability_event) = capability_event.as_ref() {
        if let Err(e) = state.log.append(capability_event) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "log_error", "message": format!("{e:#}") })),
            )
                .into_response();
        }
    }

    let dto = {
        let mut views = state.views.write().await;
        views.apply(&event);
        if let Some(capability_event) = capability_event.as_ref() {
            views.apply(capability_event);
        }
        views.sessions.get(&id).map(SessionDto::from_row)
    };

    let mut changes = serde_json::Map::new();
    if let Some(a) = &body.agent {
        changes.insert("agent".into(), serde_json::Value::String(a.clone()));
    }
    if let Some(n) = &body.node {
        changes.insert("node".into(), serde_json::Value::String(n.clone()));
    }
    if let Some(m) = &body.model {
        changes.insert("model".into(), serde_json::Value::String(m.clone()));
    }
    if let Some(t) = &body.title {
        changes.insert("title".into(), serde_json::Value::String(t.clone()));
    }
    if let Some(a) = body.archived {
        changes.insert("archived".into(), serde_json::Value::Bool(a));
    }
    if let Some(p) = body.pinned {
        changes.insert("pinned".into(), serde_json::Value::Bool(p));
    }
    let _ = state.deltas.send(ServerFrame::SessionUpdated {
        session_id: id.clone(),
        changes: serde_json::Value::Object(changes),
    });

    match dto {
        Some(dto) => Json(serde_json::to_value(&dto).unwrap()).into_response(),
        None => (StatusCode::NOT_FOUND, "session not found").into_response(),
    }
}

/// POST /api/sessions/:id/fork — fork an observed session into Olympus.
pub(crate) async fn fork_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ForkSessionBody>,
) -> Response {
    let (source, messages) = {
        let views = state.views.read().await;
        let Some(source) = views.sessions.get(&id).cloned() else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let messages = views
            .messages
            .recent(&id, usize::MAX)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        (source, messages)
    };

    let child_capabilities = match (&source.capabilities, body.capabilities) {
        (Some(parent), Some(mut requested)) => {
            if !parent.can_fork {
                return capability_error("capability_denied", "parent lacks session.fork");
            }
            requested.signature.clear();
            let effective = CapabilitySet::intersect(parent, &requested);
            if effective != requested {
                return capability_error(
                    "capability_expansion",
                    "requested child capabilities exceed the parent",
                );
            }
            Some(effective)
        }
        (Some(parent), None) => {
            if !parent.can_fork {
                return capability_error("capability_denied", "parent lacks session.fork");
            }
            let mut inherited = parent.clone();
            inherited.signature.clear();
            Some(inherited)
        }
        (None, requested) => requested,
    };
    let fork_type = body.fork_type.unwrap_or_else(|| "sub".to_string());
    let fork = match state
        .bridge
        .fork_session(
            &source.hermes_id,
            source.model.clone(),
            source.title.clone(),
            messages.len() as u64,
            Some(&source.org_id),
        )
        .await
    {
        Ok(fork) => fork,
        Err(e) => {
            tracing::error!(error = %e, source_session = %id, "bridge fork_session failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "bridge_error",
                    "message": format!("Failed to fork agent session: {e:#}"),
                })),
            )
                .into_response();
        }
    };

    for (idx, msg) in messages.iter().enumerate() {
        if let Err(e) = state.log.append(&crate::event::Event::MessageAppended {
            session_id: fork.session_id.clone(),
            hermes_session_id: fork.hermes_id.clone(),
            message_id: idx as u64,
            role: msg.role.clone(),
            content: msg.content.clone(),
            tool_name: msg.tool_name.clone(),
            tool_calls: None,
            reasoning: None,
            timestamp: msg.timestamp,
            token_count: msg.token_count,
            finish_reason: None,
        }) {
            tracing::warn!(error = %e, fork_session = %fork.session_id, "failed to append forked message");
        }
    }

    let mut dto = {
        let mut views = state.views.write().await;
        if let Ok(events) = state.log.read_all() {
            for (_seq, event) in events {
                match &event {
                    crate::event::Event::SessionCreated { session_id, .. }
                    | crate::event::Event::MessageAppended { session_id, .. }
                    | crate::event::Event::SessionUpdated { session_id, .. }
                    | crate::event::Event::SessionOrganizationAssigned { session_id, .. }
                        if session_id == &fork.session_id =>
                    {
                        views.apply(&event);
                    }
                    _ => {}
                }
            }
        }
        match views.sessions.get(&fork.session_id) {
            Some(row) => SessionDto::from_row(row),
            None => SessionDto {
                id: fork.session_id.clone(),
                hermes_id: fork.hermes_id.clone(),
                org_id: source.org_id.clone(),
                owner_id: "rpw".into(),
                context_id: None,
                source: "olympus".into(),
                model: source.model.clone(),
                title: source.title.clone(),
                started_at: 0.0,
                last_activity: 0.0,
                message_count: messages.len() as u64,
                input_tokens: 0,
                output_tokens: 0,
                archived: false,
                pinned: false,
                forked_from: None,
                fork_point: None,
                fork_type: None,
                managed: true,
                agent: None,
                node: None,
                liveness: "active".to_string(),
                parent_session_id: None,
                card_id: None,
                capabilities: None,
            },
        }
    };
    dto.forked_from = Some(id.clone());
    dto.fork_type = Some(fork_type.clone());
    dto.parent_session_id = Some(id.clone());

    // Emit SessionForked so the session tree is durable (ADR 0006 §7 footgun 3).
    // The child inherits the parent's card_id (if any) via the view projection.
    let forked_event = crate::event::Event::SessionForked {
        parent_session_id: id.clone(),
        child_session_id: fork.session_id.clone(),
        fork_type: fork_type.clone(),
        fork_point: None,
        forked_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
    };
    let capability_event = match child_capabilities {
        Some(capabilities) => match signed_capability_event(
            &state,
            fork.session_id.clone(),
            capabilities,
            &Principal::Operator,
            Some(id.clone()),
        ) {
            Ok(event) => Some(event),
            Err(response) => return response,
        },
        None => None,
    };
    if let Err(e) = state.log.append(&forked_event) {
        tracing::warn!(error = %e, "failed to append SessionForked event");
    }
    {
        let mut views = state.views.write().await;
        views.apply(&forked_event);
        if let Some(event) = capability_event.as_ref() {
            if let Err(error) = state.log.append(event) {
                tracing::error!(%error, "persisting fork capabilities");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            views.apply(event);
        }
    }
    // Re-read the child row to get projected inherited state.
    if let Some(child_row) = {
        let views = state.views.read().await;
        views.sessions.get(&fork.session_id).cloned()
    } {
        dto.card_id = child_row.card_id.clone();
        dto.capabilities = child_row.capabilities.clone().map(Box::new);
    }

    let _ = state.deltas.send(ServerFrame::SessionAdded {
        session: dto.clone(),
    });

    Json(json!({ "session": dto })).into_response()
}

/// POST a message to drive a session.
///
/// Only MANAGED (olympus/acp-source) sessions are steerable. Observed sessions
/// (imported telegram/cli/etc.) return 409 — the UI must FORK them into an
/// olympus-owned session first (cross-channel continuation, ADR §6.6).
///
/// For managed sessions the prompt is sent to the agent runtime and the response
/// is streamed over /ws as message.delta / message.done frames. Returns 202.
pub(crate) async fn post_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PostMessageBody>,
) -> Response {
    let (managed, hermes_id, agent, node, organization_id) = {
        let views = state.views.read().await;
        let Some(session) = views.sessions.get(&id) else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let managed = session.source == "olympus" || session.source == "acp";
        (
            managed,
            session.hermes_id.clone(),
            session.agent.clone(),
            session.node.clone(),
            session.org_id.clone(),
        )
    };

    if !managed {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "observed",
                "message": "This session is observed (read-only). Fork it into an Olympus-managed session to continue.",
            })),
        )
            .into_response();
    }

    // Record the user message in the log + views + broadcast IMMEDIATELY, before
    // any (potentially slow) runtime spawn — so the UI shows the user bubble and
    // the POST returns fast. `hermes_id` may be empty here for a fresh draft; the
    // user message carries the current (possibly empty) hermes id and is fine.
    // Use max(existing message_id)+1, NOT the count — message ids must be
    // monotonic and collision-free even if the hot window evicted older rows or
    // ids aren't contiguous (a count would reuse an id and clobber a message).
    let next_id = {
        let views = state.views.read().await;
        views
            .messages
            .recent(&id, usize::MAX)
            .iter()
            .map(|m| m.message_id)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0)
    };
    match state
        .bridge
        .append_user_message(&id, &hermes_id, next_id, &body.text)
    {
        Ok(event) => {
            {
                let mut views = state.views.write().await;
                views.apply(&event);
            }
            let dto = crate::server::dto::MessageDto {
                message_id: next_id,
                session_id: id.clone(),
                role: "user".into(),
                content: Some(body.text.clone()),
                tool_name: None,
                tool_calls: None,
                reasoning: None,
                timestamp: event_timestamp(&event),
                token_count: None,
                finish_reason: None,
            };
            let _ = state.deltas.send(ServerFrame::MessageAppended {
                session_id: id.clone(),
                message: dto,
            });
        }
        Err(e) => tracing::warn!(error = %e, "failed to append user message"),
    }

    // Derive a session title from the first user message when the session has
    // none — otherwise API/UI-created sessions show "Untitled" forever. Cheap
    // heuristic: first line, trimmed to ~60 chars (no LLM round-trip needed).
    if next_id == 0 {
        let needs_title = {
            let views = state.views.read().await;
            views
                .sessions
                .get(&id)
                .map(|s| s.title.as_deref().unwrap_or("").trim().is_empty())
                .unwrap_or(true)
        };
        if needs_title {
            let derived = derive_title(&body.text);
            if !derived.is_empty() {
                if let Ok(event) = state.bridge.set_title(&id, &derived) {
                    {
                        let mut views = state.views.write().await;
                        views.apply(&event);
                    }
                    let _ = state.deltas.send(ServerFrame::SessionUpdated {
                        session_id: id.clone(),
                        changes: serde_json::json!({ "title": derived }),
                    });
                }
            }
        }
    }

    // Mark in-flight up front so liveness shows "active" the instant the POST
    // returns (the runtime spawn + turn happen in the background task below).
    state.bridge.mark_in_flight(&id).await;
    // Broadcast liveness so other tabs/windows watching this session flip to
    // the thinking state immediately (and refreshes rehydrate from GET).
    let _ = state.deltas.send(ServerFrame::SessionUpdated {
        session_id: id.clone(),
        changes: serde_json::json!({ "liveness": "running" }),
    });

    // Everything expensive — lazily spawning/resuming the agent runtime, sending
    // the prompt, and draining the event stream — happens OFF the request path
    // so POST returns ~instantly (the ACP handshake can take seconds). The UI
    // shows the user message + "active" immediately and the reply streams over WS.
    let session_id = id.clone();
    let deltas = state.deltas.clone();
    let bridge = state.bridge.clone();
    let views = state.views.clone();
    let envoy_conns = state.envoy_conns.clone();
    // Bind the agent to its session space (working directory). The space was
    // materialized eagerly at create time; derive its path here so the lazily
    // spawned runtime runs scoped to it, not the host cwd.
    let cwd = state
        .bridge
        .space_path(&organization_id, &id)
        .map(|p| p.to_string_lossy().into_owned());

    // --- ADR 0006 §9.3: resolve the effective setup for this session's
    // org/project scope, then materialize via the Hermes adapter. ---
    // This is where the declaration manifest + registry become REAL: MCP
    // servers resolved from registry definitions get injected into the ACP
    // session/new, skills get symlinked into the session space, and env vars
    // (HERMES_SKILLS_PATH) are set on the child.
    let org_slug = std::env::var("OLYMPUS_DEFAULT_ORG").unwrap_or_else(|_| "default".to_string());
    let (mcp_servers, env_vars, adapter_warnings) = {
        let views = state.views.read().await;
        // Get the effective (merged org+project) setup. For now, no project
        // scoping — just org-level. TODO: wire project from session metadata.
        let effective = views.setup.effective_for_project(&org_slug, "");
        let resolved = crate::adapter::ResolvedSetup::from_registry(
            &views.registry,
            &effective.skills,
            &effective.mcp,
            &effective.plugins,
            &effective.hooks,
        );
        // Materialize into the session space if we have one.
        let agent_kind = crate::adapter::AgentKind::from_agent_str(agent.as_deref().unwrap_or(""));
        let adapter = crate::adapter::for_kind(agent_kind);
        if let Some(ref space_path) = cwd {
            match adapter.materialize(
                &resolved,
                std::path::Path::new(space_path),
                crate::adapter::MergeMode::Union,
            ) {
                Ok(overlay) => (overlay.mcp_servers, overlay.env, overlay.warnings),
                Err(e) => {
                    tracing::warn!(error = %e, session = %id, "adapter materialize failed; spawning with empty setup");
                    (vec![], vec![], vec![format!("adapter failed: {e:#}")])
                }
            }
        } else {
            (
                vec![],
                vec![],
                vec!["no session space; skipping adapter".into()],
            )
        }
    };
    if !adapter_warnings.is_empty() {
        for w in &adapter_warnings {
            tracing::info!(session = %id, warning = %w, "adapter warning");
            let _ = deltas.send(ServerFrame::SessionLog {
                session_id: id.clone(),
                level: "warn".into(),
                source: "adapter".into(),
                message: w.clone(),
                timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
            });
        }
    }

    let spec = crate::server::bridge_mgr::RuntimeSpec {
        agent,
        node,
        cwd,
        mcp_servers,
        env: env_vars,
    };
    let resume_hermes = if hermes_id.is_empty() {
        None
    } else {
        Some(hermes_id.clone())
    };
    let prompt_text = body.text.clone();
    let prompt_thinking = body
        .thinking
        .clone()
        .filter(|t| matches!(t.as_str(), "low" | "medium" | "high"));
    let prompt_model = body.model.clone();
    let assistant_seed_id = next_id + 1;
    let log_deltas = deltas.clone();
    let log_session_id = session_id.clone();
    let log_agent = spec.agent.clone().unwrap_or_default();
    let log_resume = resume_hermes.clone();
    tokio::spawn(async move {
        use futures::stream::StreamExt;

        // Emit structured log events for the Logs panel.
        let emit_log = |level: &str, source: &str, msg: &str| {
            let _ = log_deltas.send(ServerFrame::SessionLog {
                session_id: log_session_id.clone(),
                level: level.into(),
                source: source.into(),
                message: msg.into(),
                timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
            });
        };

        if log_resume.is_some() {
            emit_log(
                "info",
                "bridge",
                &format!("Resuming agent runtime ({})…", log_agent),
            );
        } else {
            emit_log(
                "info",
                "bridge",
                &format!("Starting new agent runtime ({})…", log_agent),
            );
        }

        // Lazily ensure a runtime (spawn for a fresh draft, resume by hermes_id
        // after a restart). This is the slow part — now off the request path.
        //
        // ADR 0008 S6 cutover: route to a connected envoy (RemoteRuntime) when
        // the session's node has an active UDS connection. This replaces the
        // in-process bridge for production — the local node is now
        // olympus-envoy@1 over UDS, not an in-process pseudo-envoy. Tests that
        // build AppState with no connected envoys fall back to the in-process
        // bridge (mock factory), so existing tests keep working unchanged.
        let node_id = spec.node.clone().unwrap_or_default();
        // If the session has no explicit node, route to the first connected
        // envoy (default for the single-operator case). Sessions with an
        // explicit node route to that specific envoy.
        let route_node = if node_id.is_empty() {
            envoy_conns.first_node().await.unwrap_or_default()
        } else {
            node_id
        };
        let conn = envoy_conns.get(&route_node).await;
        let (runtime, captured_hermes_id) = if let Some(conn) = conn {
            // Route to the connected envoy via RemoteRuntime.
            let rt = crate::server::envoy_conn::RemoteRuntime::arc_with_spec(
                conn,
                session_id.clone(),
                spec.clone(),
            );
            emit_log(
                "info",
                "bridge",
                &format!("Routing to envoy {}…", route_node),
            );
            match rt.start(resume_hermes.as_deref()).await {
                Ok(()) => {
                    let hid = rt.hermes_session_id().await.unwrap_or_default();
                    emit_log("info", "bridge", "Agent runtime ready (envoy)");
                    (rt, hid)
                }
                Err(e) => {
                    tracing::error!(error = %e, session = %session_id, "envoy ensure_runtime failed");
                    let err_msg = format!("⚠ Failed to start agent: {e:#}");
                    let hid = resume_hermes.clone().unwrap_or_default();
                    if let Ok(event) = bridge.append_system_message(
                        &session_id,
                        &hid,
                        assistant_seed_id,
                        &err_msg,
                        Some("error"),
                    ) {
                        {
                            let mut v = views.write().await;
                            v.apply(&event);
                        }
                        let _ = deltas.send(ServerFrame::MessageAppended {
                            session_id: session_id.clone(),
                            message: crate::server::dto::MessageDto {
                                message_id: assistant_seed_id,
                                session_id: session_id.clone(),
                                role: "system".into(),
                                content: Some(err_msg.clone()),
                                tool_name: None,
                                tool_calls: None,
                                reasoning: None,
                                timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
                                token_count: None,
                                finish_reason: Some("error".into()),
                            },
                        });
                    }
                    let _ = deltas.send(ServerFrame::MessageDone {
                        session_id: session_id.clone(),
                        message_id: assistant_seed_id,
                        finish_reason: Some(format!("error: failed to start agent: {e:#}")),
                    });
                    bridge.clear_in_flight(&session_id).await;
                    return;
                }
            }
        } else {
            // No connected envoy for this node — fall back to the in-process
            // bridge (tests, or a legacy deployment without an envoy service).
            match bridge
                .ensure_runtime(&session_id, &spec, resume_hermes.as_deref())
                .await
            {
                Ok(pair) => {
                    emit_log("info", "bridge", "Agent runtime ready");
                    pair
                }
                Err(e) => {
                    tracing::error!(error = %e, session = %session_id, "ensure_runtime failed");
                    // PERSIST the error as a system message so the user sees it in
                    // the transcript — the old code only broadcast a transient WS
                    // frame, so if the user wasn't watching it vanished silently.
                    let err_msg = format!("⚠ Failed to start agent: {e:#}");
                    let hid = resume_hermes.clone().unwrap_or_default();
                    if let Ok(event) = bridge.append_system_message(
                        &session_id,
                        &hid,
                        assistant_seed_id,
                        &err_msg,
                        Some("error"),
                    ) {
                        {
                            let mut v = views.write().await;
                            v.apply(&event);
                        }
                        let _ = deltas.send(ServerFrame::MessageAppended {
                            session_id: session_id.clone(),
                            message: crate::server::dto::MessageDto {
                                message_id: assistant_seed_id,
                                session_id: session_id.clone(),
                                role: "system".into(),
                                content: Some(err_msg.clone()),
                                tool_name: None,
                                tool_calls: None,
                                reasoning: None,
                                timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
                                token_count: None,
                                finish_reason: Some("error".into()),
                            },
                        });
                    }
                    let _ = deltas.send(ServerFrame::MessageDone {
                        session_id: session_id.clone(),
                        message_id: assistant_seed_id,
                        finish_reason: Some(format!("error: failed to start agent: {e:#}")),
                    });
                    bridge.clear_in_flight(&session_id).await;
                    return;
                }
            }
        };

        // Backfill the captured Hermes id onto the session row (draft → live).
        let hermes_id_clone = if resume_hermes.is_none() && !captured_hermes_id.is_empty() {
            let _ = bridge.backfill_hermes_id(&session_id, &captured_hermes_id);
            let mut v = views.write().await;
            v.apply(&crate::event::Event::SessionUpdated {
                session_id: session_id.clone(),
                title: None,
                model: None,
                archived: None,
                message_count: None,
                agent: None,
                node: None,
                hermes_id: Some(captured_hermes_id.clone()),
                pinned: None,
            });
            captured_hermes_id
        } else {
            resume_hermes.unwrap_or(captured_hermes_id)
        };

        let mut stream = runtime.events();
        let mut assistant_text = String::new();
        let assistant_msg_id = assistant_seed_id;
        // Accumulate structured tool calls seen this turn so they're persisted
        // on the assistant message (and surface in the transcript's tool UI).
        let mut tool_calls_acc: Vec<serde_json::Value> = Vec::new();

        // If a thinking level was requested, prepend it as a /thinking slash
        // command on the first line of the prompt text. Hermes processes
        // slash commands at the start of a multi-line prompt, setting the
        // session's reasoning effort for the current turn. (Sending /thinking
        // as a separate ACP turn doesn't work — it's a CLI command, not an
        // ACP primitive.)
        let final_prompt = if let Some(ref level) = prompt_thinking {
            emit_log("info", "bridge", &format!("Thinking level: {level}"));
            format!("/thinking {level}\n{prompt_text}")
        } else {
            prompt_text
        };

        // Subscribe before sending the prompt so fast runtimes cannot emit and
        // finish the whole turn before the drain loop is listening.
        emit_log("info", "bridge", "Sending prompt to agent…");
        if let Err(e) = runtime
            .send(AgentCommand::Prompt {
                text: final_prompt,
                model: prompt_model,
            })
            .await
        {
            tracing::error!(error = %e, session = %session_id, "prompt send failed");
            emit_log("error", "bridge", &format!("Prompt send failed: {e:#}"));
            let _ = deltas.send(ServerFrame::MessageDone {
                session_id: session_id.clone(),
                message_id: assistant_seed_id,
                finish_reason: Some(format!("error: {e:#}")),
            });
            bridge.clear_in_flight(&session_id).await;
            return;
        }
        let mut terminal_event_seen = false;
        // While a steer-ack is being consumed (its Text + Done), suppress the
        // ack text so it doesn't pollute the assistant reply. The ack is the
        // adapter's "⏩ Steer queued for the active turn: …" string — useful
        // in a CLI but noise in the transcript.
        let mut suppressing_steer_ack = false;

        while let Some(event) = stream.next().await {
            #[allow(unreachable_patterns)]
            match event {
                AgentEvent::Text(chunk) => {
                    if suppressing_steer_ack {
                        // Drop the ack text; the real reply comes after.
                        continue;
                    }
                    // Detect the start of a steer ack and begin suppressing.
                    // The adapter emits this exact prefix in _cmd_steer.
                    if chunk.starts_with("⏩ Steer queued")
                        || chunk.starts_with("⚠️ Steer failed")
                        || chunk.starts_with("No active turn — queued")
                    {
                        suppressing_steer_ack = true;
                        continue;
                    }
                    assistant_text.push_str(&chunk);
                    let _ = deltas.send(ServerFrame::MessageDelta {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        text_delta: chunk,
                    });
                }
                AgentEvent::ToolCall {
                    id,
                    name,
                    args,
                    status,
                    result,
                } => {
                    // Two shapes arrive here:
                    //  - `tool_call` (new invocation): has name+args, status
                    //    "pending" (queued/awaiting permission) or "in_progress".
                    //  - `tool_call_update`: status transition and/or result.
                    // Match updates to their originating call by ACP toolCallId;
                    // fall back to "most recent entry without a result" when the
                    // id is missing (some adapters omit it on updates).
                    let is_update = args.is_empty() && !tool_calls_acc.is_empty() && {
                        // An update either carries a known id or has no args.
                        id.as_deref().is_none_or(|i| {
                            tool_calls_acc
                                .iter()
                                .any(|tc| tc.get("id").and_then(|v| v.as_str()) == Some(i))
                        })
                    };
                    if is_update {
                        let idx = tool_calls_acc
                            .iter()
                            .rposition(|tc| match id.as_deref() {
                                Some(i) => tc.get("id").and_then(|v| v.as_str()) == Some(i),
                                None => tc.get("result").is_none(),
                            })
                            .or_else(|| {
                                tool_calls_acc
                                    .iter()
                                    .rposition(|tc| tc.get("result").is_none())
                            });
                        if let Some(idx) = idx {
                            let tc = &mut tool_calls_acc[idx];
                            if let Some(s) = &status {
                                tc["status"] = serde_json::json!(s);
                            }
                            if let Some(r) = &result {
                                tc["result"] = serde_json::json!(r);
                            }
                            if !name.is_empty() {
                                tc["name"] = serde_json::json!(name);
                            }
                            // Stream the updated card (full state) so the UI
                            // patches it in place — chronological position is
                            // preserved because the UI matches by id.
                            let dto = crate::server::dto::ToolCallDto {
                                id: tc.get("id").and_then(|v| v.as_str()).map(String::from),
                                name: tc
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("tool")
                                    .to_string(),
                                args: tc.get("args").cloned().unwrap_or(serde_json::json!({})),
                                label: None,
                                status: tc.get("status").and_then(|v| v.as_str()).map(String::from),
                                result: tc.get("result").and_then(|v| v.as_str()).map(String::from),
                            };
                            let _ = deltas.send(ServerFrame::MessageToolCall {
                                session_id: session_id.clone(),
                                message_id: assistant_msg_id,
                                tool_call: dto,
                            });
                        }
                    } else {
                        // New tool call — record and stream with its status.
                        // `anchor` = codepoint offset into the assistant text at
                        // the moment the call fired; the UI uses it to interleave
                        // the card chronologically inside the final message.
                        let parsed_args = serde_json::from_str::<serde_json::Value>(&args)
                            .unwrap_or(serde_json::json!({ "raw": args }));
                        let status_str = status.clone().unwrap_or_else(|| "pending".into());
                        let mut entry = serde_json::json!({
                            "name": name,
                            "args": parsed_args,
                            "status": status_str,
                            "anchor": assistant_text.chars().count(),
                        });
                        if let Some(i) = &id {
                            entry["id"] = serde_json::json!(i);
                        }
                        if let Some(r) = &result {
                            entry["result"] = serde_json::json!(r);
                        }
                        tool_calls_acc.push(entry);
                        emit_log("info", "agent", &format!("Tool call: {}", name));
                        let _ = deltas.send(ServerFrame::MessageToolCall {
                            session_id: session_id.clone(),
                            message_id: assistant_msg_id,
                            tool_call: crate::server::dto::ToolCallDto {
                                id: id.clone(),
                                name: name.clone(),
                                args: parsed_args,
                                label: None,
                                status: status.clone(),
                                result: result.clone(),
                            },
                        });
                    }
                }
                AgentEvent::AwaitingInput { .. } => {
                    emit_log("warn", "agent", "Awaiting permission decision…");
                }
                AgentEvent::Reasoning(delta) => {
                    // Stream reasoning/CoT chunks live alongside text + tool calls.
                    let _ = deltas.send(ServerFrame::MessageReasoning {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        text_delta: delta,
                    });
                }
                AgentEvent::Text(_) => {} // streamed to the chat bubble, not logs
                AgentEvent::Done { finish_reason } => {
                    // Skip the Done ack from a /steer slash command. The Hermes
                    // adapter processes /steer as a slash command that returns
                    // immediately with stop_reason="end_turn", which would
                    // terminate the original turn early. Only the genuine
                    // end-of-turn Done should break the drain loop.
                    if bridge.take_steer_pending(&session_id).await {
                        tracing::debug!(session = %session_id, "skipped steer-ack Done");
                        suppressing_steer_ack = false; // resume normal text capture
                                                       // Broadcast delivery status so the steer bubble's
                                                       // badge flips from 'pending' to 'delivered'.
                        let _ = deltas.send(ServerFrame::SessionLog {
                            session_id: session_id.clone(),
                            level: "info".into(),
                            source: "bridge".into(),
                            message: "steer.delivered".into(),
                            timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
                        });
                        continue;
                    }
                    terminal_event_seen = true;
                    emit_log(
                        "info",
                        "agent",
                        &format!(
                            "Turn finished: {}",
                            finish_reason.as_deref().unwrap_or("end_turn")
                        ),
                    );
                    let _ = deltas.send(ServerFrame::MessageDone {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        finish_reason: finish_reason.clone(),
                    });
                    let tool_calls_json = if tool_calls_acc.is_empty() {
                        None
                    } else {
                        Some(serde_json::Value::Array(tool_calls_acc.clone()).to_string())
                    };
                    // Persist the final assistant message AND apply it to the
                    // views so a subsequent GET /messages reflects it. Also
                    // broadcast a message.appended frame so the streaming UI
                    // replaces the in-flight bubble with the final message (the
                    // streamingText is cleared on message.done, but the final
                    // message only enters the list via this append frame).
                    if let Ok(event) = bridge.append_assistant_message(
                        &session_id,
                        &hermes_id_clone,
                        assistant_msg_id,
                        &assistant_text,
                        &tool_calls_json,
                        finish_reason.as_deref(),
                    ) {
                        {
                            let mut v = views.write().await;
                            v.apply(&event);
                        }
                        let dto = crate::server::dto::MessageDto {
                            message_id: assistant_msg_id,
                            session_id: session_id.clone(),
                            role: "assistant".into(),
                            content: Some(assistant_text.clone()),
                            tool_name: None,
                            tool_calls: tool_calls_json
                                .as_deref()
                                .and_then(crate::server::dto::parse_tool_calls),
                            reasoning: None,
                            timestamp: event_timestamp(&event),
                            token_count: None,
                            finish_reason: finish_reason.clone(),
                        };
                        let _ = deltas.send(ServerFrame::MessageAppended {
                            session_id: session_id.clone(),
                            message: dto,
                        });
                    }
                    break;
                }
                AgentEvent::Error(e) => {
                    terminal_event_seen = true;
                    tracing::warn!(error = %e, session = %session_id, "agent error event");
                    emit_log("error", "agent", &e);
                    let content = format!("⚠ agent error: {e:#}");
                    let finish_reason = format!("error: {e:#}");
                    if let Ok(event) = bridge.append_system_message(
                        &session_id,
                        &hermes_id_clone,
                        assistant_msg_id,
                        &content,
                        Some(&finish_reason),
                    ) {
                        {
                            let mut v = views.write().await;
                            v.apply(&event);
                        }
                        let dto = crate::server::dto::MessageDto {
                            message_id: assistant_msg_id,
                            session_id: session_id.clone(),
                            role: "system".into(),
                            content: Some(content),
                            tool_name: None,
                            tool_calls: None,
                            reasoning: None,
                            timestamp: event_timestamp(&event),
                            token_count: None,
                            finish_reason: Some(finish_reason.clone()),
                        };
                        let _ = deltas.send(ServerFrame::MessageAppended {
                            session_id: session_id.clone(),
                            message: dto,
                        });
                    }
                    let _ = deltas.send(ServerFrame::MessageDone {
                        session_id: session_id.clone(),
                        message_id: assistant_msg_id,
                        finish_reason: Some(finish_reason),
                    });
                    break;
                }
                AgentEvent::ToolCall {
                    id,
                    name,
                    args,
                    status,
                    result,
                } => {
                    // Accumulate so the final assistant message carries its tool
                    // calls (rendered in the transcript's tool UI).
                    let mut entry = serde_json::json!({
                        "name": name,
                        "args": serde_json::from_str::<serde_json::Value>(&args)
                            .unwrap_or(serde_json::Value::String(args.clone())),
                        "result": result,
                    });
                    if let Some(i) = &id {
                        entry["id"] = serde_json::json!(i);
                    }
                    if let Some(s) = &status {
                        entry["status"] = serde_json::json!(s);
                    }
                    tool_calls_acc.push(entry);
                }
                AgentEvent::Reasoning(_) => {
                    // Accumulate silently for now; reasoning rendering is separate.
                }
                AgentEvent::AwaitingInput {
                    request_id,
                    tool_call,
                    options,
                } => {
                    // The agent is blocked on a permission decision. Record the
                    // pending request (so /permission can answer it) and flip
                    // liveness to "input-required" via the awaiting set. Do NOT
                    // end the turn — the stream continues once the client
                    // responds and the agent resumes the tool call.
                    bridge.mark_awaiting_input(&session_id, &request_id).await;
                    let options_json = serde_json::Value::Array(
                        options
                            .iter()
                            .map(|o| {
                                serde_json::json!({
                                    "optionId": o.option_id,
                                    "name": o.name,
                                    "kind": o.kind,
                                })
                            })
                            .collect(),
                    );
                    let _ = deltas.send(ServerFrame::PermissionRequired {
                        session_id: session_id.clone(),
                        tool_call,
                        options: options_json,
                    });
                    // Also nudge the session list so the row shows input-required.
                    let _ = deltas.send(ServerFrame::SessionUpdated {
                        session_id: session_id.clone(),
                        changes: serde_json::json!({ "liveness": "input-required" }),
                    });
                }
            }
            // NOTE: assistant_msg_id must NOT increment per event. One prompt
            // produces one assistant message (text + accumulated tool calls),
            // persisted once on Done at assistant_seed_id. The old per-iteration
            // increment inflated the id on any turn with a tool call, colliding
            // with the next turn's user-message id and dropping/clobbering the
            // assistant reply (the multi-turn "no response" bug).
        }
        if !terminal_event_seen {
            tracing::warn!(session = %session_id, "agent stream closed without terminal event");
            let content = "⚠ agent stream closed unexpectedly".to_string();
            let finish_reason = "error: agent stream closed unexpectedly".to_string();
            if let Ok(event) = bridge.append_system_message(
                &session_id,
                &hermes_id_clone,
                assistant_msg_id,
                &content,
                Some(&finish_reason),
            ) {
                {
                    let mut v = views.write().await;
                    v.apply(&event);
                }
                let dto = crate::server::dto::MessageDto {
                    message_id: assistant_msg_id,
                    session_id: session_id.clone(),
                    role: "system".into(),
                    content: Some(content),
                    tool_name: None,
                    tool_calls: None,
                    reasoning: None,
                    timestamp: event_timestamp(&event),
                    token_count: None,
                    finish_reason: Some(finish_reason.clone()),
                };
                let _ = deltas.send(ServerFrame::MessageAppended {
                    session_id: session_id.clone(),
                    message: dto,
                });
            }
            let _ = deltas.send(ServerFrame::MessageDone {
                session_id: session_id.clone(),
                message_id: assistant_msg_id,
                finish_reason: Some(finish_reason),
            });
        }
        // Turn finished (Done, Error, or stream closed): clear the in-flight flag
        // so liveness drops back to idle. Also clear any dangling awaiting-input
        // flag (e.g. the turn was cancelled while a permission was pending).
        bridge.clear_in_flight(&session_id).await;
        bridge.clear_awaiting_input(&session_id).await;
        // Broadcast liveness=idle so the UI drops the thinking indicator
        // immediately and refreshes see idle (not stale 'running').
        let _ = deltas.send(ServerFrame::SessionUpdated {
            session_id: session_id.clone(),
            changes: serde_json::json!({ "liveness": "idle" }),
        });
    });

    (StatusCode::ACCEPTED, Json(json!({ "accepted": true }))).into_response()
}

/// POST /api/sessions/:id/cancel — stop the in-flight turn for a managed
/// session. Sends AgentCommand::Cancel to the runtime (ACP session/cancel) and
/// clears the in-flight flag. No-op (still 200) if there's no active runtime.
pub(crate) async fn cancel_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    if let Some(runtime) = state.bridge.get_runtime(&id).await {
        if let Err(e) = runtime.send(AgentCommand::Cancel).await {
            tracing::warn!(error = %e, session = %id, "cancel send failed");
        }
    }
    state.bridge.clear_in_flight(&id).await;
    // Tell subscribers the turn is no longer running so the UI drops the
    // thinking indicator immediately.
    let _ = state.deltas.send(ServerFrame::SessionUpdated {
        session_id: id.clone(),
        changes: json!({ "liveness": "idle" }),
    });
    (StatusCode::OK, Json(json!({ "cancelled": true }))).into_response()
}

/// POST /api/sessions/:id/steer — steer the in-flight turn without stopping it.
/// Maps to the Hermes /steer command (AgentCommand::Steer). 409 when no turn
/// is running — steering an idle session is a normal message, use POST
/// /messages instead.
pub(crate) async fn steer_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SteerBody>,
) -> Response {
    let text = body.text.trim().to_string();
    if text.is_empty() {
        return (StatusCode::BAD_REQUEST, "steer text is required").into_response();
    }
    if !state.bridge.in_flight_set().await.contains(&id) {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "not_running",
                "message": "No turn in flight — send a normal message instead.",
            })),
        )
            .into_response();
    }
    let Some(runtime) = state.bridge.get_runtime(&id).await else {
        return (StatusCode::CONFLICT, "no runtime for session").into_response();
    };
    // Mark that a steer ack is pending BEFORE sending it — the drain loop in
    // post_message is concurrently consuming the shared ACP event stream and
    // must skip the `Done` this steer produces (the Hermes adapter returns
    // end_turn for the /steer slash-command ack, which would otherwise
    // terminate the original turn early with an empty reply).
    state.bridge.mark_steer_pending(&id).await;
    if let Err(e) = runtime
        .send(AgentCommand::Steer { text: text.clone() })
        .await
    {
        tracing::warn!(error = %e, session = %id, "steer send failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "steer_failed", "message": e.to_string() })),
        )
            .into_response();
    }
    // Persist the steer as a user message with finish_reason="steer" so the
    // transcript shows it as a distinct bubble (an interrupt, not a new turn).
    // Use max(message_id)+1 to avoid colliding with the in-flight assistant
    // message ID (count+1 can collide after window eviction).
    let (hermes_id, steer_msg_id) = {
        let v = state.views.read().await;
        let sid = v
            .sessions
            .get(&id)
            .map(|r| r.hermes_id.clone())
            .unwrap_or_default();
        let max_id = v
            .messages
            .recent(&id, usize::MAX)
            .iter()
            .map(|m| m.message_id)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        (sid, max_id)
    };
    if let Ok(event) = state
        .bridge
        .append_steer_message(&id, &hermes_id, steer_msg_id, &text)
    {
        {
            let mut v = state.views.write().await;
            v.apply(&event);
        }
        let dto = crate::server::dto::MessageDto {
            message_id: steer_msg_id,
            session_id: id.clone(),
            role: "user".into(),
            content: Some(text.clone()),
            tool_name: None,
            tool_calls: None,
            reasoning: None,
            timestamp: event_timestamp(&event),
            token_count: None,
            finish_reason: Some("steer".into()),
        };
        let _ = state.deltas.send(ServerFrame::MessageAppended {
            session_id: id.clone(),
            message: dto,
        });
    }
    let _ = state.deltas.send(ServerFrame::SessionLog {
        session_id: id.clone(),
        level: "info".into(),
        source: "bridge".into(),
        message: format!(
            "Steering turn: {}",
            text.chars().take(80).collect::<String>()
        ),
        timestamp: crate::server::bridge_mgr::chrono_epoch_pub(),
    });
    (StatusCode::ACCEPTED, Json(json!({ "steered": true }))).into_response()
}

/// POST /api/sessions/:id/permission — answer a pending permission request.
/// Forwards the decision to the runtime (which unblocks the agent's gated tool
/// call), clears the awaiting flag, and nudges liveness back to "running".
pub(crate) async fn respond_permission_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PermissionBody>,
) -> Response {
    match state
        .bridge
        .respond_permission(&id, body.option_id.as_deref())
        .await
    {
        Ok(()) => {
            // The agent resumes; it's running again until the next Done.
            let _ = state.deltas.send(ServerFrame::SessionUpdated {
                session_id: id.clone(),
                changes: json!({ "liveness": "running" }),
            });
            (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
        }
        Err(e) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub(crate) async fn attach_repo(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<AttachRepoBody>,
) -> Response {
    if state.views.read().await.sessions.get(&session_id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let event = crate::event::Event::SessionRepoAttached {
        session_id,
        slug: body.slug,
        attached_at: now_epoch(),
    };
    append_and_apply(&state, event).await
}

/// Best-effort: copy jj workspaces from parent to child session space.
pub(crate) fn copy_jj_workspaces(parent_space: &std::path::Path, child_space: &std::path::Path) {
    let parent_repos = match parent_space.join("repos").read_dir() {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for entry in parent_repos.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let src = entry.path();
        if !src.join(".jj").is_dir() {
            continue;
        }
        let dest = child_space.join("repos").join(name);
        let root_output = tokio::task::block_in_place(|| {
            std::process::Command::new("jj")
                .arg("workspace")
                .arg("root")
                .current_dir(&src)
                .output()
        });
        let main_repo = match root_output {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            _ => {
                continue;
            }
        };
        let _ = std::fs::create_dir_all(child_space.join("repos"));
        let add_output = tokio::task::block_in_place(|| {
            std::process::Command::new("jj")
                .arg("workspace")
                .arg("add")
                .arg(&dest)
                .current_dir(&main_repo)
                .output()
        });
        match add_output {
            Ok(out) if out.status.success() => {
                tracing::info!(workspace = %dest.display(), "copied jj workspace into child");
            }
            Ok(out) => {
                tracing::warn!(workspace = %dest.display(), stderr = %String::from_utf8_lossy(&out.stderr), "jj workspace add failed")
            }
            Err(e) => {
                tracing::warn!(workspace = %dest.display(), error = %e, "failed to invoke jj")
            }
        }
    }
}

/// POST /api/sessions/:id/subsessions — spawn a child managed session.
pub(crate) async fn create_subsession(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<CreateSubsessionBody>>,
) -> Response {
    let body = body.map(|Json(b)| b).unwrap_or_default();

    let (parent_agent, parent_space, parent_organization, parent_capabilities) = {
        let views = state.views.read().await;
        let Some(parent) = views.sessions.get(&id) else {
            return (StatusCode::NOT_FOUND, "parent session not found").into_response();
        };
        let agent = body.agent.clone().or_else(|| parent.agent.clone());
        let space = state.bridge.space_path(&parent.org_id, &id);
        (
            agent,
            space,
            parent.org_id.clone(),
            parent.capabilities.clone(),
        )
    };
    let child_capabilities = match (parent_capabilities, body.capabilities.clone()) {
        (Some(parent), Some(mut requested)) => {
            if !parent.can_fork {
                return capability_error("capability_denied", "parent lacks session.fork");
            }
            requested.signature.clear();
            let effective = CapabilitySet::intersect(&parent, &requested);
            if effective != requested {
                return capability_error(
                    "capability_expansion",
                    "requested child capabilities exceed the parent",
                );
            }
            Some(effective)
        }
        (Some(mut parent), None) => {
            if !parent.can_fork {
                return capability_error("capability_denied", "parent lacks session.fork");
            }
            parent.signature.clear();
            Some(parent)
        }
        (None, requested) => requested,
    };

    let spec = crate::server::bridge_mgr::RuntimeSpec {
        agent: parent_agent.clone(),
        node: None,
        cwd: None,
        mcp_servers: vec![],
        env: vec![],
    };

    let ns = match state.bridge.create_draft(&spec, Some(&parent_organization)) {
        Ok(ns) => ns,
        Err(e) => {
            tracing::error!(error = %e, parent = %id, "create_subsession create_draft failed");
            return (StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "bridge_error", "message": format!("Failed to create subsession: {e:#}") })))
                .into_response();
        }
    };

    // Best-effort jj-workspace copy.
    if let (Some(parent_sp), Some(child_sp)) = (
        parent_space.as_ref(),
        state
            .bridge
            .space_path(&parent_organization, &ns.session_id),
    ) {
        if parent_sp.exists() {
            copy_jj_workspaces(parent_sp, &child_sp);
        }
    }

    let created = crate::event::Event::SessionCreated {
        session_id: ns.session_id.clone(),
        hermes_id: ns.hermes_id.clone(),
        source: "olympus".into(),
        model: None,
        title: body.title.clone(),
        started_at: ns.started_at,
        message_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        agent: parent_agent.clone(),
        node: None,
    };
    {
        let mut views = state.views.write().await;
        views.apply(&created);
        views.apply(&crate::event::Event::SessionOrganizationAssigned {
            session_id: ns.session_id.clone(),
            organization_id: parent_organization.clone(),
        });
    }

    let forked_event = crate::event::Event::SessionForked {
        parent_session_id: id.clone(),
        child_session_id: ns.session_id.clone(),
        fork_type: "sub".into(),
        fork_point: None,
        forked_at: now_epoch(),
    };
    let capability_event = match child_capabilities {
        Some(capabilities) => match signed_capability_event(
            &state,
            ns.session_id.clone(),
            capabilities,
            &Principal::Operator,
            Some(id.clone()),
        ) {
            Ok(event) => Some(event),
            Err(response) => return response,
        },
        None => None,
    };
    if let Err(e) = state.log.append(&forked_event) {
        tracing::warn!(error = %e, "failed to append SessionForked for subsession");
    }
    {
        let mut views = state.views.write().await;
        views.apply(&forked_event);
        if let Some(event) = capability_event.as_ref() {
            if let Err(error) = state.log.append(event) {
                tracing::error!(%error, "persisting subsession capabilities");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            views.apply(event);
        }
    }

    // Optionally enqueue the first user message.
    if let Some(prompt) = &body.prompt {
        if !prompt.trim().is_empty() {
            let next_id = 0u64;
            match state
                .bridge
                .append_user_message(&ns.session_id, &ns.hermes_id, next_id, prompt)
            {
                Ok(event) => {
                    {
                        let mut views = state.views.write().await;
                        views.apply(&event);
                    }
                    let dto = crate::server::dto::MessageDto {
                        message_id: next_id,
                        session_id: ns.session_id.clone(),
                        role: "user".into(),
                        content: Some(prompt.clone()),
                        tool_name: None,
                        tool_calls: None,
                        reasoning: None,
                        timestamp: event_timestamp(&event),
                        token_count: None,
                        finish_reason: None,
                    };
                    let _ = state.deltas.send(ServerFrame::MessageAppended {
                        session_id: ns.session_id.clone(),
                        message: dto,
                    });
                }
                Err(e) => {
                    tracing::warn!(error = %e, child = %ns.session_id, "failed to enqueue subsession prompt")
                }
            }
        }
    }

    let dto = {
        let views = state.views.read().await;
        match views.sessions.get(&ns.session_id) {
            Some(row) => {
                let mut d = SessionDto::from_row(row);
                d.parent_session_id = Some(id.clone());
                d.fork_type = Some("sub".into());
                d
            }
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "subsession view lookup failed",
                )
                    .into_response()
            }
        }
    };

    let _ = state.deltas.send(ServerFrame::SessionAdded {
        session: dto.clone(),
    });
    (
        StatusCode::CREATED,
        Json(serde_json::to_value(&dto).unwrap()),
    )
        .into_response()
}

/// GET /api/sessions/:id/subsessions — list direct children.
pub(crate) async fn list_subsessions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let children: Vec<SessionDto> = {
        let views = state.views.read().await;
        let Some(parent) = views.sessions.get(&id) else {
            return (StatusCode::NOT_FOUND, "parent session not found").into_response();
        };
        views
            .sessions
            .list(&Filters::default())
            .into_iter()
            .filter(|row| {
                row.parent_session_id.as_deref() == Some(id.as_str()) && row.org_id == parent.org_id
            })
            .map(SessionDto::from_row)
            .collect()
    };
    Json(json!({ "subsessions": children })).into_response()
}

/// POST /api/sessions/:id/complete — check gate. Only subsessions can complete.
pub(crate) async fn complete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CompleteBody>,
) -> Response {
    let verdict = body.verdict.as_str();
    if verdict != "pass" && verdict != "fail" {
        return (StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_verdict", "message": "verdict must be \"pass\" or \"fail\"" })))
            .into_response();
    }

    let (parent_id, child_hermes_id, parent_hermes_id) = {
        let views = state.views.read().await;
        let Some(child) = views.sessions.get(&id) else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let Some(ref parent_id) = child.parent_session_id else {
            return (StatusCode::CONFLICT,
                Json(json!({ "error": "not_a_subsession", "message": "Only subsessions can be completed." })))
                .into_response();
        };
        let Some(parent) = views.sessions.get(parent_id) else {
            return (StatusCode::NOT_FOUND, "parent session not found").into_response();
        };
        if parent.org_id != child.org_id {
            return (StatusCode::NOT_FOUND, "parent session not found").into_response();
        }
        (
            parent_id.clone(),
            child.hermes_id.clone(),
            parent.hermes_id.clone(),
        )
    };

    let summary_text = body.summary.as_deref().unwrap_or("");
    let notice = format!("[subsession {id} {verdict}] {summary_text}");

    let next_id = {
        let views = state.views.read().await;
        views
            .messages
            .recent(&parent_id, usize::MAX)
            .iter()
            .map(|m| m.message_id)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0)
    };

    match state
        .bridge
        .append_system_message(&parent_id, &parent_hermes_id, next_id, &notice, None)
    {
        Ok(event) => {
            {
                let mut views = state.views.write().await;
                views.apply(&event);
            }
            let dto = crate::server::dto::MessageDto {
                message_id: next_id,
                session_id: parent_id.clone(),
                role: "system".into(),
                content: Some(notice.clone()),
                tool_name: None,
                tool_calls: None,
                reasoning: None,
                timestamp: event_timestamp(&event),
                token_count: None,
                finish_reason: None,
            };
            let _ = state.deltas.send(ServerFrame::MessageAppended {
                session_id: parent_id.clone(),
                message: dto,
            });
        }
        Err(e) => tracing::error!(error = %e, "failed to append complete-gate system message"),
    }

    // Archive the child.
    let archive_event = crate::event::Event::SessionUpdated {
        session_id: id.clone(),
        title: None,
        model: None,
        archived: Some(true),
        message_count: None,
        agent: None,
        node: None,
        hermes_id: Some(child_hermes_id),
        pinned: None,
    };
    if let Err(e) = state.log.append(&archive_event) {
        tracing::warn!(error = %e, "failed to append archive event for completed subsession");
    }
    {
        let mut views = state.views.write().await;
        views.apply(&archive_event);
    }
    let _ = state.deltas.send(ServerFrame::SessionUpdated {
        session_id: id.clone(),
        changes: json!({ "archived": true }),
    });

    Json(json!({ "sessionId": id, "parentId": parent_id, "verdict": verdict, "archived": true }))
        .into_response()
}

/// POST /api/sessions/:id/handover — switch this session to a different agent
/// harness (ADR 0006 §9.1). This is the SOLE mechanism for switching agent kind.
///
/// Creates a new session with the target agent, copies the conversation history
/// (translated to prose context for the target harness), inherits the card_id,
/// archives the source, and emits SessionHandover + SessionForked events.
pub(crate) async fn handover_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<HandoverBody>,
) -> Response {
    let (source, messages) = {
        let views = state.views.read().await;
        let Some(source) = views.sessions.get(&id).cloned() else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let messages = views
            .messages
            .recent(&id, usize::MAX)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        (source, messages)
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let to_kind = crate::adapter::AgentKind::from_agent_str(&body.to_agent_kind);
    let from_kind =
        crate::adapter::AgentKind::from_agent_str(source.agent.as_deref().unwrap_or(""));
    let to_agent_name = match to_kind {
        crate::adapter::AgentKind::Hermes => "hermes".to_string(),
        crate::adapter::AgentKind::ClaudeCode => "claude-code".to_string(),
        crate::adapter::AgentKind::Codex => "codex".to_string(),
    };

    // Create the target session.
    let target_id = format!("oly-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    if let Err(error) = state.bridge.ensure_space(&source.org_id, &target_id) {
        tracing::error!(%error, "creating organization-scoped handover workspace");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create handover workspace",
        )
            .into_response();
    }

    // Persist the complete handover as one batch. In particular, the target
    // must never become durable without inheriting the source organization.
    let created = crate::event::Event::SessionCreated {
        session_id: target_id.clone(),
        hermes_id: String::new(),
        source: "olympus".into(),
        model: body.model.clone().or(source.model.clone()),
        title: source.title.clone(),
        started_at: now,
        message_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        agent: Some(to_agent_name.clone()),
        node: source.node.clone(),
    };
    let organization = crate::event::Event::SessionOrganizationAssigned {
        session_id: target_id.clone(),
        organization_id: source.org_id.clone(),
    };
    let mut events = vec![created, organization];
    if let Some(mut capabilities) = source.capabilities.clone() {
        capabilities.signature.clear();
        let capability_event = match signed_capability_event(
            &state,
            target_id.clone(),
            capabilities,
            &Principal::Operator,
            Some(id.clone()),
        ) {
            Ok(event) => event,
            Err(response) => return response,
        };
        events.push(capability_event);
    }
    events.extend(messages.iter().enumerate().map(|(idx, msg)| {
        crate::event::Event::MessageAppended {
            session_id: target_id.clone(),
            hermes_session_id: String::new(),
            message_id: idx as u64,
            role: msg.role.clone(),
            content: msg.content.clone(),
            tool_name: msg.tool_name.clone(),
            tool_calls: None,
            reasoning: None,
            timestamp: msg.timestamp,
            token_count: msg.token_count,
            finish_reason: None,
        }
    }));

    let handover_event = crate::event::Event::SessionHandover {
        source_session_id: id.clone(),
        target_session_id: target_id.clone(),
        from_agent_kind: format!("{:?}", from_kind),
        to_agent_kind: format!("{:?}", to_kind),
        translated_message_count: messages.len() as u64,
        handed_over_at: now,
    };
    let archive = crate::event::Event::SessionUpdated {
        session_id: id.clone(),
        title: None,
        model: None,
        archived: Some(true),
        message_count: None,
        agent: None,
        node: None,
        hermes_id: None,
        pinned: None,
    };
    events.push(handover_event);
    events.push(archive);
    if let Err(e) = state.log.append_batch(&events) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "log_error", "message": e.to_string() })),
        )
            .into_response();
    }
    {
        let mut views = state.views.write().await;
        for event in &events {
            views.apply(event);
        }
    }

    // Build the DTO for the target session.
    let dto = {
        let views = state.views.read().await;
        match views.sessions.get(&target_id) {
            Some(row) => SessionDto::from_row(row),
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "target session not found after creation",
                )
                    .into_response();
            }
        }
    };

    let _ = state.deltas.send(ServerFrame::SessionAdded {
        session: dto.clone(),
    });

    Json(json!({ "session": dto, "handover": {
        "fromAgentKind": format!("{:?}", from_kind),
        "toAgentKind": format!("{:?}", to_kind),
        "translatedMessages": messages.len(),
    } }))
    .into_response()
}

pub(crate) async fn attach_session_project(
    State(state): State<AppState>,
    scope: Option<axum::extract::Extension<OrgScope>>,
    Path(session_id): Path<String>,
    Json(body): Json<AttachProjectBody>,
) -> Response {
    // Validate session exists.
    let session_space = {
        let views = state.views.read().await;
        let Some(session) = views.sessions.get(&session_id) else {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "message": "session not found" })),
            )
                .into_response();
        };
        if scope
            .as_ref()
            .is_some_and(|scope| session.org_id != scope.0.organization_id)
        {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "message": "session not found" })),
            )
                .into_response();
        }
        // Also validate project exists.
        if views.projects.get(&body.project_id).is_none_or(|project| {
            scope
                .as_ref()
                .is_some_and(|scope| project.org_id != scope.0.organization_id)
        }) {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "not_found", "message": "project not found" })),
            )
                .into_response();
        }
        state.bridge.space_for(&session.org_id, &session_id)
    };
    // Create symlink (best-effort).
    let _ = state
        .projects
        .attach_symlink(&body.project_id, session_space.as_deref());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let event = crate::event::Event::SessionProjectAttached {
        session_id: session_id.clone(),
        project_id: body.project_id.clone(),
        attached_at: now,
    };
    append_and_apply(&state, event).await;
    let views = state.views.read().await;
    match views.sessions.get(&session_id) {
        Some(row) => Json(json!({ "sessionId": row.session_id, "projectId": row.project_id }))
            .into_response(),
        None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
