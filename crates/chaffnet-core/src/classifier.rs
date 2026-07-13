use crate::features::Features;

#[derive(Debug, Clone, Copy)]
pub struct ClassifierOutput {
    pub spam_logodds: f32,
    pub slop_logodds: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum ClassifierError {
    #[error("classifier initialization failed: {0}")]
    Initialization(String),
    #[error("classifier inference failed: {0}")]
    Inference(String),
}

/// Produces raw spam/slop log-odds from features.
pub trait Classifier: Send + Sync {
    fn classify(&self, features: &Features) -> Result<ClassifierOutput, ClassifierError>;
}

/// A deterministic logistic model with hand-set weights. Good enough to ship and
/// fully testable; the weights are the thing a trained model later replaces.
#[derive(Debug, Clone)]
pub struct BaselineClassifier {
    // spam weights
    w_link_ratio: f32,
    w_uppercase: f32,
    w_low_diversity: f32,
    spam_bias: f32,
    // slop weights: slop = "smooth, generic, even" text
    w_high_diversity_penalty: f32,
    w_uniform_word_len: f32,
    w_high_entropy: f32,
    slop_bias: f32,
}

impl Default for BaselineClassifier {
    fn default() -> Self {
        Self {
            w_link_ratio: 4.0,
            w_uppercase: 1.5,
            w_low_diversity: 2.0,
            spam_bias: -1.5,
            w_high_diversity_penalty: -1.0,
            w_uniform_word_len: 0.8,
            w_high_entropy: 0.25,
            slop_bias: -1.0,
        }
    }
}

impl BaselineClassifier {
    fn spam_logodds(&self, f: &Features) -> f32 {
        self.spam_bias
            + self.w_link_ratio * f.link_ratio
            + self.w_uppercase * f.uppercase_ratio
            + self.w_low_diversity * (1.0 - f.lexical_diversity)
    }

    fn slop_logodds(&self, f: &Features) -> f32 {
        // Slop tends to be fluent, even, and moderately diverse but templated.
        // Heuristic proxy: penalize very low diversity (that's spam, not slop),
        // reward even word length and high-but-not-chaotic entropy.
        let word_len_evenness = 1.0 / (1.0 + (f.avg_word_length - 5.0).abs());
        self.slop_bias
            + self.w_high_diversity_penalty * (1.0 - f.lexical_diversity)
            + self.w_uniform_word_len * word_len_evenness
            + self.w_high_entropy * f.char_entropy
    }
}

impl Classifier for BaselineClassifier {
    fn classify(&self, features: &Features) -> Result<ClassifierOutput, ClassifierError> {
        Ok(ClassifierOutput {
            spam_logodds: self.spam_logodds(features),
            slop_logodds: self.slop_logodds(features),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Content, ContentContext};
    use crate::features::Features;

    fn feats(text: &str) -> Features {
        Features::extract(&Content::new(text, ContentContext::Comment))
    }

    #[test]
    fn spammy_features_score_higher_than_clean() {
        let clf = BaselineClassifier::default();
        let spammy = clf.spam_logodds(&feats("FREE https://x.com https://y.com FREE FREE FREE"));
        let clean = clf.spam_logodds(&feats(
            "Thanks for the thoughtful writeup, I learned something new about caching today.",
        ));
        assert!(spammy > clean);
    }

    #[test]
    fn uniform_low_entropy_text_reads_as_more_sloppy() {
        let clf = BaselineClassifier::default();
        // Very even, templated phrasing vs. specific human detail.
        let sloppy = clf.slop_logodds(&feats(
            "In conclusion this is a great product and I highly recommend it to everyone always",
        ));
        let human = clf.slop_logodds(&feats(
            "ok so the zipper broke after two days, i emailed them but no reply yet lol",
        ));
        assert!(sloppy > human);
    }
}
