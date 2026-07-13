use crate::auth::{ApiCredential, ApiKeyAuth, ApiKeyConfigError, AuthRejection};
use crate::routes;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chaffnet_core::classifier::ClassifierError;
use chaffnet_core::classifier_onnx::OnnxClassifier;
use chaffnet_core::config::EngineConfig;
use chaffnet_core::engine::EngineError;
use chaffnet_core::reputation_hosted::{HostedStore, HostedStoreError, TenantId};
use chaffnet_core::reputation_local::{LocalStore, StoreError};
use chaffnet_core::Engine;
use chaffnet_core::{Assessment, Content};
use std::path::Path;
use std::sync::Arc;

pub type SharedEngine = Engine<LocalStore, OnnxClassifier>;

#[derive(Debug, thiserror::Error)]
pub enum AppInitError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Classifier(#[from] ClassifierError),
    #[error(transparent)]
    HostedStore(#[from] HostedStoreError),
    #[error(transparent)]
    ApiKeys(#[from] ApiKeyConfigError),
}

pub trait ContentAssessor: Send + Sync {
    fn assess(&self, content: &Content) -> Result<Assessment, EngineError>;
}

impl<S, C> ContentAssessor for Engine<S, C>
where
    S: chaffnet_core::reputation::ReputationStore,
    C: chaffnet_core::classifier::Classifier,
{
    fn assess(&self, content: &Content) -> Result<Assessment, EngineError> {
        Engine::assess(self, content)
    }
}

pub struct AppState {
    pub assessor: Arc<dyn ContentAssessor>,
    hosted_store: Option<Arc<HostedStore>>,
    api_key_auth: Option<ApiKeyAuth>,
}

impl AppState {
    pub fn new_local(db_path: &Path) -> Result<Self, AppInitError> {
        let store = LocalStore::open_seeded(db_path)?;
        let engine = Engine::new(store, OnnxClassifier::bundled()?, EngineConfig::default());
        Ok(Self::from_assessor(Arc::new(engine)))
    }

    pub fn new_hosted(
        local_db_path: &Path,
        network_db_path: &Path,
        network_secret: &[u8],
        api_keys: &[ApiCredential<'_>],
        requests_per_minute: u32,
    ) -> Result<Self, AppInitError> {
        let api_key_auth = ApiKeyAuth::new(api_keys, requests_per_minute)?;
        let store = Arc::new(HostedStore::open(
            local_db_path,
            network_db_path,
            network_secret,
        )?);
        let engine = Engine::new(
            Arc::clone(&store),
            OnnxClassifier::bundled()?,
            EngineConfig::default(),
        );
        Ok(Self {
            assessor: Arc::new(engine),
            hosted_store: Some(store),
            api_key_auth: Some(api_key_auth),
        })
    }

    pub fn from_assessor(assessor: Arc<dyn ContentAssessor>) -> Self {
        Self {
            assessor,
            hosted_store: None,
            api_key_auth: None,
        }
    }

    pub fn hosted_store(&self) -> Option<&HostedStore> {
        self.hosted_store.as_deref()
    }

    fn is_hosted(&self) -> bool {
        self.hosted_store.is_some()
    }
}

async fn require_api_key(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(auth) = &state.api_key_auth else {
        return next.run(request).await;
    };
    match auth.authenticate(request.headers()) {
        Ok(tenant) => {
            request.extensions_mut().insert::<TenantId>(tenant);
            next.run(request).await
        }
        Err(AuthRejection::Unauthorized) => {
            let mut response = (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            response
        }
        Err(AuthRejection::RateLimited {
            retry_after_seconds,
        }) => {
            let mut response =
                (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded").into_response();
            if let Ok(value) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            response
        }
    }
}

pub fn build_app(state: Arc<AppState>) -> axum::Router {
    use axum::routing::{get, post};
    let mut api = axum::Router::new()
        .route("/v1/check", post(routes::check::check))
        .route("/v1/check/batch", post(routes::check::check_batch));
    if state.is_hosted() {
        api = api
            .route("/v1/feedback", post(routes::feedback::feedback))
            .route_layer(axum::middleware::from_fn_with_state(
                Arc::clone(&state),
                require_api_key,
            ));
    }
    axum::Router::new()
        .route("/healthz", get(routes::meta::healthz))
        .route("/llms.txt", get(routes::meta::llms_txt))
        .route("/openapi.json", get(routes::meta::openapi_json))
        .merge(api)
        .with_state(state)
}
