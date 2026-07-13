#!/usr/bin/env python3
"""Refresh validated, attributed public reputation snapshots."""

from __future__ import annotations

import argparse
from collections.abc import Callable, Iterable
from datetime import UTC, datetime
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
from typing import Any
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "reputation" / "sources.json"
DEFAULT_PROVENANCE = ROOT / "reputation" / "snapshot.json"
USER_AGENT = "chaffnet-reputation/0.1 (+https://github.com/iuliandita/chaffnet)"
MAX_DOWNLOAD_BYTES = 20 * 1024 * 1024
DOMAIN_LABEL = re.compile(r"^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$")


def fetch_bytes(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        payload = response.read(MAX_DOWNLOAD_BYTES + 1)
    if len(payload) > MAX_DOWNLOAD_BYTES:
        raise ValueError(f"download exceeds {MAX_DOWNLOAD_BYTES} bytes: {url}")
    return payload


def _normalize_domain(value: str) -> str | None:
    domain = value.strip().rstrip(".").lower()
    if not domain:
        return None
    try:
        domain = domain.encode("idna").decode("ascii")
    except UnicodeError:
        return None
    labels = domain.split(".")
    if len(domain) > 253 or len(labels) < 2:
        return None
    if any(not DOMAIN_LABEL.fullmatch(label) for label in labels):
        return None
    return domain


def parse_domains(payload: bytes, source_id: str) -> list[str]:
    try:
        text = payload.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError(f"{source_id}: source is not valid UTF-8") from error
    domains: set[str] = set()
    for line_number, line in enumerate(text.splitlines(), start=1):
        value = line.strip()
        if not value or value.startswith("#"):
            continue
        domain = _normalize_domain(value)
        if domain is None:
            raise ValueError(f"{source_id}:{line_number}: invalid domain {value!r}")
        domains.add(domain)
    return sorted(domains)


IMPORTERS: dict[str, Callable[[bytes, str], list[str]]] = {
    "domains": parse_domains,
}


def _relative_output(root: Path, value: object) -> Path:
    relative = Path(str(value))
    if relative.is_absolute() or ".." in relative.parts or relative == Path("."):
        raise ValueError(f"output must be a relative path within the repository: {value!r}")
    return root / relative


def _write_atomic(destination: Path, content: str) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            "w",
            encoding="utf-8",
            dir=destination.parent,
            prefix=f".{destination.name}.",
            suffix=".tmp",
            delete=False,
        ) as output:
            temporary = Path(output.name)
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, destination)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def refresh_sources(
    sources: Iterable[dict[str, Any]],
    root: Path,
    provenance_path: Path,
    *,
    fetcher: Callable[[str], bytes] = fetch_bytes,
    retrieved_at: str | None = None,
) -> dict[str, int]:
    prepared: list[tuple[Path, str, dict[str, Any]]] = []
    source_ids: set[str] = set()
    destinations: set[Path] = set()

    for source in sources:
        source_id = str(source["id"])
        if source_id in source_ids:
            raise ValueError(f"duplicate source id: {source_id}")
        source_ids.add(source_id)
        destination = _relative_output(root, source["output"])
        if destination in destinations:
            raise ValueError(f"duplicate output: {source['output']}")
        destinations.add(destination)

        payload = fetcher(str(source["url"]))
        importer_name = str(source["importer"])
        try:
            importer = IMPORTERS[importer_name]
        except KeyError as error:
            raise ValueError(
                f"{source_id}: unsupported importer {importer_name!r}"
            ) from error
        records = importer(payload, source_id)
        minimum = int(source["minimum_records"])
        maximum = int(source["maximum_records"])
        if minimum < 0 or maximum < minimum:
            raise ValueError(f"{source_id}: invalid record count gate {minimum}..{maximum}")
        if not minimum <= len(records) <= maximum:
            raise ValueError(
                f"{source_id}: expected {minimum}..{maximum} records, parsed {len(records)}"
            )
        license_name = str(source["license"]).strip()
        license_url = str(source["license_url"]).strip()
        if not license_name or not license_url.startswith("https://"):
            raise ValueError(f"{source_id}: license and HTTPS license_url are required")
        prepared.append(
            (
                destination,
                "".join(f"{record}\n" for record in records),
                {
                    "id": source_id,
                    "url": str(source["url"]),
                    "license": license_name,
                    "license_url": license_url,
                    "output": str(source["output"]),
                    "records": len(records),
                    "sha256": hashlib.sha256(payload).hexdigest(),
                },
            )
        )

    if not prepared:
        raise ValueError("expected at least one reputation source")

    timestamp = retrieved_at or datetime.now(UTC).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )
    provenance = {
        "retrieved_at": timestamp,
        "sources": [metadata for _, _, metadata in prepared],
    }
    for destination, content, _ in prepared:
        _write_atomic(destination, content)
    _write_atomic(
        provenance_path,
        json.dumps(provenance, indent=2, sort_keys=True, ensure_ascii=True) + "\n",
    )
    return {metadata["id"]: metadata["records"] for _, _, metadata in prepared}


def load_sources(manifest: Path) -> list[dict[str, Any]]:
    with manifest.open(encoding="utf-8") as source_file:
        document = json.load(source_file)
    sources = document.get("sources")
    if not isinstance(sources, list) or not sources:
        raise ValueError(f"{manifest}: expected a non-empty sources array")
    return sources


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--provenance", type=Path, default=DEFAULT_PROVENANCE)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    result = refresh_sources(load_sources(args.manifest), ROOT, args.provenance)
    for source_id, records in result.items():
        print(f"{source_id}: {records} records")


if __name__ == "__main__":
    main()
