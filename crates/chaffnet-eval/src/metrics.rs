use serde::Serialize;

const SWEEP_THRESHOLDS: [f64; 5] = [0.50, 0.70, 0.80, 0.90, 0.95];
const CALIBRATION_BINS: usize = 10;

#[derive(Debug, Clone, Copy)]
pub struct LabeledScore {
    pub score: f64,
    pub label: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Confusion {
    pub true_positive: u64,
    pub false_positive: u64,
    pub true_negative: u64,
    pub false_negative: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThresholdMetrics {
    pub threshold: f64,
    pub confusion: Confusion,
    pub precision: Option<f64>,
    pub recall: Option<f64>,
    pub f1: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignalMetrics {
    pub labeled: usize,
    pub positive: usize,
    pub negative: usize,
    pub brier: f64,
    pub expected_calibration_error: f64,
    pub average_precision: Option<f64>,
    pub selected: ThresholdMetrics,
    pub sweep: Vec<ThresholdMetrics>,
}

#[derive(Debug, thiserror::Error)]
pub enum MetricError {
    #[error("at least one labeled score is required")]
    Empty,
    #[error("score must be finite and within 0..=1, got {0}")]
    InvalidScore(f64),
    #[error("threshold must be finite and within 0..=1, got {0}")]
    InvalidThreshold(f64),
}

fn validate_unit_interval(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn threshold_metrics(samples: &[LabeledScore], threshold: f64) -> ThresholdMetrics {
    let mut confusion = Confusion {
        true_positive: 0,
        false_positive: 0,
        true_negative: 0,
        false_negative: 0,
    };

    for sample in samples {
        match (sample.score >= threshold, sample.label) {
            (true, true) => confusion.true_positive += 1,
            (true, false) => confusion.false_positive += 1,
            (false, false) => confusion.true_negative += 1,
            (false, true) => confusion.false_negative += 1,
        }
    }

    let predicted_positive = confusion.true_positive + confusion.false_positive;
    let actual_positive = confusion.true_positive + confusion.false_negative;
    let precision = (predicted_positive > 0)
        .then(|| confusion.true_positive as f64 / predicted_positive as f64);
    let recall =
        (actual_positive > 0).then(|| confusion.true_positive as f64 / actual_positive as f64);
    let f1 = match (precision, recall) {
        (Some(precision), Some(recall)) if precision + recall > 0.0 => {
            Some(2.0 * precision * recall / (precision + recall))
        }
        (Some(_), Some(_)) => Some(0.0),
        _ => None,
    };

    ThresholdMetrics {
        threshold,
        confusion,
        precision,
        recall,
        f1,
    }
}

fn brier_score(samples: &[LabeledScore]) -> f64 {
    samples
        .iter()
        .map(|sample| {
            let label = if sample.label { 1.0 } else { 0.0 };
            (sample.score - label).powi(2)
        })
        .sum::<f64>()
        / samples.len() as f64
}

fn expected_calibration_error(samples: &[LabeledScore]) -> f64 {
    let mut count = [0usize; CALIBRATION_BINS];
    let mut score_sum = [0.0f64; CALIBRATION_BINS];
    let mut positive_sum = [0usize; CALIBRATION_BINS];

    for sample in samples {
        let bin =
            ((sample.score * CALIBRATION_BINS as f64).floor() as usize).min(CALIBRATION_BINS - 1);
        count[bin] += 1;
        score_sum[bin] += sample.score;
        positive_sum[bin] += usize::from(sample.label);
    }

    count
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .map(|(bin, count)| {
            let average_score = score_sum[bin] / *count as f64;
            let positive_rate = positive_sum[bin] as f64 / *count as f64;
            *count as f64 / samples.len() as f64 * (average_score - positive_rate).abs()
        })
        .sum()
}

fn average_precision(samples: &[LabeledScore]) -> Option<f64> {
    let positives = samples.iter().filter(|sample| sample.label).count();
    if positives == 0 {
        return None;
    }

    let mut ranked = samples.to_vec();
    ranked.sort_by(|left, right| right.score.total_cmp(&left.score));
    let mut seen_positive = 0usize;
    let mut seen_total = 0usize;
    let mut area = 0.0;
    let mut index = 0usize;
    while index < ranked.len() {
        let score = ranked[index].score;
        let group_start = index;
        let mut group_positive = 0usize;
        while index < ranked.len() && ranked[index].score == score {
            group_positive += usize::from(ranked[index].label);
            index += 1;
        }
        seen_total += index - group_start;
        seen_positive += group_positive;
        let recall_increment = group_positive as f64 / positives as f64;
        let precision = seen_positive as f64 / seen_total as f64;
        area += recall_increment * precision;
    }
    Some(area)
}

pub fn calculate(samples: &[LabeledScore], threshold: f64) -> Result<SignalMetrics, MetricError> {
    if samples.is_empty() {
        return Err(MetricError::Empty);
    }
    if !validate_unit_interval(threshold) {
        return Err(MetricError::InvalidThreshold(threshold));
    }
    for sample in samples {
        if !validate_unit_interval(sample.score) {
            return Err(MetricError::InvalidScore(sample.score));
        }
    }

    let positive = samples.iter().filter(|sample| sample.label).count();
    Ok(SignalMetrics {
        labeled: samples.len(),
        positive,
        negative: samples.len() - positive,
        brier: brier_score(samples),
        expected_calibration_error: expected_calibration_error(samples),
        average_precision: average_precision(samples),
        selected: threshold_metrics(samples, threshold),
        sweep: SWEEP_THRESHOLDS
            .into_iter()
            .map(|threshold| threshold_metrics(samples, threshold))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<LabeledScore> {
        vec![
            LabeledScore {
                score: 0.9,
                label: true,
            },
            LabeledScore {
                score: 0.8,
                label: false,
            },
            LabeledScore {
                score: 0.7,
                label: true,
            },
            LabeledScore {
                score: 0.1,
                label: false,
            },
        ]
    }

    #[test]
    fn calculates_threshold_and_probability_metrics() {
        let metrics = calculate(&sample(), 0.75).unwrap();
        assert_eq!(metrics.labeled, 4);
        assert_eq!(metrics.positive, 2);
        assert_eq!(metrics.negative, 2);
        assert_eq!(
            metrics.selected.confusion,
            Confusion {
                true_positive: 1,
                false_positive: 1,
                true_negative: 1,
                false_negative: 1,
            }
        );
        assert_eq!(metrics.selected.precision, Some(0.5));
        assert_eq!(metrics.selected.recall, Some(0.5));
        assert_eq!(metrics.selected.f1, Some(0.5));
        assert!((metrics.brier - 0.1875).abs() < 1e-12);
        assert!((metrics.expected_calibration_error - 0.325).abs() < 1e-12);
        assert!((metrics.average_precision.unwrap() - 5.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn rejects_non_finite_and_out_of_range_values() {
        for score in [f64::NAN, -0.1, 1.1] {
            let error = calculate(&[LabeledScore { score, label: true }], 0.5).unwrap_err();
            assert!(matches!(error, MetricError::InvalidScore(_)));
        }
        for threshold in [f64::NAN, -0.1, 1.1] {
            let error = calculate(&sample(), threshold).unwrap_err();
            assert!(matches!(error, MetricError::InvalidThreshold(_)));
        }
    }

    #[test]
    fn reports_undefined_positive_metrics_as_none() {
        let metrics = calculate(
            &[
                LabeledScore {
                    score: 0.2,
                    label: false,
                },
                LabeledScore {
                    score: 0.1,
                    label: false,
                },
            ],
            0.5,
        )
        .unwrap();
        assert_eq!(metrics.selected.precision, None);
        assert_eq!(metrics.selected.recall, None);
        assert_eq!(metrics.selected.f1, None);
        assert_eq!(metrics.average_precision, None);
    }

    #[test]
    fn average_precision_is_invariant_to_equal_score_order() {
        let positive_first = calculate(
            &[
                LabeledScore {
                    score: 0.5,
                    label: true,
                },
                LabeledScore {
                    score: 0.5,
                    label: false,
                },
            ],
            0.5,
        )
        .unwrap();
        let negative_first = calculate(
            &[
                LabeledScore {
                    score: 0.5,
                    label: false,
                },
                LabeledScore {
                    score: 0.5,
                    label: true,
                },
            ],
            0.5,
        )
        .unwrap();
        assert_eq!(
            positive_first.average_precision,
            negative_first.average_precision
        );
        assert_eq!(positive_first.average_precision, Some(0.5));
    }
}
