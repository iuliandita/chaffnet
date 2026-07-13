use axum::body::Body;
use axum::http::{Request, StatusCode};
use chaffnet_core::classifier::ClassifierError;
use chaffnet_core::content::Content;
use chaffnet_core::engine::EngineError;
use chaffnet_core::Assessment;
use chaffnet_server::app::{build_app, AppState, ContentAssessor};
use chaffnet_server::auth::ApiCredential;
use http_body_util::BodyExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

// Each test runs concurrently in one process; redb permits only one open handle
// per file, so give every `test_app()` its own db path (pid + per-call counter).
static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

struct FailingAssessor;

impl ContentAssessor for FailingAssessor {
    fn assess(&self, _content: &Content) -> Result<Assessment, EngineError> {
        Err(EngineError::Classifier(ClassifierError::Inference(
            "forced".into(),
        )))
    }
}

fn test_app() -> axum::Router {
    let n = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("chaffnet-http-{}-{}.redb", std::process::id(), n));
    let _ = std::fs::remove_file(&path);
    let state = AppState::new_local(&path).unwrap();
    build_app(Arc::new(state))
}

const API_KEY_A: &str = "cn_test_a_abcdefghijklmnopqrstuvwxyz0123456789";
const API_KEY_B: &str = "cn_test_b_abcdefghijklmnopqrstuvwxyz0123456789";
const API_KEY_C: &str = "cn_test_c_abcdefghijklmnopqrstuvwxyz0123456789";
const NETWORK_SECRET: &[u8] = b"test-network-secret-with-at-least-32-bytes";

fn hosted_test_app(rate_limit_per_minute: u32) -> axum::Router {
    let n = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir();
    let local = root.join(format!(
        "chaffnet-hosted-http-{}-{n}-local.redb",
        std::process::id()
    ));
    let network = root.join(format!(
        "chaffnet-hosted-http-{}-{n}-network.redb",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&local);
    let _ = std::fs::remove_file(&network);
    let state = AppState::new_hosted(
        &local,
        &network,
        NETWORK_SECRET,
        &[
            ApiCredential {
                tenant: "tenant-a",
                api_key: API_KEY_A,
            },
            ApiCredential {
                tenant: "tenant-b",
                api_key: API_KEY_B,
            },
            ApiCredential {
                tenant: "tenant-c",
                api_key: API_KEY_C,
            },
        ],
        rate_limit_per_minute,
    )
    .unwrap();
    build_app(Arc::new(state))
}

fn authenticated_post(uri: &str, api_key: &str, body: impl Into<Body>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {api_key}"))
        .body(body.into())
        .unwrap()
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn healthz_returns_ok() {
    let resp = test_app()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn check_scores_obvious_spam_high() {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/check")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"text":"BUY CHEAP WATCHES https://a.io https://b.io https://c.io FREE FREE FREE","context":"comment"}"#,
        ))
        .unwrap();
    let resp = test_app().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert!(v["spam"].as_f64().unwrap() > 0.7);
    assert!(!v["reasons"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn batch_returns_one_result_per_item() {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/check/batch")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"items":[{"text":"hello friend","context":"comment"},{"text":"SPAM https://x.io https://y.io FREE","context":"comment"}]}"#,
        ))
        .unwrap();
    let resp = test_app().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["results"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn llms_txt_is_served_as_plain_text() {
    let resp = test_app()
        .oneshot(
            Request::builder()
                .uri("/llms.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("text/plain"));
}

#[tokio::test]
async fn openapi_json_is_served() {
    let resp = test_app()
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert!(v["paths"]["/v1/check"].is_object());
    assert!(v["paths"]["/v1/feedback"].is_object());
    assert_eq!(
        v["components"]["securitySchemes"]["bearer_auth"]["scheme"],
        "bearer"
    );
}

// Mirrors MAX_BATCH_ITEMS in src/routes/check.rs (that const is private).
const MAX_BATCH_ITEMS: usize = 1000;

fn batch_body(item_count: usize) -> String {
    let items = vec![r#"{"text":"a","context":"comment"}"#; item_count].join(",");
    format!(r#"{{"items":[{items}]}}"#)
}

#[tokio::test]
async fn malformed_json_is_client_error() {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/check")
        .header("content-type", "application/json")
        .body(Body::from("{not json"))
        .unwrap();
    let resp = test_app().oneshot(req).await.unwrap();
    assert!(
        resp.status().is_client_error(),
        "status was {}",
        resp.status()
    );
}

#[tokio::test]
async fn batch_over_cap_is_payload_too_large() {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/check/batch")
        .header("content-type", "application/json")
        .body(Body::from(batch_body(MAX_BATCH_ITEMS + 1)))
        .unwrap();
    let resp = test_app().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn batch_at_cap_is_ok() {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/check/batch")
        .header("content-type", "application/json")
        .body(Body::from(batch_body(MAX_BATCH_ITEMS)))
        .unwrap();
    let resp = test_app().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["results"].as_array().unwrap().len(), MAX_BATCH_ITEMS);
}

#[tokio::test]
async fn classifier_failure_returns_generic_server_error() {
    let app = build_app(Arc::new(AppState::from_assessor(Arc::new(FailingAssessor))));
    let request = Request::builder()
        .method("POST")
        .uri("/v1/check")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"text":"hello","context":"comment"}"#))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..], b"classification failed");
}

#[tokio::test]
async fn self_hosted_mode_does_not_expose_feedback() {
    let response = test_app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/feedback")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn hosted_routes_require_a_valid_bearer_key() {
    let app = hosted_test_app(100);
    let missing = Request::builder()
        .method("POST")
        .uri("/v1/check")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"text":"hello"}"#))
        .unwrap();
    let response = app.clone().oneshot(missing).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers().get("www-authenticate").unwrap(),
        "Bearer"
    );

    let invalid = authenticated_post(
        "/v1/check",
        "cn_test_invalid_abcdefghijklmnopqrstuvwxyz012345",
        r#"{"text":"hello"}"#,
    );
    assert_eq!(
        app.clone().oneshot(invalid).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );

    let valid = authenticated_post("/v1/check", API_KEY_A, r#"{"text":"hello"}"#);
    assert_eq!(app.oneshot(valid).await.unwrap().status(), StatusCode::OK);
}

#[tokio::test]
async fn hosted_rate_limit_is_per_tenant() {
    let app = hosted_test_app(1);
    let first = authenticated_post("/v1/check", API_KEY_A, r#"{"text":"hello"}"#);
    assert_eq!(
        app.clone().oneshot(first).await.unwrap().status(),
        StatusCode::OK
    );

    let second = authenticated_post("/v1/check", API_KEY_A, r#"{"text":"hello"}"#);
    let limited = app.clone().oneshot(second).await.unwrap();
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers().contains_key("retry-after"));

    let other_tenant = authenticated_post("/v1/check", API_KEY_B, r#"{"text":"hello"}"#);
    assert_eq!(
        app.oneshot(other_tenant).await.unwrap().status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn three_tenant_feedback_affects_hosted_checks() {
    let app = hosted_test_app(100);
    let text = "shared campaign phrase buy the same cheap product now";
    let feedback_body =
        format!(r#"{{"content":{{"text":"{text}","context":"comment"}},"verdict":"spam"}}"#);

    for key in [API_KEY_A, API_KEY_B, API_KEY_C] {
        let response = app
            .clone()
            .oneshot(authenticated_post(
                "/v1/feedback",
                key,
                feedback_body.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body, serde_json::json!({"accepted": true}));
    }

    let check_body = serde_json::json!({"text": text, "context": "comment"}).to_string();
    let response = app
        .oneshot(authenticated_post("/v1/check", API_KEY_A, check_body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert!(body["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reason| reason == "content_fingerprint_seen_spam"));
}

#[tokio::test]
async fn hosted_feedback_rejects_oversized_text() {
    let app = hosted_test_app(100);
    let body = serde_json::json!({
        "content": {"text": "x".repeat(64 * 1024 + 1), "context": "comment"},
        "verdict": "spam"
    })
    .to_string();
    let response = app
        .oneshot(authenticated_post("/v1/feedback", API_KEY_A, body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
