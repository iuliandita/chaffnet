//! chaffnet-core: hybrid spam + AI-slop classification engine.
//!
//! Extracts features from [`content::Content`], runs independent signal layers
//! (rules, reputation, classifier), and folds them into an [`assessment::Assessment`].

pub mod assessment;
pub mod classifier;
pub mod classifier_onnx;
pub mod config;
pub mod content;
pub mod engine;
pub mod features;
pub mod fingerprint;
pub mod model_features;
pub mod reputation;
pub mod reputation_hosted;
pub mod reputation_local;
pub mod rules;

pub use assessment::{Assessment, ReasonCode};
pub use content::{Content, ContentContext};
pub use engine::Engine;
