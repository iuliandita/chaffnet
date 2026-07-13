use crate::api_types::{FeedbackRequest, FeedbackResponse};
use crate::app::AppState;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::Json;
use chaffnet_core::features::Features;
use chaffnet_core::reputation_hosted::TenantId;
use std::sync::Arc;

const MAX_FEEDBACK_TEXT_BYTES: usize = 64 * 1024;

#[utoipa::path(
    post,
    path = "/v1/feedback",
    request_body = FeedbackRequest,
    responses(
        (status = 200, description = "Feedback persisted", body = FeedbackResponse),
        (status = 401, description = "Missing or invalid API key"),
        (status = 413, description = "Feedback text exceeds the size limit"),
        (status = 429, description = "Tenant request limit exceeded"),
        (status = 500, description = "Feedback persistence failed")
    ),
    security(("bearer_auth" = []))
)]
pub async fn feedback(
    State(state): State<Arc<AppState>>,
    Extension(tenant): Extension<TenantId>,
    Json(request): Json<FeedbackRequest>,
) -> Result<Json<FeedbackResponse>, (StatusCode, String)> {
    if request.content.text.len() > MAX_FEEDBACK_TEXT_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("feedback text exceeds {MAX_FEEDBACK_TEXT_BYTES} bytes"),
        ));
    }
    let Some(store) = state.hosted_store() else {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    };
    let content = request.content.into_content();
    let fingerprint = Features::extract(&content).content_fingerprint;
    store
        .record_feedback(tenant, fingerprint, request.verdict.into())
        .map_err(|error| {
            tracing::error!(%error, "hosted feedback persistence failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "feedback persistence failed".to_string(),
            )
        })?;
    Ok(Json(FeedbackResponse { accepted: true }))
}
