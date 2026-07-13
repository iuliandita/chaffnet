use chaffnet_core::content::{Content, ContentContext};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckRequest {
    /// The content to assess.
    pub text: String,
    /// One of: comment, review, forum, other. Defaults to "other".
    #[serde(default)]
    pub context: ContextDto,
    // utoipa 5 has no built-in schema for `IpAddr`; it serializes as a JSON string,
    // so describe it as an optional string in the OpenAPI doc.
    #[schema(value_type = Option<String>)]
    pub author_ip: Option<IpAddr>,
    pub author_email: Option<String>,
    pub author_name: Option<String>,
}

#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContextDto {
    Comment,
    Review,
    Forum,
    #[default]
    Other,
}

impl From<ContextDto> for ContentContext {
    fn from(c: ContextDto) -> Self {
        match c {
            ContextDto::Comment => ContentContext::Comment,
            ContextDto::Review => ContentContext::Review,
            ContextDto::Forum => ContentContext::Forum,
            ContextDto::Other => ContentContext::Other,
        }
    }
}

impl CheckRequest {
    pub fn into_content(self) -> Content {
        Content {
            text: self.text,
            author_ip: self.author_ip,
            author_email: self.author_email,
            author_name: self.author_name,
            context: self.context.into(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CheckResponse {
    pub spam: f32,
    pub slop: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BatchRequest {
    pub items: Vec<CheckRequest>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BatchResponse {
    pub results: Vec<CheckResponse>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct FeedbackRequest {
    pub content: CheckRequest,
    pub verdict: FeedbackVerdictDto,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackVerdictDto {
    Ham,
    Spam,
}

impl From<FeedbackVerdictDto> for chaffnet_core::reputation_hosted::FeedbackVerdict {
    fn from(value: FeedbackVerdictDto) -> Self {
        match value {
            FeedbackVerdictDto::Ham => Self::Ham,
            FeedbackVerdictDto::Spam => Self::Spam,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FeedbackResponse {
    pub accepted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_request_deserializes_minimal_body() {
        let json = r#"{"text":"hello","context":"comment"}"#;
        let req: CheckRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "hello");
        assert!(req.author_ip.is_none());
    }

    #[test]
    fn check_response_serializes_assessment() {
        let resp = CheckResponse {
            spam: 0.9,
            slop: 0.1,
            reasons: vec!["high_link_ratio".into()],
        };
        let j = serde_json::to_string(&resp).unwrap();
        assert!(j.contains("\"spam\":0.9"));
    }
}
