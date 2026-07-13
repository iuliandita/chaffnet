/// Tunable weights and priors for the engine. All log-odds contributions and the
/// base rates live here so behavior is data, not code.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub base_spam_logodds: f32, // prior; negative = "most content is not spam"
    pub base_slop_logodds: f32,
    pub link_ratio_threshold: f32,
    pub link_ratio_weight: f32,
    pub uppercase_threshold: f32,
    pub uppercase_weight: f32,
    pub low_diversity_threshold: f32,
    pub low_diversity_weight: f32,
    pub reputation_weight: f32, // multiplies reputation score (0..1) into log-odds
    pub disposable_email_weight: f32,
    pub fingerprint_weight: f32,
    pub classifier_spam_weight: f32, // scales classifier spam log-odds
    pub classifier_slop_weight: f32, // scales classifier slop log-odds (kept < spam, per R1)
    pub slop_reason_threshold: f32,  // final slop prob above which AiStylometryHigh is emitted
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            base_spam_logodds: -2.2, // ~10% prior
            base_slop_logodds: -1.4, // ~20% prior
            link_ratio_threshold: 0.15,
            link_ratio_weight: 2.5,
            uppercase_threshold: 0.6,
            uppercase_weight: 1.2,
            low_diversity_threshold: 0.35,
            low_diversity_weight: 1.0,
            reputation_weight: 3.0,
            disposable_email_weight: 1.5,
            fingerprint_weight: 3.5,
            classifier_spam_weight: 1.0,
            classifier_slop_weight: 0.7,
            slop_reason_threshold: 0.6,
        }
    }
}
