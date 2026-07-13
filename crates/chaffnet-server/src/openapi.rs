use crate::api_types::{
    BatchRequest, BatchResponse, CheckRequest, CheckResponse, ContextDto, FeedbackRequest,
    FeedbackResponse, FeedbackVerdictDto,
};
use utoipa::openapi::security::{Http, HttpAuthScheme, SecurityScheme};
use utoipa::{Modify, OpenApi};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::check::check,
        crate::routes::check::check_batch,
        crate::routes::feedback::feedback
    ),
    components(schemas(
        CheckRequest,
        CheckResponse,
        BatchRequest,
        BatchResponse,
        ContextDto,
        FeedbackRequest,
        FeedbackResponse,
        FeedbackVerdictDto
    )),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
            );
        }
    }
}
