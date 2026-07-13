use std::fmt;
use std::sync::Mutex;

use ort::session::Session;
use ort::value::{TensorElementType, TensorRef, ValueType};

use crate::classifier::{BaselineClassifier, Classifier, ClassifierError, ClassifierOutput};
use crate::features::Features;
use crate::model_features::spam_model_features;

const BUNDLED_MODEL: &[u8] = include_bytes!("../../../models/spam-residual-v1.onnx");
const INPUT_NAME: &str = "features";
const OUTPUT_NAME: &str = "spam_residual_logodds";
const FEATURE_COUNT: i64 = 7;

pub struct OnnxClassifier {
    session: Mutex<Session>,
    slop_classifier: BaselineClassifier,
}

impl fmt::Debug for OnnxClassifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OnnxClassifier")
            .finish_non_exhaustive()
    }
}

impl OnnxClassifier {
    pub fn bundled() -> Result<Self, ClassifierError> {
        Self::from_bytes(BUNDLED_MODEL)
    }

    pub fn from_bytes(model: &[u8]) -> Result<Self, ClassifierError> {
        let session = Session::builder()
            .and_then(|mut builder| builder.commit_from_memory(model))
            .map_err(|error| ClassifierError::Initialization(error.to_string()))?;
        validate_model_contract(&session)?;
        Ok(Self {
            session: Mutex::new(session),
            slop_classifier: BaselineClassifier::default(),
        })
    }

    pub fn infer_spam_residual(
        &self,
        features: &[f32; FEATURE_COUNT as usize],
    ) -> Result<f32, ClassifierError> {
        let input = TensorRef::from_array_view(([1usize, FEATURE_COUNT as usize], &features[..]))
            .map_err(|error| ClassifierError::Inference(error.to_string()))?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| ClassifierError::Inference("ONNX session lock poisoned".into()))?;
        let outputs = session
            .run(ort::inputs![INPUT_NAME => input])
            .map_err(|error| ClassifierError::Inference(error.to_string()))?;
        let output = outputs.get(OUTPUT_NAME).ok_or_else(|| {
            ClassifierError::Inference(format!("model did not return {OUTPUT_NAME}"))
        })?;
        let (shape, values) = output
            .try_extract_tensor::<f32>()
            .map_err(|error| ClassifierError::Inference(error.to_string()))?;
        if shape.as_ref() != [1, 1] || values.len() != 1 || !values[0].is_finite() {
            return Err(ClassifierError::Inference(format!(
                "invalid {OUTPUT_NAME} tensor: shape={shape:?}, values={values:?}"
            )));
        }
        Ok(values[0])
    }
}

impl Classifier for OnnxClassifier {
    fn classify(&self, features: &Features) -> Result<ClassifierOutput, ClassifierError> {
        let spam_logodds = self.infer_spam_residual(&spam_model_features(features))?;
        let slop_logodds = self.slop_classifier.classify(features)?.slop_logodds;
        Ok(ClassifierOutput {
            spam_logodds,
            slop_logodds,
        })
    }
}

fn validate_model_contract(session: &Session) -> Result<(), ClassifierError> {
    if session.inputs().len() != 1 || session.outputs().len() != 1 {
        return Err(ClassifierError::Initialization(
            "model must have exactly one input and one output".into(),
        ));
    }
    validate_tensor(
        "input",
        session.inputs()[0].name(),
        session.inputs()[0].dtype(),
        INPUT_NAME,
        FEATURE_COUNT,
    )?;
    validate_tensor(
        "output",
        session.outputs()[0].name(),
        session.outputs()[0].dtype(),
        OUTPUT_NAME,
        1,
    )
}

fn validate_tensor(
    kind: &str,
    actual_name: &str,
    value_type: &ValueType,
    expected_name: &str,
    width: i64,
) -> Result<(), ClassifierError> {
    let ValueType::Tensor { ty, shape, .. } = value_type else {
        return Err(ClassifierError::Initialization(format!(
            "model {kind} {actual_name} is not a tensor"
        )));
    };
    let valid_batch = matches!(shape.first(), Some(-1 | 1));
    if actual_name != expected_name
        || *ty != TensorElementType::Float32
        || shape.len() != 2
        || !valid_batch
        || shape[1] != width
    {
        return Err(ClassifierError::Initialization(format!(
            "invalid model {kind}: name={actual_name}, type={ty:?}, shape={shape:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::{BaselineClassifier, Classifier, ClassifierError};
    use crate::content::{Content, ContentContext};
    use crate::features::Features;

    #[test]
    fn invalid_model_bytes_fail_initialization() {
        let error = OnnxClassifier::from_bytes(b"not an onnx model").unwrap_err();
        assert!(matches!(error, ClassifierError::Initialization(_)));
    }

    #[test]
    fn bundled_classifier_returns_finite_spam_and_baseline_slop() {
        let classifier = OnnxClassifier::bundled().unwrap();
        let features = Features::extract(&Content::new(
            "A specific note about retry behavior",
            ContentContext::Comment,
        ));
        let actual = classifier.classify(&features).unwrap();
        let baseline = BaselineClassifier::default().classify(&features).unwrap();
        assert!(actual.spam_logodds.is_finite());
        assert_eq!(actual.slop_logodds, baseline.slop_logodds);
    }

    #[test]
    fn bundled_runtime_matches_python_parity_vectors() {
        let classifier = OnnxClassifier::bundled().unwrap();
        let fixtures = [
            (
                [2.8332133, 0.0, 0.0, 0.033333335, 0.9375, 4.049146, 3.875],
                -0.43769315,
            ),
            ([2.1972246, 0.0, 0.0, 1.0, 1.0, 3.7803948, 3.5], -0.98900414),
            (
                [1.609438, 0.0, 0.0, 0.05882353, 1.0, 3.9361815, 5.75],
                0.15343103,
            ),
        ];
        for (features, expected) in fixtures {
            let actual = classifier.infer_spam_residual(&features).unwrap();
            assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
        }
    }
}
