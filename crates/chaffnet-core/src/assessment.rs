use serde::Serialize;

/// A stable, machine-readable explanation for why a signal fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    HighLinkRatio,
    KnownSpamPattern,
    ExcessiveCapitalization,
    LowLexicalDiversity,
    IpReputationBad,
    DisposableEmailDomain,
    EmailDomainReputationBad,
    ContentFingerprintSeenSpam,
    AiStylometryHigh,
}

impl ReasonCode {
    /// Stable snake_case string, matching the serde representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            ReasonCode::HighLinkRatio => "high_link_ratio",
            ReasonCode::KnownSpamPattern => "known_spam_pattern",
            ReasonCode::ExcessiveCapitalization => "excessive_capitalization",
            ReasonCode::LowLexicalDiversity => "low_lexical_diversity",
            ReasonCode::IpReputationBad => "ip_reputation_bad",
            ReasonCode::DisposableEmailDomain => "disposable_email_domain",
            ReasonCode::EmailDomainReputationBad => "email_domain_reputation_bad",
            ReasonCode::ContentFingerprintSeenSpam => "content_fingerprint_seen_spam",
            ReasonCode::AiStylometryHigh => "ai_stylometry_high",
        }
    }
}

/// A single layer's additive contribution to the fold.
#[derive(Debug, Clone, Copy, Default)]
pub struct Signal {
    pub spam_logodds: f32,
    pub slop_logodds: f32,
    pub reason: Option<ReasonCode>,
}

impl Signal {
    /// A signal that only nudges spam and carries a reason.
    pub fn spam(logodds: f32, reason: ReasonCode) -> Self {
        Self {
            spam_logodds: logodds,
            slop_logodds: 0.0,
            reason: Some(reason),
        }
    }

    /// A signal that only nudges slop and carries a reason.
    pub fn slop(logodds: f32, reason: ReasonCode) -> Self {
        Self {
            spam_logodds: 0.0,
            slop_logodds: logodds,
            reason: Some(reason),
        }
    }
}

/// The engine's verdict: two scores normalized to 0..=1 and the reasons behind them.
#[derive(Debug, Clone, Serialize)]
pub struct Assessment {
    pub spam: f32,
    pub slop: f32,
    pub reasons: Vec<ReasonCode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_zero_is_neutral() {
        let s = Signal::default();
        assert_eq!(s.spam_logodds, 0.0);
        assert_eq!(s.slop_logodds, 0.0);
        assert!(s.reason.is_none());
    }

    #[test]
    fn reason_code_serializes_to_snake_case() {
        let j = serde_json::to_string(&ReasonCode::HighLinkRatio).unwrap();
        assert_eq!(j, "\"high_link_ratio\"");
    }

    #[test]
    fn as_str_matches_serde_representation() {
        // Guards against the inherent as_str() and the serde derive drifting apart.
        for rc in [
            ReasonCode::HighLinkRatio,
            ReasonCode::ExcessiveCapitalization,
            ReasonCode::DisposableEmailDomain,
            ReasonCode::AiStylometryHigh,
        ] {
            let serde = serde_json::to_value(rc).unwrap();
            assert_eq!(serde.as_str().unwrap(), rc.as_str());
        }
    }
}
