use std::net::IpAddr;

/// Where the content was submitted. Influences rule thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentContext {
    Comment,
    Review,
    Forum,
    Other,
}

/// A single piece of user-generated content to assess.
#[derive(Debug, Clone)]
pub struct Content {
    pub text: String,
    pub author_ip: Option<IpAddr>,
    pub author_email: Option<String>,
    pub author_name: Option<String>,
    pub context: ContentContext,
}

impl Content {
    /// Construct content with only the required fields.
    pub fn new(text: impl Into<String>, context: ContentContext) -> Self {
        Self {
            text: text.into(),
            author_ip: None,
            author_email: None,
            author_name: None,
            context,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_defaults_to_empty_optionals() {
        let c = Content::new("hello world", ContentContext::Comment);
        assert_eq!(c.text, "hello world");
        assert!(c.author_ip.is_none());
        assert!(c.author_email.is_none());
        assert!(matches!(c.context, ContentContext::Comment));
    }
}
