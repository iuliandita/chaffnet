use crate::api_types::{BatchRequest, BatchResponse, CheckRequest, CheckResponse};
use crate::app::AppState;
use axum::http::StatusCode;
use axum::{extract::State, Json};
use std::sync::Arc;

type HandlerError = (StatusCode, String);

/// Maximum number of items accepted in a single batch request.
const MAX_BATCH_ITEMS: usize = 1000;

fn assess_one(state: &AppState, req: CheckRequest) -> Result<CheckResponse, HandlerError> {
    let content = req.into_content();
    let a = state.assessor.assess(&content).map_err(|error| {
        tracing::error!(%error, "content classification failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "classification failed".to_string(),
        )
    })?;
    Ok(CheckResponse {
        spam: a.spam,
        slop: a.slop,
        reasons: a.reasons.iter().map(|r| r.as_str().to_string()).collect(),
    })
}

#[utoipa::path(
    post,
    path = "/v1/check",
    request_body = CheckRequest,
    responses(
        (status = 200, description = "Assessment", body = CheckResponse),
        (status = 401, description = "Missing or invalid hosted API key"),
        (status = 429, description = "Hosted tenant request limit exceeded"),
        (status = 500, description = "Classification failed")
    ),
    security((), ("bearer_auth" = []))
)]
pub async fn check(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CheckRequest>,
) -> Result<Json<CheckResponse>, HandlerError> {
    Ok(Json(assess_one(&state, req)?))
}

#[utoipa::path(
    post,
    path = "/v1/check/batch",
    request_body = BatchRequest,
    responses(
        (status = 200, description = "Batch assessment", body = BatchResponse),
        (status = 401, description = "Missing or invalid hosted API key"),
        (status = 429, description = "Hosted tenant request limit exceeded"),
        (status = 413, description = "Batch exceeds the item limit")
    ),
    security((), ("bearer_auth" = []))
)]
pub async fn check_batch(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchRequest>,
) -> Result<Json<BatchResponse>, (StatusCode, String)> {
    if req.items.len() > MAX_BATCH_ITEMS {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("batch exceeds {MAX_BATCH_ITEMS} items"),
        ));
    }
    let results = req
        .items
        .into_iter()
        .map(|request| assess_one(&state, request))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(BatchResponse { results }))
}
