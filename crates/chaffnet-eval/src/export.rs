use crate::record::{EvalRecord, RecordError};
use chaffnet_core::classifier::{BaselineClassifier, Classifier, ClassifierError};
use chaffnet_core::config::EngineConfig;
use chaffnet_core::features::Features;
use chaffnet_core::model_features::{
    spam_model_features, spam_rule_offset, SPAM_MODEL_FEATURE_NAMES,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
pub struct ExportRow {
    pub id: String,
    pub source: String,
    pub text_group: String,
    pub label: bool,
    pub rule_offset: f32,
    pub baseline_logit: f32,
    pub feature_names: Vec<String>,
    pub features: [f32; 7],
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("source must not be empty")]
    EmptySource,
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
    #[error("corpus contains no spam labels")]
    NoSpamLabels,
    #[error("failed to write training features: {0}")]
    Write(#[source] std::io::Error),
    #[error("failed to serialize training features: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to open {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to create temporary output beside {path}: {source}")]
    CreateOutput {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to persist output to {path}: {source}")]
    Persist {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Classifier(#[from] ClassifierError),
}

fn text_group(text: &str) -> String {
    let normalized = text
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

pub fn export_reader<R: BufRead, W: Write>(
    reader: R,
    source: &str,
    mut writer: W,
) -> Result<usize, ExportError> {
    let source = source.trim();
    if source.is_empty() {
        return Err(ExportError::EmptySource);
    }

    let config = EngineConfig::default();
    let mut ids = HashSet::new();
    let mut exported = 0usize;
    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.map_err(|source| ExportError::Read {
            line: line_number,
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let record: EvalRecord =
            serde_json::from_str(&line).map_err(|source| ExportError::Json {
                line: line_number,
                source,
            })?;
        record.validate().map_err(|source| ExportError::Record {
            line: line_number,
            id: record.id.clone(),
            source,
        })?;
        if !ids.insert(record.id.clone()) {
            return Err(ExportError::DuplicateId {
                line: line_number,
                id: record.id,
            });
        }
        let Some(label) = record.spam else {
            continue;
        };
        let group = text_group(&record.text);
        let id = record.id.clone();
        let features = Features::extract(&record.into_content());
        let baseline = BaselineClassifier::default().classify(&features)?;
        let rule_offset = spam_rule_offset(&features, &config);
        let row = ExportRow {
            id,
            source: source.to_string(),
            text_group: group,
            label,
            rule_offset,
            baseline_logit: rule_offset + config.classifier_spam_weight * baseline.spam_logodds,
            feature_names: SPAM_MODEL_FEATURE_NAMES
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            features: spam_model_features(&features),
        };
        serde_json::to_writer(&mut writer, &row).map_err(ExportError::Serialize)?;
        writer.write_all(b"\n").map_err(ExportError::Write)?;
        exported += 1;
    }
    if exported == 0 {
        return Err(ExportError::NoSpamLabels);
    }
    Ok(exported)
}

pub fn export_path(input: &Path, source: &str, output: &Path) -> Result<usize, ExportError> {
    let input_file = File::open(input).map_err(|error| ExportError::Open {
        path: input.to_path_buf(),
        source: error,
    })?;
    let output_parent = output.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(output_parent).map_err(|error| {
        ExportError::CreateOutput {
            path: output.to_path_buf(),
            source: error,
        }
    })?;
    let exported = export_reader(BufReader::new(input_file), source, temporary.as_file_mut())?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(ExportError::Write)?;
    temporary
        .persist(output)
        .map_err(|error| ExportError::Persist {
            path: output.to_path_buf(),
            source: error.error,
        })?;
    Ok(exported)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chaffnet_core::classifier::{BaselineClassifier, Classifier};
    use chaffnet_core::config::EngineConfig;
    use chaffnet_core::content::{Content, ContentContext};
    use chaffnet_core::features::Features;
    use chaffnet_core::model_features::{
        spam_model_features, spam_rule_offset, SPAM_MODEL_FEATURE_NAMES,
    };
    use std::io::Cursor;

    #[test]
    fn exports_spam_labeled_rows_with_rust_features() {
        let input = concat!(
            r#"{"id":"one","text":"BUY now https://a.test","context":"comment","spam":true}"#,
            "\n",
            r#"{"id":"slop-only","text":"generic filler","slop":true}"#,
            "\n"
        );
        let mut output = Vec::new();
        let count = export_reader(Cursor::new(input), "fixture", &mut output).unwrap();
        assert_eq!(count, 1);
        let row: ExportRow = serde_json::from_slice(&output).unwrap();
        assert_eq!(row.id, "one");
        assert_eq!(row.source, "fixture");
        assert!(row.label);
        assert_eq!(row.text_group.len(), 64);
        assert_eq!(row.feature_names, SPAM_MODEL_FEATURE_NAMES);

        let features = Features::extract(&Content::new(
            "BUY now https://a.test",
            ContentContext::Comment,
        ));
        for (actual, expected) in row
            .features
            .iter()
            .zip(spam_model_features(&features).iter())
        {
            assert!((actual - expected).abs() < 1e-6);
        }
        assert_eq!(
            row.rule_offset,
            spam_rule_offset(&features, &EngineConfig::default())
        );
        let baseline = BaselineClassifier::default().classify(&features).unwrap();
        assert_eq!(row.baseline_logit, row.rule_offset + baseline.spam_logodds);
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let input = concat!(
            r#"{"id":"same","text":"one","spam":true}"#,
            "\n",
            r#"{"id":"same","text":"two","spam":false}"#,
            "\n"
        );
        let error = export_reader(Cursor::new(input), "fixture", Vec::new()).unwrap_err();
        assert!(matches!(
            error,
            ExportError::DuplicateId { line: 2, ref id } if id == "same"
        ));
    }

    #[test]
    fn corpus_without_spam_labels_is_rejected() {
        let input = r#"{"id":"slop-only","text":"hello","slop":false}"#;
        let error = export_reader(Cursor::new(input), "fixture", Vec::new()).unwrap_err();
        assert!(matches!(error, ExportError::NoSpamLabels));
    }

    #[test]
    fn source_must_not_be_empty() {
        let error = export_reader(Cursor::new(""), " ", Vec::new()).unwrap_err();
        assert!(matches!(error, ExportError::EmptySource));
    }
}
