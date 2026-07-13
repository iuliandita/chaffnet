#!/usr/bin/env python3
"""Download, verify, and normalize public chaffnet evaluation corpora."""

from __future__ import annotations

import argparse
import csv
import hashlib
import hmac
import io
import json
import os
from pathlib import Path
import tempfile
from typing import Any, Callable, Iterable
import urllib.request
import zipfile


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "eval" / "sources.json"
DEFAULT_OUTPUT_DIR = ROOT / "data" / "eval"
USER_AGENT = "chaffnet-eval/0.1 (+https://github.com/iuliandita/chaffnet)"


def fetch_bytes(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def verify_sha256(payload: bytes, expected: str) -> None:
    actual = hashlib.sha256(payload).hexdigest()
    if not hmac.compare_digest(actual, expected.lower()):
        raise ValueError(f"SHA-256 mismatch: expected {expected}, got {actual}")


def _record(source_id: str, item_id: str, text: str, context: str, spam: bool) -> dict[str, Any]:
    if not item_id.strip():
        raise ValueError(f"{source_id}: record id must not be empty")
    if not text.strip():
        raise ValueError(f"{source_id}:{item_id}: text must not be empty")
    return {
        "id": f"{source_id}:{item_id}",
        "text": text,
        "context": context,
        "spam": spam,
    }


def parse_uci_sms(payload: bytes, source_id: str) -> list[dict[str, Any]]:
    with zipfile.ZipFile(io.BytesIO(payload)) as bundle:
        raw = bundle.read("SMSSpamCollection").decode("utf-8")

    records = []
    for line_number, line in enumerate(raw.splitlines(), start=1):
        if not line:
            continue
        label, separator, text = line.partition("\t")
        if not separator:
            raise ValueError(f"{source_id}:{line_number}: missing tab separator")
        if label == "ham":
            spam = False
        elif label == "spam":
            spam = True
        else:
            raise ValueError(f"{source_id}:{line_number}: unsupported SMS label {label!r}")
        records.append(_record(source_id, str(line_number), text, "other", spam))
    return records


def parse_uci_youtube(payload: bytes, source_id: str) -> list[dict[str, Any]]:
    records = []
    seen: dict[str, dict[str, Any]] = {}
    with zipfile.ZipFile(io.BytesIO(payload)) as bundle:
        members = sorted(
            name
            for name in bundle.namelist()
            if name.startswith("Youtube") and name.endswith(".csv")
        )
        if not members:
            raise ValueError(f"{source_id}: archive contains no YouTube CSV files")
        for member in members:
            text = bundle.read(member).decode("utf-8-sig")
            reader = csv.DictReader(io.StringIO(text, newline=""))
            required = {"COMMENT_ID", "CONTENT", "CLASS"}
            if reader.fieldnames is None or not required.issubset(reader.fieldnames):
                raise ValueError(f"{source_id}:{member}: missing required CSV columns")
            stem = Path(member).stem
            for row_number, row in enumerate(reader, start=2):
                label = row["CLASS"]
                if label == "0":
                    spam = False
                elif label == "1":
                    spam = True
                else:
                    raise ValueError(
                        f"{source_id}:{member}:{row_number}: unsupported YouTube label {label!r}"
                    )
                item_id = f"{stem}:{row['COMMENT_ID']}"
                record = _record(source_id, item_id, row["CONTENT"], "comment", spam)
                normalized_id = record["id"]
                if normalized_id in seen:
                    if seen[normalized_id] != record:
                        raise ValueError(
                            f"{source_id}:{member}:{row_number}: "
                            f"conflicting duplicate comment id {row['COMMENT_ID']!r}"
                        )
                    continue
                seen[normalized_id] = record
                records.append(record)
    return records


IMPORTERS: dict[str, Callable[[bytes, str], list[dict[str, Any]]]] = {
    "uci_sms": parse_uci_sms,
    "uci_youtube": parse_uci_youtube,
}


def normalize_source(
    source: dict[str, Any],
    output_dir: Path,
    *,
    fetcher: Callable[[str], bytes] = fetch_bytes,
) -> Path:
    source_id = str(source["id"])
    payload = fetcher(str(source["url"]))
    verify_sha256(payload, str(source["sha256"]))
    importer_name = str(source["importer"])
    try:
        importer = IMPORTERS[importer_name]
    except KeyError as error:
        raise ValueError(f"{source_id}: unsupported importer {importer_name!r}") from error
    records = importer(payload, source_id)
    expected_records = int(source["expected_records"])
    if len(records) != expected_records:
        raise ValueError(
            f"{source_id}: expected {expected_records} records, parsed {len(records)}"
        )

    output_dir.mkdir(parents=True, exist_ok=True)
    destination = output_dir / f"{source_id}.jsonl"
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            "w",
            encoding="utf-8",
            dir=output_dir,
            prefix=f".{source_id}.",
            suffix=".tmp",
            delete=False,
        ) as output:
            temporary = Path(output.name)
            for record in records:
                json.dump(record, output, ensure_ascii=False, separators=(",", ":"))
                output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, destination)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return destination


def load_sources(manifest: Path) -> list[dict[str, Any]]:
    with manifest.open(encoding="utf-8") as source_file:
        document = json.load(source_file)
    sources = document.get("sources")
    if not isinstance(sources, list) or not sources:
        raise ValueError(f"{manifest}: expected a non-empty sources array")
    return sources


def selected_sources(
    sources: Iterable[dict[str, Any]], requested: set[str]
) -> list[dict[str, Any]]:
    available = {str(source["id"]): source for source in sources}
    missing = requested - available.keys()
    if missing:
        raise ValueError(f"unknown source id(s): {', '.join(sorted(missing))}")
    if not requested:
        return list(available.values())
    return [available[source_id] for source_id in sorted(requested)]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR)
    parser.add_argument("--source", action="append", default=[])
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    sources = selected_sources(load_sources(args.manifest), set(args.source))
    for source in sources:
        destination = normalize_source(source, args.output_dir)
        print(
            f"{source['id']}: {source['expected_records']} records -> "
            f"{destination.relative_to(ROOT) if destination.is_relative_to(ROOT) else destination}"
        )


if __name__ == "__main__":
    main()
