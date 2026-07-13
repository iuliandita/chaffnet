use crate::config::EngineConfig;
use crate::features::Features;

pub const SPAM_MODEL_FEATURE_NAMES: [&str; 7] = [
    "ln_1p_word_count",
    "ln_1p_link_count",
    "link_ratio",
    "uppercase_ratio",
    "lexical_diversity",
    "char_entropy",
    "avg_word_length",
];

pub fn spam_model_features(features: &Features) -> [f32; 7] {
    [
        (features.word_count as f32).ln_1p(),
        (features.link_count as f32).ln_1p(),
        features.link_ratio,
        features.uppercase_ratio,
        features.lexical_diversity,
        features.char_entropy,
        features.avg_word_length,
    ]
}

pub fn spam_rule_offset(features: &Features, config: &EngineConfig) -> f32 {
    config.base_spam_logodds
        + crate::rules::evaluate(features, config)
            .iter()
            .map(|signal| signal.spam_logodds)
            .sum::<f32>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::content::{Content, ContentContext};
    use crate::features::Features;

    #[test]
    fn feature_names_and_order_are_stable() {
        assert_eq!(
            SPAM_MODEL_FEATURE_NAMES,
            [
                "ln_1p_word_count",
                "ln_1p_link_count",
                "link_ratio",
                "uppercase_ratio",
                "lexical_diversity",
                "char_entropy",
                "avg_word_length",
            ]
        );
        let features = Features::extract(&Content::new(
            "BUY now https://example.test",
            ContentContext::Comment,
        ));
        let vector = spam_model_features(&features);
        assert_eq!(vector[0], (features.word_count as f32).ln_1p());
        assert_eq!(vector[1], (features.link_count as f32).ln_1p());
        assert_eq!(vector[2], features.link_ratio);
        assert_eq!(vector[3], features.uppercase_ratio);
        assert_eq!(vector[4], features.lexical_diversity);
        assert_eq!(vector[5], features.char_entropy);
        assert_eq!(vector[6], features.avg_word_length);
    }

    #[test]
    fn empty_content_produces_finite_features() {
        let features = Features::extract(&Content::new("", ContentContext::Other));
        assert!(spam_model_features(&features)
            .iter()
            .all(|value| value.is_finite()));
    }

    #[test]
    fn rule_offset_matches_base_plus_firing_rules() {
        let config = EngineConfig::default();
        let features = Features::extract(&Content::new(
            "BUY BUY BUY https://a.test https://b.test",
            ContentContext::Comment,
        ));
        let expected = config.base_spam_logodds
            + crate::rules::evaluate(&features, &config)
                .iter()
                .map(|signal| signal.spam_logodds)
                .sum::<f32>();
        assert_eq!(spam_rule_offset(&features, &config), expected);
    }
}
