use crate::assessment::{ReasonCode, Signal};
use crate::config::EngineConfig;
use crate::features::Features;
use std::collections::{HashMap, HashSet};

/// Reputation lookups keyed by irreversible fingerprints. Implementations decide
/// storage; the local self-host default is redb (see `reputation_local`) and the
/// hosted store combines that baseline with tenant-consensus network feedback.
pub trait ReputationStore: Send + Sync {
    /// Spam score (0=clean, 1=known spammer) for a hashed IP bucket, if known.
    fn ip_bucket_score(&self, bucket: u64) -> Option<f32>;
    /// Spam score for an email domain, if known.
    fn email_domain_score(&self, domain: &str) -> Option<f32>;
    /// Spam score for a content SimHash, if a near-duplicate was seen before.
    fn fingerprint_score(&self, fp: u64) -> Option<f32>;
    /// Whether a domain is a known disposable/throwaway provider.
    fn is_disposable_domain(&self, domain: &str) -> bool;
}

impl<T: ReputationStore + ?Sized> ReputationStore for std::sync::Arc<T> {
    fn ip_bucket_score(&self, bucket: u64) -> Option<f32> {
        (**self).ip_bucket_score(bucket)
    }

    fn email_domain_score(&self, domain: &str) -> Option<f32> {
        (**self).email_domain_score(domain)
    }

    fn fingerprint_score(&self, fp: u64) -> Option<f32> {
        (**self).fingerprint_score(fp)
    }

    fn is_disposable_domain(&self, domain: &str) -> bool {
        (**self).is_disposable_domain(domain)
    }
}

/// Turn reputation lookups into [`Signal`]s. Score (0..1) is scaled into log-odds
/// by the configured weight.
pub fn evaluate<S: ReputationStore + ?Sized>(
    f: &Features,
    store: &S,
    cfg: &EngineConfig,
) -> Vec<Signal> {
    let mut out = Vec::new();

    if let Some(bucket) = f.ip_bucket {
        if let Some(score) = store.ip_bucket_score(bucket) {
            if score > 0.5 {
                out.push(Signal::spam(
                    score * cfg.reputation_weight,
                    ReasonCode::IpReputationBad,
                ));
            }
        }
    }

    if let Some(domain) = &f.email_domain {
        if store.is_disposable_domain(domain) {
            out.push(Signal::spam(
                cfg.disposable_email_weight,
                ReasonCode::DisposableEmailDomain,
            ));
        }
        if let Some(score) = store.email_domain_score(domain) {
            if score > 0.5 {
                out.push(Signal::spam(
                    score * cfg.reputation_weight,
                    ReasonCode::EmailDomainReputationBad,
                ));
            }
        }
    }

    if let Some(score) = store.fingerprint_score(f.content_fingerprint) {
        if score > 0.5 {
            out.push(Signal::spam(
                score * cfg.fingerprint_weight,
                ReasonCode::ContentFingerprintSeenSpam,
            ));
        }
    }

    out
}

/// A simple in-memory [`ReputationStore`] for tests and ephemeral use.
#[derive(Debug, Default)]
pub struct MemoryStore {
    ip: HashMap<u64, f32>,
    domain: HashMap<String, f32>,
    fingerprints: Vec<(u64, f32)>,
    disposable: HashSet<String>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_ip_bucket_score(&mut self, bucket: u64, score: f32) {
        self.ip.insert(bucket, score);
    }
    pub fn set_email_domain_score(&mut self, domain: &str, score: f32) {
        self.domain.insert(domain.to_lowercase(), score);
    }
    pub fn add_fingerprint(&mut self, fp: u64, score: f32) {
        self.fingerprints.push((fp, score));
    }
    pub fn add_disposable_domain(&mut self, domain: &str) {
        self.disposable.insert(domain.to_lowercase());
    }
}

impl ReputationStore for MemoryStore {
    fn ip_bucket_score(&self, bucket: u64) -> Option<f32> {
        self.ip.get(&bucket).copied()
    }
    fn email_domain_score(&self, domain: &str) -> Option<f32> {
        self.domain.get(&domain.to_lowercase()).copied()
    }
    fn fingerprint_score(&self, fp: u64) -> Option<f32> {
        // Near-duplicate: best score among fingerprints within Hamming distance 3.
        self.fingerprints
            .iter()
            .filter(|(known, _)| crate::fingerprint::hamming(*known, fp) <= 3)
            .map(|(_, score)| *score)
            .fold(None, |acc: Option<f32>, s| {
                Some(acc.map_or(s, |a| a.max(s)))
            })
    }
    fn is_disposable_domain(&self, domain: &str) -> bool {
        self.disposable.contains(&domain.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::content::{Content, ContentContext};
    use crate::features::Features;
    use std::net::{IpAddr, Ipv4Addr};

    fn feats_with(email: &str, ip: Ipv4Addr) -> Features {
        let mut c = Content::new("hello there friend", ContentContext::Comment);
        c.author_email = Some(email.into());
        c.author_ip = Some(IpAddr::V4(ip));
        Features::extract(&c)
    }

    #[test]
    fn memory_store_reports_seeded_disposable_domain() {
        let mut s = MemoryStore::new();
        s.add_disposable_domain("mailinator.com");
        assert!(s.is_disposable_domain("mailinator.com"));
        assert!(!s.is_disposable_domain("gmail.com"));
    }

    #[test]
    fn disposable_domain_fires_signal() {
        let cfg = EngineConfig::default();
        let mut s = MemoryStore::new();
        s.add_disposable_domain("mailinator.com");
        let f = feats_with("bob@mailinator.com", Ipv4Addr::new(10, 0, 0, 1));
        let signals = evaluate(&f, &s, &cfg);
        assert!(signals
            .iter()
            .any(|x| x.reason == Some(ReasonCode::DisposableEmailDomain)));
    }

    #[test]
    fn bad_ip_bucket_fires_signal() {
        let cfg = EngineConfig::default();
        let mut s = MemoryStore::new();
        let ip = Ipv4Addr::new(203, 0, 113, 5);
        let bucket = crate::fingerprint::ip_bucket(IpAddr::V4(ip));
        s.set_ip_bucket_score(bucket, 0.9);
        let f = feats_with("bob@example.com", ip);
        let signals = evaluate(&f, &s, &cfg);
        assert!(signals
            .iter()
            .any(|x| x.reason == Some(ReasonCode::IpReputationBad)));
    }

    #[test]
    fn clean_reputation_fires_nothing() {
        let cfg = EngineConfig::default();
        let s = MemoryStore::new();
        let f = feats_with("bob@example.com", Ipv4Addr::new(10, 0, 0, 1));
        assert!(evaluate(&f, &s, &cfg).is_empty());
    }
}
