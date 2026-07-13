import importlib.util
import json
from pathlib import Path
import sys
import unittest

import numpy as np


SCRIPT = Path(__file__).parents[1] / "train_spam_model.py"
SPEC = importlib.util.spec_from_file_location("train_spam_model", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
TRAIN = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = TRAIN
SPEC.loader.exec_module(TRAIN)


class TrainingTests(unittest.TestCase):
    def test_hash_bucket_partition_boundaries(self) -> None:
        self.assertEqual(TRAIN.split_bucket("00000000" + "0" * 56), "train")
        self.assertEqual(TRAIN.split_bucket("00000046" + "0" * 56), "validation")
        self.assertEqual(TRAIN.split_bucket("00000055" + "0" * 56), "test")

    def test_loss_gradient_matches_finite_difference(self) -> None:
        features = np.array([[0.1, 0.2], [0.5, -0.4], [1.2, 0.3]], dtype=np.float64)
        offsets = np.array([-0.3, 0.2, 0.1], dtype=np.float64)
        labels = np.array([0.0, 1.0, 1.0], dtype=np.float64)
        params = np.array([0.2, -0.1, 0.05], dtype=np.float64)
        loss, gradient = TRAIN.loss_and_gradient(
            params, features, offsets, labels, 0.1
        )
        self.assertTrue(np.isfinite(loss))
        epsilon = 1e-6
        numeric = np.empty_like(params)
        for index in range(params.size):
            left = params.copy()
            right = params.copy()
            left[index] -= epsilon
            right[index] += epsilon
            numeric[index] = (
                TRAIN.loss_and_gradient(left, features, offsets, labels, 0.1)[0]
                - TRAIN.loss_and_gradient(right, features, offsets, labels, 0.1)[0]
            ) / (-2 * epsilon)
        np.testing.assert_allclose(gradient, numeric, rtol=1e-5, atol=1e-7)

    def test_candidate_selection_uses_brier_then_ap_then_regularization(self) -> None:
        candidates = [
            TRAIN.CandidateResult(0.01, np.zeros(2), 0.0, 0.20, 0.80),
            TRAIN.CandidateResult(0.1, np.zeros(2), 0.0, 0.19, 0.70),
            TRAIN.CandidateResult(1.0, np.zeros(2), 0.0, 0.19, 0.70),
        ]
        self.assertEqual(TRAIN.choose_candidate(candidates).l2, 1.0)

    def test_conflicting_labels_for_one_text_group_are_rejected(self) -> None:
        rows = [
            TRAIN.TrainingRow("one", "a", "0" * 64, False, -1.0, -1.1, [0.0]),
            TRAIN.TrainingRow("two", "b", "0" * 64, True, -1.0, -0.9, [1.0]),
        ]
        with self.assertRaisesRegex(ValueError, "conflicting labels"):
            TRAIN.partition_rows(rows)

    def test_onnx_output_matches_numpy_residual_logits(self) -> None:
        means = np.array([1.0, 2.0], dtype=np.float64)
        scales = np.array([2.0, 4.0], dtype=np.float64)
        weights = np.array([0.5, -0.25], dtype=np.float64)
        intercept = 0.3
        model = TRAIN.build_onnx(means, scales, weights, intercept)
        matrix = np.array([[3.0, 6.0], [1.0, 2.0]], dtype=np.float32)
        actual = TRAIN.run_onnx(model, matrix)
        expected = ((matrix - means) / scales) @ weights + intercept
        np.testing.assert_allclose(actual, expected, rtol=1e-5, atol=1e-6)

    def test_sha256_bytes_is_stable(self) -> None:
        self.assertEqual(
            TRAIN.sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        )

    def test_tracked_model_matches_metadata(self) -> None:
        model_path = Path(__file__).parents[2] / "models" / "spam-residual-v1.onnx"
        metadata_path = Path(__file__).parents[2] / "models" / "spam-residual-v1.json"
        model = model_path.read_bytes()
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
        self.assertEqual(TRAIN.sha256_bytes(model), metadata["model_sha256"])
        self.assertEqual(metadata["feature_count"], len(metadata["feature_names"]))
        self.assertEqual(metadata["input_name"], "features")
        self.assertEqual(metadata["output_name"], "spam_residual_logodds")
        self.assertTrue(metadata["default_eligible"])
        self.assertEqual(len(metadata["parity_vectors"]), 3)
        self.assertEqual(len(metadata["parity_vectors"][0]["features"]), 7)
        self.assertTrue(np.isfinite(metadata["parity_vectors"][0]["residual_logit"]))


if __name__ == "__main__":
    unittest.main()
