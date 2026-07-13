use crate::assessment::{Assessment, ReasonCode};
use crate::classifier::{Classifier, ClassifierError};
use crate::config::EngineConfig;
use crate::content::Content;
use crate::features::Features;
use crate::reputation::ReputationStore;
use crate::{reputation, rules};

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// The assembled engine. Generic over the reputation store and classifier so the
/// hosted tier and a trained model can swap in without changing this code.
pub struct Engine<S: ReputationStore, C: Classifier> {
    store: S,
    classifier: C,
    config: EngineConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Classifier(#[from] ClassifierError),
}

impl<S: ReputationStore, C: Classifier> Engine<S, C> {
    pub fn new(store: S, classifier: C, config: EngineConfig) -> Self {
        Self {
            store,
            classifier,
            config,
        }
    }

    /// Read-only access to the store (for feedback ingestion by the caller).
    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn assess(&self, content: &Content) -> Result<Assessment, EngineError> {
        let f = Features::extract(content);
        let cfg = &self.config;

        let mut spam_lo = cfg.base_spam_logodds;
        let mut slop_lo = cfg.base_slop_logodds;
        let mut reasons: Vec<ReasonCode> = Vec::new();

        for sig in rules::evaluate(&f, cfg) {
            spam_lo += sig.spam_logodds;
            slop_lo += sig.slop_logodds;
            if let Some(r) = sig.reason {
                reasons.push(r);
            }
        }
        for sig in reputation::evaluate(&f, &self.store, cfg) {
            spam_lo += sig.spam_logodds;
            slop_lo += sig.slop_logodds;
            if let Some(r) = sig.reason {
                reasons.push(r);
            }
        }

        let classifier = self.classifier.classify(&f)?;
        spam_lo += cfg.classifier_spam_weight * classifier.spam_logodds;
        slop_lo += cfg.classifier_slop_weight * classifier.slop_logodds;

        // Gate the stylometry reason on the FINAL slop score, not the
        // classifier's partial contribution, so a reported reason always implies
        // the reported slop is above threshold.
        let slop = sigmoid(slop_lo);
        if slop > cfg.slop_reason_threshold {
            reasons.push(ReasonCode::AiStylometryHigh);
        }

        // Stable de-dup preserving first-seen order.
        let mut seen = std::collections::HashSet::new();
        reasons.retain(|r| seen.insert(*r));

        Ok(Assessment {
            spam: sigmoid(spam_lo),
            slop,
            reasons,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::{ClassifierError, ClassifierOutput};
    use crate::content::{Content, ContentContext};
    use crate::features::Features;
    use crate::reputation::MemoryStore;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailingClassifier {
        calls: AtomicUsize,
    }

    impl Classifier for FailingClassifier {
        fn classify(&self, _features: &Features) -> Result<ClassifierOutput, ClassifierError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Err(ClassifierError::Inference("forced".into()))
        }
    }

    fn engine() -> Engine<MemoryStore, crate::classifier::BaselineClassifier> {
        Engine::new(MemoryStore::new(), Default::default(), Default::default())
    }

    #[test]
    fn scores_are_bounded() {
        let e = engine();
        let a = e
            .assess(&Content::new("hello", ContentContext::Comment))
            .unwrap();
        assert!(a.spam >= 0.0 && a.spam <= 1.0);
        assert!(a.slop >= 0.0 && a.slop <= 1.0);
    }

    #[test]
    fn clean_comment_scores_low_spam() {
        let e = engine();
        let a = e
            .assess(&Content::new(
                "Great point about idempotency, I hadn't considered the retry case.",
                ContentContext::Comment,
            ))
            .unwrap();
        assert!(a.spam < 0.3, "spam was {}", a.spam);
    }

    #[test]
    fn obvious_spam_scores_high_and_lists_reasons() {
        let e = engine();
        let a = e
            .assess(&Content::new(
                "BUY CHEAP WATCHES https://a.io https://b.io https://c.io FREE FREE FREE",
                ContentContext::Comment,
            ))
            .unwrap();
        assert!(a.spam > 0.7, "spam was {}", a.spam);
        assert!(!a.reasons.is_empty());
    }

    #[test]
    fn stylometry_reason_is_consistent_with_reported_slop() {
        let e = engine();
        let threshold = EngineConfig::default().slop_reason_threshold;
        // A short, plain comment lands below the slop threshold.
        let low = e
            .assess(&Content::new("thanks, fixed it", ContentContext::Comment))
            .unwrap();
        assert!(low.slop <= threshold, "expected low slop, was {}", low.slop);
        assert!(
            !low.reasons.contains(&ReasonCode::AiStylometryHigh),
            "AiStylometryHigh must not fire when slop {} <= threshold {}",
            low.slop,
            threshold,
        );

        // Invariant across a spread of inputs: the reason is present iff the
        // FINAL reported slop exceeds the threshold.
        for text in [
            "thanks, fixed it",
            "Great point about idempotency, I hadn't considered the retry case.",
            "In conclusion this is a great product and I highly recommend it to everyone always",
            "BUY CHEAP WATCHES https://a.io https://b.io https://c.io FREE FREE FREE",
        ] {
            let a = e
                .assess(&Content::new(text, ContentContext::Comment))
                .unwrap();
            assert_eq!(
                a.reasons.contains(&ReasonCode::AiStylometryHigh),
                a.slop > threshold,
                "reason/slop mismatch for {text:?}: slop={} reasons={:?}",
                a.slop,
                a.reasons,
            );
        }
    }

    #[test]
    fn reasons_are_deduplicated_and_stable() {
        let e = engine();
        let a = e
            .assess(&Content::new(
                "SPAM SPAM SPAM https://a.io https://b.io",
                ContentContext::Comment,
            ))
            .unwrap();
        let mut sorted = a.reasons.clone();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            a.reasons.len(),
            "reasons contained duplicates"
        );
    }

    #[test]
    fn classifier_failure_is_propagated_after_one_call() {
        let engine = Engine::new(
            MemoryStore::new(),
            FailingClassifier {
                calls: AtomicUsize::new(0),
            },
            EngineConfig::default(),
        );
        let error = engine
            .assess(&Content::new("hello", ContentContext::Comment))
            .unwrap_err();
        assert!(matches!(error, EngineError::Classifier(_)));
        assert_eq!(engine.classifier.calls.load(Ordering::Relaxed), 1);
    }
}
