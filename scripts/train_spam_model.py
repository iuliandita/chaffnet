#!/usr/bin/env python3
"""Train and export the deterministic chaffnet spam residual model."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import tempfile
from typing import Any, Iterable, Sequence

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper
import onnxruntime
from scipy.optimize import minimize
from scipy.special import expit


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MODEL = ROOT / "models" / "spam-residual-v1.onnx"
DEFAULT_METADATA = ROOT / "models" / "spam-residual-v1.json"
L2_GRID = (0.0, 0.01, 0.1, 1.0, 10.0)


@dataclass(frozen=True)
class TrainingRow:
    id: str
    source: str
    text_group: str
    label: bool
    rule_offset: float
    baseline_logit: float
    features: list[float]
    feature_names: tuple[str, ...] = ()


@dataclass
class CandidateResult:
    l2: float
    weights: np.ndarray
    intercept: float
    validation_brier: float
    validation_average_precision: float
    means: np.ndarray | None = None
    scales: np.ndarray | None = None
    iterations: int = 0


def sha256_bytes(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def split_bucket(text_group: str) -> str:
    if len(text_group) != 64:
        raise ValueError(f"text_group must be a 64-character SHA-256 hex digest: {text_group!r}")
    try:
        bucket = int(text_group[:8], 16) % 100
    except ValueError as error:
        raise ValueError(f"text_group is not hexadecimal: {text_group!r}") from error
    if bucket < 70:
        return "train"
    if bucket < 85:
        return "validation"
    return "test"


def partition_rows(rows: Sequence[TrainingRow]) -> dict[str, list[TrainingRow]]:
    partitions: dict[str, list[TrainingRow]] = {
        "train": [],
        "validation": [],
        "test": [],
    }
    labels_by_group: dict[str, bool] = {}
    for row in rows:
        previous = labels_by_group.setdefault(row.text_group, row.label)
        if previous != row.label:
            raise ValueError(f"conflicting labels for text group {row.text_group}")
        partitions[split_bucket(row.text_group)].append(row)
    for name, partition in partitions.items():
        labels = {row.label for row in partition}
        if labels != {False, True}:
            raise ValueError(f"{name} partition must contain both classes")
    return partitions


def loss_and_gradient(
    params: np.ndarray,
    features: np.ndarray,
    offsets: np.ndarray,
    labels: np.ndarray,
    l2: float,
) -> tuple[float, np.ndarray]:
    weights = params[:-1]
    intercept = params[-1]
    logits = offsets + features @ weights + intercept
    loss = np.mean(np.logaddexp(0.0, logits) - labels * logits)
    loss += 0.5 * l2 * float(weights @ weights)
    errors = (expit(logits) - labels) / labels.size
    gradient = np.empty_like(params)
    gradient[:-1] = features.T @ errors + l2 * weights
    gradient[-1] = np.sum(errors)
    return float(loss), gradient


def average_precision(labels: np.ndarray, probabilities: np.ndarray) -> float | None:
    positives = int(np.sum(labels))
    if positives == 0:
        return None
    order = np.argsort(-probabilities, kind="stable")
    ranked_probabilities = probabilities[order]
    ranked_labels = labels[order]
    seen_positive = 0
    seen_total = 0
    area = 0.0
    index = 0
    while index < ranked_labels.size:
        score = ranked_probabilities[index]
        end = index + 1
        while end < ranked_labels.size and ranked_probabilities[end] == score:
            end += 1
        group_positive = int(np.sum(ranked_labels[index:end]))
        seen_positive += group_positive
        seen_total += end - index
        area += (group_positive / positives) * (seen_positive / seen_total)
        index = end
    return area


def classification_metrics(labels: np.ndarray, logits: np.ndarray) -> dict[str, Any]:
    probabilities = expit(logits)
    predictions = probabilities >= 0.5
    positive = labels.astype(bool)
    true_positive = int(np.sum(predictions & positive))
    false_positive = int(np.sum(predictions & ~positive))
    true_negative = int(np.sum(~predictions & ~positive))
    false_negative = int(np.sum(~predictions & positive))
    precision_denominator = true_positive + false_positive
    recall_denominator = true_positive + false_negative
    precision = true_positive / precision_denominator if precision_denominator else None
    recall = true_positive / recall_denominator if recall_denominator else None
    if precision is None or recall is None:
        f1 = None
    elif precision + recall == 0:
        f1 = 0.0
    else:
        f1 = 2 * precision * recall / (precision + recall)

    ece = 0.0
    for bin_index in range(10):
        lower = bin_index / 10
        upper = (bin_index + 1) / 10
        if bin_index == 9:
            members = (probabilities >= lower) & (probabilities <= upper)
        else:
            members = (probabilities >= lower) & (probabilities < upper)
        count = int(np.sum(members))
        if count:
            ece += count / labels.size * abs(
                float(np.mean(probabilities[members])) - float(np.mean(labels[members]))
            )

    return {
        "records": int(labels.size),
        "positive": int(np.sum(positive)),
        "negative": int(labels.size - np.sum(positive)),
        "brier": float(np.mean((probabilities - labels) ** 2)),
        "expected_calibration_error": float(ece),
        "average_precision": average_precision(labels, probabilities),
        "threshold_0_5": {
            "true_positive": true_positive,
            "false_positive": false_positive,
            "true_negative": true_negative,
            "false_negative": false_negative,
            "precision": precision,
            "recall": recall,
            "f1": f1,
        },
    }


def _arrays(rows: Sequence[TrainingRow]) -> tuple[np.ndarray, ...]:
    features = np.asarray([row.features for row in rows], dtype=np.float64)
    offsets = np.asarray([row.rule_offset for row in rows], dtype=np.float64)
    baseline = np.asarray([row.baseline_logit for row in rows], dtype=np.float64)
    labels = np.asarray([row.label for row in rows], dtype=np.float64)
    return features, offsets, baseline, labels


def fit_candidate(train_rows: Sequence[TrainingRow], l2: float) -> CandidateResult:
    features, offsets, _, labels = _arrays(train_rows)
    means = np.mean(features, axis=0)
    scales = np.std(features, axis=0)
    scales[scales == 0] = 1.0
    normalized = (features - means) / scales
    initial = np.zeros(normalized.shape[1] + 1, dtype=np.float64)
    result = minimize(
        loss_and_gradient,
        initial,
        args=(normalized, offsets, labels, l2),
        method="L-BFGS-B",
        jac=True,
        options={"maxiter": 1000, "ftol": 1e-12, "gtol": 1e-8},
    )
    if not result.success:
        raise RuntimeError(f"optimizer failed for L2={l2}: {result.message}")
    return CandidateResult(
        l2=l2,
        weights=np.asarray(result.x[:-1], dtype=np.float64),
        intercept=float(result.x[-1]),
        validation_brier=float("inf"),
        validation_average_precision=float("-inf"),
        means=means,
        scales=scales,
        iterations=int(result.nit),
    )


def residual_logits(candidate: CandidateResult, features: np.ndarray) -> np.ndarray:
    if candidate.means is None or candidate.scales is None:
        raise ValueError("candidate is missing normalization parameters")
    return (features - candidate.means) / candidate.scales @ candidate.weights + candidate.intercept


def score_candidate(candidate: CandidateResult, rows: Sequence[TrainingRow]) -> dict[str, Any]:
    features, offsets, _, labels = _arrays(rows)
    return classification_metrics(labels, offsets + residual_logits(candidate, features))


def choose_candidate(candidates: Sequence[CandidateResult]) -> CandidateResult:
    if not candidates:
        raise ValueError("at least one candidate is required")
    return min(
        candidates,
        key=lambda candidate: (
            candidate.validation_brier,
            -candidate.validation_average_precision,
            -candidate.l2,
        ),
    )


def build_onnx(
    means: np.ndarray,
    scales: np.ndarray,
    weights: np.ndarray,
    intercept: float,
) -> bytes:
    means = np.asarray(means, dtype=np.float32)
    scales = np.asarray(scales, dtype=np.float32)
    weights = np.asarray(weights, dtype=np.float32).reshape((-1, 1))
    intercept_array = np.asarray([intercept], dtype=np.float32)
    if not (means.shape == scales.shape == (weights.shape[0],)):
        raise ValueError("normalization and weight dimensions do not match")
    if np.any(scales == 0) or not all(
        np.all(np.isfinite(value)) for value in (means, scales, weights, intercept_array)
    ):
        raise ValueError("model parameters must be finite with non-zero scales")

    graph = helper.make_graph(
        [
            helper.make_node("Sub", ["features", "feature_means"], ["centered"]),
            helper.make_node("Div", ["centered", "feature_scales"], ["normalized"]),
            helper.make_node("MatMul", ["normalized", "weights"], ["linear"]),
            helper.make_node(
                "Add", ["linear", "intercept"], ["spam_residual_logodds"]
            ),
        ],
        "chaffnet_spam_residual_v1",
        [
            helper.make_tensor_value_info(
                "features", TensorProto.FLOAT, ["batch", weights.shape[0]]
            )
        ],
        [
            helper.make_tensor_value_info(
                "spam_residual_logodds", TensorProto.FLOAT, ["batch", 1]
            )
        ],
        initializer=[
            numpy_helper.from_array(means, "feature_means"),
            numpy_helper.from_array(scales, "feature_scales"),
            numpy_helper.from_array(weights, "weights"),
            numpy_helper.from_array(intercept_array, "intercept"),
        ],
    )
    model = helper.make_model(
        graph,
        producer_name="chaffnet",
        opset_imports=[helper.make_opsetid("", 17)],
    )
    model.ir_version = 10
    onnx.checker.check_model(model)
    return model.SerializeToString()


def run_onnx(model: bytes, features: np.ndarray) -> np.ndarray:
    session = onnxruntime.InferenceSession(
        model, providers=["CPUExecutionProvider"]
    )
    result = session.run(
        ["spam_residual_logodds"],
        {"features": np.asarray(features, dtype=np.float32)},
    )[0]
    return np.asarray(result, dtype=np.float64).reshape(-1)


def load_rows(paths: Iterable[Path]) -> tuple[list[TrainingRow], tuple[str, ...]]:
    rows: list[TrainingRow] = []
    feature_names: tuple[str, ...] | None = None
    identities: set[tuple[str, str]] = set()
    for path in paths:
        with path.open(encoding="utf-8") as source_file:
            for line_number, line in enumerate(source_file, start=1):
                if not line.strip():
                    continue
                document = json.loads(line)
                names = tuple(document.pop("feature_names"))
                if feature_names is None:
                    feature_names = names
                elif names != feature_names:
                    raise ValueError(f"{path}:{line_number}: feature schema mismatch")
                row = TrainingRow(feature_names=names, **document)
                identity = (row.source, row.id)
                if identity in identities:
                    raise ValueError(f"{path}:{line_number}: duplicate row {identity}")
                identities.add(identity)
                values = [row.rule_offset, row.baseline_logit, *row.features]
                if not all(np.isfinite(value) for value in values):
                    raise ValueError(f"{path}:{line_number}: non-finite numeric value")
                rows.append(row)
    if not rows or feature_names is None:
        raise ValueError("training input contains no rows")
    if any(len(row.features) != len(feature_names) for row in rows):
        raise ValueError("feature vector length does not match feature schema")
    return rows, feature_names


def _counts(rows: Sequence[TrainingRow]) -> dict[str, Any]:
    by_source: dict[str, dict[str, int]] = {}
    for row in rows:
        source = by_source.setdefault(row.source, {"records": 0, "positive": 0})
        source["records"] += 1
        source["positive"] += int(row.label)
    return {
        "records": len(rows),
        "positive": sum(row.label for row in rows),
        "negative": sum(not row.label for row in rows),
        "sources": by_source,
    }


def _atomic_write(path: Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            "wb", dir=path.parent, prefix=f".{path.name}.", delete=False
        ) as output:
            temporary = Path(output.name)
            output.write(payload)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def train(
    inputs: Sequence[Path],
    source_manifest: Path,
    output_model: Path,
    output_metadata: Path,
) -> dict[str, Any]:
    rows, feature_names = load_rows(inputs)
    partitions = partition_rows(rows)
    candidates = []
    for l2 in L2_GRID:
        candidate = fit_candidate(partitions["train"], l2)
        validation = score_candidate(candidate, partitions["validation"])
        candidate.validation_brier = validation["brier"]
        candidate.validation_average_precision = validation["average_precision"]
        candidates.append(candidate)
    selected = choose_candidate(candidates)
    assert selected.means is not None and selected.scales is not None

    model = build_onnx(
        selected.means, selected.scales, selected.weights, selected.intercept
    )
    test_features, test_offsets, baseline_logits, test_labels = _arrays(partitions["test"])
    numpy_residual = residual_logits(selected, test_features)
    onnx_residual = run_onnx(model, test_features.astype(np.float32))
    np.testing.assert_allclose(onnx_residual, numpy_residual, rtol=1e-5, atol=1e-5)
    parity_vectors = [
        {
            "id": row.id,
            "features": test_features[index].astype(np.float32).tolist(),
            "residual_logit": float(onnx_residual[index]),
        }
        for index, row in enumerate(partitions["test"][:3])
    ]

    trained_metrics = classification_metrics(test_labels, test_offsets + onnx_residual)
    baseline_metrics = classification_metrics(test_labels, baseline_logits)
    default_eligible = (
        trained_metrics["brier"] < baseline_metrics["brier"]
        and trained_metrics["average_precision"]
        > baseline_metrics["average_precision"]
    )

    with source_manifest.open(encoding="utf-8") as manifest_file:
        sources = json.load(manifest_file)
    metadata = {
        "model": "spam-residual-v1",
        "model_sha256": sha256_bytes(model),
        "input_name": "features",
        "output_name": "spam_residual_logodds",
        "feature_names": list(feature_names),
        "feature_count": len(feature_names),
        "split_policy": "sha256-prefix-mod-100: train=0..69, validation=70..84, test=85..99",
        "partition_counts": {
            name: _counts(partition) for name, partition in partitions.items()
        },
        "selected_l2": selected.l2,
        "optimizer_iterations": selected.iterations,
        "validation_candidates": [
            {
                "l2": candidate.l2,
                "brier": candidate.validation_brier,
                "average_precision": candidate.validation_average_precision,
                "iterations": candidate.iterations,
            }
            for candidate in candidates
        ],
        "test_metrics": {
            "trained": trained_metrics,
            "baseline": baseline_metrics,
        },
        "default_eligible": default_eligible,
        "parity_vectors": parity_vectors,
        "source_manifest": sources,
        "tool_versions": {
            package: importlib.metadata.version(package)
            for package in ("numpy", "scipy", "onnx", "onnxruntime")
        },
        "training_command": (
            "uv run --group model python scripts/train_spam_model.py "
            + " ".join(f"--input {path}" for path in inputs)
        ),
    }
    _atomic_write(output_model, model)
    _atomic_write(
        output_metadata,
        (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode("utf-8"),
    )
    return metadata


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, action="append", required=True)
    parser.add_argument(
        "--source-manifest", type=Path, default=ROOT / "eval" / "sources.json"
    )
    parser.add_argument("--output-model", type=Path, default=DEFAULT_MODEL)
    parser.add_argument("--output-metadata", type=Path, default=DEFAULT_METADATA)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    metadata = train(
        args.input,
        args.source_manifest,
        args.output_model,
        args.output_metadata,
    )
    print(json.dumps(metadata["test_metrics"], indent=2, sort_keys=True))
    print(f"default_eligible={metadata['default_eligible']}")
    print(f"model_sha256={metadata['model_sha256']}")


if __name__ == "__main__":
    main()
