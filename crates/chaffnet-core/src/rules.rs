use crate::assessment::{ReasonCode, Signal};
use crate::config::EngineConfig;
use crate::features::Features;

/// Run the deterministic heuristic rules. Each firing rule yields one [`Signal`].
/// Pure function of features + config; no I/O, no state.
pub fn evaluate(f: &Features, cfg: &EngineConfig) -> Vec<Signal> {
    let mut out = Vec::new();

    if f.word_count > 0 && f.link_ratio > cfg.link_ratio_threshold {
        out.push(Signal::spam(
            cfg.link_ratio_weight,
            ReasonCode::HighLinkRatio,
        ));
    }
    // Only judge capitalization on content long enough to be meaningful.
    if f.word_count >= 3 && f.uppercase_ratio > cfg.uppercase_threshold {
        out.push(Signal::spam(
            cfg.uppercase_weight,
            ReasonCode::ExcessiveCapitalization,
        ));
    }
    if f.word_count >= 5 && f.lexical_diversity < cfg.low_diversity_threshold {
        out.push(Signal::spam(
            cfg.low_diversity_weight,
            ReasonCode::LowLexicalDiversity,
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::content::{Content, ContentContext};
    use crate::features::Features;

    fn feats(text: &str) -> Features {
        Features::extract(&Content::new(text, ContentContext::Comment))
    }

    #[test]
    fn clean_text_fires_no_rules() {
        let cfg = EngineConfig::default();
        let signals = evaluate(
            &feats("I really enjoyed this article, thanks for writing it."),
            &cfg,
        );
        assert!(signals.is_empty());
    }

    #[test]
    fn link_heavy_text_fires_link_rule() {
        let cfg = EngineConfig::default();
        let signals = evaluate(
            &feats("buy https://a.com https://b.com https://c.com"),
            &cfg,
        );
        assert!(signals
            .iter()
            .any(|s| s.reason == Some(ReasonCode::HighLinkRatio)));
        assert!(
            signals
                .iter()
                .find(|s| s.reason == Some(ReasonCode::HighLinkRatio))
                .unwrap()
                .spam_logodds
                > 0.0
        );
    }

    #[test]
    fn shouting_fires_capitalization_rule() {
        let cfg = EngineConfig::default();
        let signals = evaluate(&feats("BUY CHEAP MEDS NOW CLICK HERE"), &cfg);
        assert!(signals
            .iter()
            .any(|s| s.reason == Some(ReasonCode::ExcessiveCapitalization)));
    }
}
