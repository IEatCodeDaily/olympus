//! Knowledge-vault routes (`/api/vaults/**`, ADR 0004).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::server::dto::{
    self, NoteDocumentDto, NoteIndexEntryDto, NoteTreeEntryDto, VaultSummaryDto,
};
use crate::server::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/vaults", get(list_vaults).post(create_vault))
        .route("/api/vaults/{id}/notes", get(list_vault_notes))
        .route("/api/vaults/{id}/documents", get(list_vault_documents))
        .route(
            "/api/vaults/{id}/note",
            get(get_vault_note)
                .put(put_vault_note)
                .delete(delete_vault_note),
        )
        .route("/api/vaults/{id}/graph", get(get_vault_graph))
        .route("/api/vaults/{id}/collections", get(list_vault_collections))
        .route(
            "/api/vaults/{id}/collections/{path}",
            get(get_collection_rows),
        )
}

#[derive(Debug, Deserialize)]
pub(crate) struct VaultNoteQuery {
    path: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateVaultBody {
    name: String,
    backend: crate::vault::VaultBackend,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PutVaultNoteBody {
    #[serde(default)]
    markdown: Option<String>,
    /// Optional rename target. `newPath` is the explicit API; `path` is accepted
    /// as the natural "PUT this note at a new path" shape for early UI callers.
    #[serde(default)]
    new_path: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    create_only: bool,
}

pub(crate) async fn list_vaults(State(state): State<AppState>) -> Response {
    match state.vaults.list_vaults() {
        Ok(vaults) => {
            let vaults: Vec<VaultSummaryDto> = vaults.into_iter().map(Into::into).collect();
            Json(json!({ "vaults": vaults })).into_response()
        }
        Err(err) => vault_error(err),
    }
}

pub(crate) async fn create_vault(
    State(state): State<AppState>,
    Json(body): Json<CreateVaultBody>,
) -> Response {
    match state.vaults.create_vault(&body.name, body.backend) {
        Ok(vault) => (
            StatusCode::CREATED,
            Json(serde_json::to_value(VaultSummaryDto::from(vault)).unwrap()),
        )
            .into_response(),
        Err(err) => vault_error(err),
    }
}

pub(crate) async fn list_vault_notes(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.vaults.list_notes(&id) {
        Ok(notes) => {
            let notes: Vec<NoteTreeEntryDto> = notes.into_iter().map(Into::into).collect();
            Json(json!({ "notes": notes })).into_response()
        }
        Err(err) => vault_error(err),
    }
}

pub(crate) async fn list_vault_documents(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.vaults.list_documents(&id) {
        Ok(documents) => {
            let documents: Vec<NoteIndexEntryDto> = documents.into_iter().map(Into::into).collect();
            Json(json!({ "documents": documents })).into_response()
        }
        Err(err) => vault_error(err),
    }
}

pub(crate) async fn get_vault_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<VaultNoteQuery>,
) -> Response {
    match state.vaults.read_note(&id, &q.path) {
        Ok(note) => {
            Json(serde_json::to_value(NoteDocumentDto::from(note)).unwrap()).into_response()
        }
        Err(err) => vault_error(err),
    }
}

pub(crate) async fn put_vault_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<VaultNoteQuery>,
    Json(body): Json<PutVaultNoteBody>,
) -> Response {
    let new_path = body.new_path.or(body.path);
    match state.vaults.write_note(
        &id,
        &q.path,
        crate::vault::WriteNote {
            markdown: body.markdown,
            new_path,
            create_only: body.create_only,
        },
    ) {
        Ok(note) => {
            Json(serde_json::to_value(NoteDocumentDto::from(note)).unwrap()).into_response()
        }
        Err(err) => vault_error(err),
    }
}

pub(crate) async fn delete_vault_note(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<VaultNoteQuery>,
) -> Response {
    match state.vaults.delete_note(&id, &q.path) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => vault_error(err),
    }
}

pub(crate) fn vault_error(err: anyhow::Error) -> Response {
    let status = if crate::vault::not_found(&err) {
        StatusCode::NOT_FOUND
    } else if crate::vault::conflict(&err) {
        StatusCode::CONFLICT
    } else if crate::vault::bad_request(&err) {
        StatusCode::BAD_REQUEST
    } else {
        tracing::error!(error = %err, "vault operation failed");
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (
        status,
        Json(json!({ "error": "vault_error", "message": err.to_string() })),
    )
        .into_response()
}

pub(crate) async fn get_vault_graph(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.vaults.graph(&id) {
        Ok(g) => {
            let dto = dto::VaultGraphDto {
                nodes: g
                    .nodes
                    .into_iter()
                    .map(|n| dto::GraphNodeDto {
                        id: n.id,
                        title: n.title,
                        path: n.path,
                        cid: n.cid,
                        link_count: n.link_count,
                    })
                    .collect(),
                edges: g
                    .edges
                    .into_iter()
                    .map(|e| dto::GraphEdgeDto {
                        source: e.source,
                        target: e.target,
                    })
                    .collect(),
            };
            Json(json!({ "nodes": dto.nodes, "edges": dto.edges })).into_response()
        }
        Err(err) => vault_error(err),
    }
}

pub(crate) async fn list_vault_collections(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.vaults.list_collections(&id) {
        Ok(collections) => {
            let dtos: Vec<dto::CollectionSummaryDto> = collections
                .into_iter()
                .map(|c| dto::CollectionSummaryDto {
                    name: c.name,
                    path: c.path,
                    row_count: c.row_count,
                })
                .collect();
            Json(json!({ "collections": dtos })).into_response()
        }
        Err(err) => vault_error(err),
    }
}

pub(crate) async fn get_collection_rows(
    State(state): State<AppState>,
    Path((id, path)): Path<(String, String)>,
) -> Response {
    match state.vaults.collection_rows(&id, &path) {
        Ok(data) => Json(json!({
            "columns": data.columns,
            "rows": data.rows,
        }))
        .into_response(),
        Err(err) => vault_error(err),
    }
}
