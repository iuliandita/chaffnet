use crate::metrics::{calculate, LabeledScore, MetricError, SignalMetrics};
use crate::record::{EvalRecord, RecordError};
use chaffnet_core::classifier::{BaselineClassifier, Classifier, ClassifierError};
use chaffnet_core::classifier_onnx::OnnxClassifier;
use chaffnet_core::config::EngineConfig;
use chaffnet_core::engine::EngineError;
use chaffnet_core::reputation::MemoryStore;
use chaffnet_core::Engine;
use serde::Serialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct EvalReport {
    pub source_records: usize,
    pub threshold: f64,
    pub spam: Option<SignalMetrics>,
    pub slop: Option<SignalMetrics>,
}

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("threshold must be finite and within 0..=1, got {0}")]
    InvalidThreshold(f64),
    #[error("evaluation corpus contains no records")]
    EmptyInput,
    #[error("failed to open {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read line {line}: {source}")]
    Read {
        line: usize,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON on line {line}: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid record {id:?} on line {line}: {source}")]
    Record {
        line: usize,
        id: String,
        #[source]
        source: RecordError,
    },
    #[error("duplicate record id {id:?} on line {line}")]
    DuplicateId { line: usize, id: String },
    #[error(transparent)]
    Metric(#[from] MetricError),
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Classifier(#[from] ClassifierError),
}

pub fn evaluate_reader<R: BufRead>(reader: R, threshold: f64) -> Result<EvalReport, EvalError> {
    evaluate_reader_with_classifier(reader, threshold, BaselineClassifier::default())
}

pub fn evaluate_reader_with_classifier<R, C>(
    reader: R,
    threshold: f64,
    classifier: C,
) -> Result<EvalReport, EvalError>
where
    R: BufRead,
    C: Classifier,
{
    if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
        return Err(EvalError::InvalidThreshold(threshold));
    }

    let engine = Engine::new(MemoryStore::new(), classifier, EngineConfig::default());
    let mut ids = HashSet::new();
    let mut source_records = 0usize;
    let mut spam = Vec::new();
    let mut slop = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.map_err(|source| EvalError::Read {
            line: line_number,
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }

        let record: EvalRecord = serde_json::from_str(&line).map_err(|source| EvalError::Json {
            line: line_number,
            source,
        })?;
        record.validate().map_err(|source| EvalError::Record {
            line: line_number,
            id: record.id.clone(),
            source,
        })?;
        if !ids.insert(record.id.clone()) {
            return Err(EvalError::DuplicateId {
                line: line_number,
                id: record.id,
            });
        }

        let spam_label = record.spam;
        let slop_label = record.slop;
        let assessment = engine.assess(&record.into_content())?;
        if let Some(label) = spam_label {
            spam.push(LabeledScore {
                score: f64::from(assessment.spam),
                label,
            });
        }
        if let Some(label) = slop_label {
            slop.push(LabeledScore {
                score: f64::from(assessment.slop),
                label,
            });
        }
        source_records += 1;
    }

    if source_records == 0 {
        return Err(EvalError::EmptyInput);
    }

    Ok(EvalReport {
        source_records,
        threshold,
        spam: (!spam.is_empty())
            .then(|| calculate(&spam, threshold))
            .transpose()?,
        slop: (!slop.is_empty())
            .then(|| calculate(&slop, threshold))
            .transpose()?,
    })
}

pub fn evaluate_path(path: &Path, threshold: f64) -> Result<EvalReport, EvalError> {
    evaluate_path_with_classifier(path, threshold, OnnxClassifier::bundled()?)
}

pub fn evaluate_path_baseline(path: &Path, threshold: f64) -> Result<EvalReport, EvalError> {
    evaluate_path_with_classifier(path, threshold, BaselineClassifier::default())
}

fn evaluate_path_with_classifier<C: Classifier>(
    path: &Path,
    threshold: f64,
    classifier: C,
) -> Result<EvalReport, EvalError> {
    let file = File::open(path).map_err(|source| EvalError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    evaluate_reader_with_classifier(BufReader::new(file), threshold, classifier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn evaluates_each_signal_only_when_labeled() {
        let input = concat!(
            r#"{"id":"one","text":"BUY NOW https://a.io","context":"comment","spam":true}"#,
            "\n",
            r#"{"id":"two","text":"specific human note","spam":false,"slop":true}"#,
            "\n"
        );
        let report = evaluate_reader(Cursor::new(input), 0.5).unwrap();
        assert_eq!(report.source_records, 2);
        assert_eq!(report.spam.unwrap().labeled, 2);
        assert_eq!(report.slop.unwrap().labeled, 1);
    }

    #[test]
    fn duplicate_ids_report_the_second_line() {
        let input = concat!(
            r#"{"id":"same","text":"one","spam":true}"#,
            "\n",
            r#"{"id":"same","text":"two","spam":false}"#,
            "\n"
        );
        let error = evaluate_reader(Cursor::new(input), 0.5).unwrap_err();
        assert!(matches!(
            error,
            EvalError::DuplicateId { line: 2, ref id } if id == "same"
        ));
    }

    #[test]
    fn invalid_json_reports_line_number() {
        let error = evaluate_reader(Cursor::new("{not json}\n"), 0.5).unwrap_err();
        assert!(matches!(error, EvalError::Json { line: 1, .. }));
    }

    #[test]
    fn invalid_record_reports_id_and_line() {
        let input = r#"{"id":"unlabeled","text":"hello"}"#;
        let error = evaluate_reader(Cursor::new(input), 0.5).unwrap_err();
        assert!(matches!(
            error,
            EvalError::Record { line: 1, ref id, .. } if id == "unlabeled"
        ));
    }

    #[test]
    fn invalid_threshold_fails_before_evaluation() {
        for threshold in [f64::NAN, -0.1, 1.1] {
            let error = evaluate_reader(Cursor::new(""), threshold).unwrap_err();
            assert!(
                matches!(error, EvalError::InvalidThreshold(value) if value.to_bits() == threshold.to_bits())
            );
        }
    }

    #[test]
    fn empty_corpus_is_rejected() {
        let error = evaluate_reader(Cursor::new("\n"), 0.5).unwrap_err();
        assert!(matches!(error, EvalError::EmptyInput));
    }
}
