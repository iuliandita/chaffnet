use crate::content::Content;
use crate::fingerprint::{email_domain, ip_bucket, simhash};
use std::collections::HashMap;

/// Numeric features derived from a [`Content`]. Layers consume these; they never
/// re-parse raw text.
#[derive(Debug, Clone)]
pub struct Features {
    pub word_count: usize,
    pub link_count: usize,
    pub link_ratio: f32,
    pub uppercase_ratio: f32,
    pub lexical_diversity: f32,
    pub char_entropy: f32,
    pub avg_word_length: f32,
    pub email_domain: Option<String>,
    pub content_fingerprint: u64,
    pub ip_bucket: Option<u64>,
}

fn is_link(token: &str) -> bool {
    token.starts_with("http://") || token.starts_with("https://") || token.starts_with("www.")
}

fn char_entropy(text: &str) -> f32 {
    let mut counts: HashMap<char, usize> = HashMap::new();
    let mut total = 0usize;
    for ch in text.chars().filter(|c| !c.is_whitespace()) {
        *counts.entry(ch).or_insert(0) += 1;
        total += 1;
    }
    if total == 0 {
        return 0.0;
    }
    let total_f = total as f32;
    -counts
        .values()
        .map(|&n| {
            let p = n as f32 / total_f;
            p * p.log2()
        })
        .sum::<f32>()
}

impl Features {
    pub fn extract(content: &Content) -> Self {
        let text = &content.text;
        let tokens: Vec<&str> = text.split_whitespace().collect();
        let word_count = tokens.len();
        let link_count = tokens.iter().filter(|t| is_link(t)).count();
        let link_ratio = link_count as f32 / word_count.max(1) as f32;

        let letters = text.chars().filter(|c| c.is_alphabetic()).count();
        let uppercase = text.chars().filter(|c| c.is_uppercase()).count();
        let uppercase_ratio = uppercase as f32 / letters.max(1) as f32;

        let unique: std::collections::HashSet<String> =
            tokens.iter().map(|t| t.to_lowercase()).collect();
        let lexical_diversity = unique.len() as f32 / word_count.max(1) as f32;

        let total_word_len: usize = tokens.iter().map(|t| t.chars().count()).sum();
        let avg_word_length = total_word_len as f32 / word_count.max(1) as f32;

        Features {
            word_count,
            link_count,
            link_ratio,
            uppercase_ratio,
            lexical_diversity,
            char_entropy: char_entropy(text),
            avg_word_length,
            email_domain: content.author_email.as_deref().and_then(email_domain),
            content_fingerprint: simhash(text),
            ip_bucket: content.author_ip.map(ip_bucket),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Content, ContentContext};
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn counts_words_and_links() {
        let c = Content::new(
            "check out https://a.com and http://b.com now",
            ContentContext::Comment,
        );
        let f = Features::extract(&c);
        assert_eq!(f.link_count, 2);
        assert_eq!(f.word_count, 6);
        assert!((f.link_ratio - 2.0 / 6.0).abs() < 1e-6);
    }

    #[test]
    fn uppercase_ratio_detects_shouting() {
        let c = Content::new("BUY NOW", ContentContext::Comment);
        let f = Features::extract(&c);
        assert!(f.uppercase_ratio > 0.9);
    }

    #[test]
    fn low_diversity_on_repetition() {
        let c = Content::new("spam spam spam spam spam", ContentContext::Comment);
        let f = Features::extract(&c);
        assert!(f.lexical_diversity < 0.3);
    }

    #[test]
    fn extracts_email_domain_and_ip_bucket() {
        let mut c = Content::new("hi", ContentContext::Comment);
        c.author_email = Some("a@Spammy.io".into());
        c.author_ip = Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)));
        let f = Features::extract(&c);
        assert_eq!(f.email_domain.as_deref(), Some("spammy.io"));
        assert!(f.ip_bucket.is_some());
    }
}
