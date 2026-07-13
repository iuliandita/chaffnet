use chaffnet_eval::evaluator::{evaluate_path, evaluate_path_baseline};
use chaffnet_eval::export::export_path;
use clap::{Parser, Subcommand, ValueEnum};
use std::error::Error;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(about = "Evaluate chaffnet classifiers against labeled JSONL corpora")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Score a normalized JSONL corpus and emit a JSON metrics report.
    Evaluate {
        /// Path to normalized JSONL evaluation records.
        #[arg(long)]
        input: PathBuf,
        /// Decision threshold used for confusion, precision, recall, and F1.
        #[arg(long, default_value_t = 0.5)]
        threshold: f64,
        /// Classifier used for spam scoring.
        #[arg(long, value_enum, default_value_t = ClassifierChoice::Onnx)]
        classifier: ClassifierChoice,
    },
    /// Export Rust-derived spam training features from normalized evaluation JSONL.
    ExportSpamFeatures {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        output: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ClassifierChoice {
    Onnx,
    Baseline,
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Evaluate {
            input,
            threshold,
            classifier,
        } => {
            let report = match classifier {
                ClassifierChoice::Onnx => evaluate_path(&input, threshold)?,
                ClassifierChoice::Baseline => evaluate_path_baseline(&input, threshold)?,
            };
            let stdout = io::stdout();
            let mut output = stdout.lock();
            serde_json::to_writer_pretty(&mut output, &report)?;
            writeln!(output)?;
        }
        Command::ExportSpamFeatures {
            input,
            source,
            output,
        } => {
            let exported = export_path(&input, &source, &output)?;
            println!("exported {exported} rows to {}", output.display());
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn evaluate_command_defaults_to_neutral_threshold() {
        let cli =
            Cli::try_parse_from(["chaffnet-eval", "evaluate", "--input", "set.jsonl"]).unwrap();
        let Command::Evaluate {
            input,
            threshold,
            classifier,
        } = cli.command
        else {
            panic!("wrong command");
        };
        assert_eq!(input, PathBuf::from("set.jsonl"));
        assert_eq!(threshold, 0.5);
        assert_eq!(classifier, ClassifierChoice::Onnx);
    }

    #[test]
    fn evaluate_command_accepts_baseline_classifier() {
        let cli = Cli::try_parse_from([
            "chaffnet-eval",
            "evaluate",
            "--input",
            "set.jsonl",
            "--classifier",
            "baseline",
        ])
        .unwrap();
        let Command::Evaluate { classifier, .. } = cli.command else {
            panic!("wrong command");
        };
        assert_eq!(classifier, ClassifierChoice::Baseline);
    }

    #[test]
    fn export_command_parses_required_paths_and_source() {
        let cli = Cli::try_parse_from([
            "chaffnet-eval",
            "export-spam-features",
            "--input",
            "input.jsonl",
            "--source",
            "sms",
            "--output",
            "features.jsonl",
        ])
        .unwrap();
        let Command::ExportSpamFeatures {
            input,
            source,
            output,
        } = cli.command
        else {
            panic!("wrong command");
        };
        assert_eq!(input, PathBuf::from("input.jsonl"));
        assert_eq!(source, "sms");
        assert_eq!(output, PathBuf::from("features.jsonl"));
    }
}
