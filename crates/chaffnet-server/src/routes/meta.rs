use crate::openapi::ApiDoc;
use axum::http::header;
use axum::response::IntoResponse;
use axum::Json;
use utoipa::OpenApi;

pub async fn healthz() -> impl IntoResponse {
    (axum::http::StatusCode::OK, "ok")
}

pub async fn openapi_json() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}

const LLMS_TXT: &str = "\
# chaffnet

Spam and AI-slop classification for user-generated content.

POST /v1/check
  body: {\"text\": string, \"context\": \"comment\"|\"review\"|\"forum\"|\"other\", \
\"author_ip\"?: string, \"author_email\"?: string, \"author_name\"?: string}
  returns: {\"spam\": 0..1, \"slop\": 0..1, \"reasons\": string[]}

POST /v1/check/batch
  body: {\"items\": CheckRequest[]}
  returns: {\"results\": CheckResponse[]}

POST /v1/feedback (hosted mode, bearer authentication required)
  body: {\"content\": CheckRequest, \"verdict\": \"spam\"|\"ham\"}
  returns: {\"accepted\": true}

Full schema: GET /openapi.json
spam and slop are scores normalized to 0..1, not verdicts. Choose your own threshold.
";

pub async fn llms_txt() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        LLMS_TXT,
    )
}
