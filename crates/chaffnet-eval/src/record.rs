use chaffnet_core::content::{Content, ContentContext};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvalContext {
    Comment,
    Review,
    Forum,
    #[default]
    Other,
}

impl From<EvalContext> for ContentContext {
    fn from(value: EvalContext) -> Self {
        match value {
            EvalContext::Comment => ContentContext::Comment,
            EvalContext::Review => ContentContext::Review,
            EvalContext::Forum => ContentContext::Forum,
            EvalContext::Other => ContentContext::Other,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalRecord {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub context: EvalContext,
    pub spam: Option<bool>,
    pub slop: Option<bool>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RecordError {
    #[error("record id must not be empty")]
    EmptyId,
    #[error("record text must not be empty")]
    EmptyText,
    #[error("record must include a spam or slop label")]
    MissingLabel,
}

impl EvalRecord {
    pub fn validate(&self) -> Result<(), RecordError> {
        if self.id.trim().is_empty() {
            return Err(RecordError::EmptyId);
        }
        if self.text.trim().is_empty() {
            return Err(RecordError::EmptyText);
        }
        if self.spam.is_none() && self.slop.is_none() {
            return Err(RecordError::MissingLabel);
        }
        Ok(())
    }

    pub fn into_content(self) -> Content {
        Content::new(self.text, self.context.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chaffnet_core::content::ContentContext;

    #[test]
    fn record_requires_at_least_one_label() {
        let record = EvalRecord {
            id: "missing-label".into(),
            text: "hello".into(),
            context: EvalContext::Other,
            spam: None,
            slop: None,
        };
        assert_eq!(record.validate().unwrap_err(), RecordError::MissingLabel);
    }

    #[test]
    fn context_maps_to_core_type() {
        assert_eq!(
            ContentContext::from(EvalContext::Comment),
            ContentContext::Comment
        );
    }
}
