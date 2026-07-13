import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).parents[1] / "fetch_reputation_data.py"
SPEC = importlib.util.spec_from_file_location("fetch_reputation_data", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
FETCH = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = FETCH
SPEC.loader.exec_module(FETCH)


def source(**overrides: object) -> dict[str, object]:
    value: dict[str, object] = {
        "id": "disposable-domains",
        "url": "https://example.invalid/domains.txt",
        "license": "CC0-1.0",
        "license_url": "https://example.invalid/license",
        "importer": "domains",
        "output": "data/domains.txt",
        "minimum_records": 2,
        "maximum_records": 10,
    }
    value.update(overrides)
    return value


class ReputationImportTests(unittest.TestCase):
    def test_domains_are_idna_normalized_deduplicated_and_sorted(self) -> None:
        records = FETCH.parse_domains(
            b"Example.COM\n# comment\nb\xc3\xbccher.example\nexample.com\n",
            "domains",
        )
        self.assertEqual(records, ["example.com", "xn--bcher-kva.example"])

    def test_invalid_domain_is_rejected_with_line_number(self) -> None:
        with self.assertRaisesRegex(ValueError, r"domains:2: invalid domain"):
            FETCH.parse_domains(b"valid.example\nnot_a_domain\n", "domains")

    def test_record_count_gate_preserves_existing_snapshot(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            destination = root / "data" / "domains.txt"
            destination.parent.mkdir()
            destination.write_text("existing.example\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "expected 2..10 records, parsed 1"):
                FETCH.refresh_sources(
                    [source()],
                    root,
                    root / "snapshot.json",
                    fetcher=lambda _: b"only.example\n",
                    retrieved_at="2026-07-13T00:00:00Z",
                )
            self.assertEqual(destination.read_text(encoding="utf-8"), "existing.example\n")
            self.assertFalse((root / "snapshot.json").exists())

    def test_refresh_writes_snapshot_and_provenance(self) -> None:
        payload = b"second.example\nfirst.example\n"
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            provenance_path = root / "snapshot.json"
            result = FETCH.refresh_sources(
                [source()],
                root,
                provenance_path,
                fetcher=lambda _: payload,
                retrieved_at="2026-07-13T00:00:00Z",
            )
            self.assertEqual(result, {"disposable-domains": 2})
            self.assertEqual(
                (root / "data" / "domains.txt").read_text(encoding="utf-8"),
                "first.example\nsecond.example\n",
            )
            provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
            self.assertEqual(provenance["retrieved_at"], "2026-07-13T00:00:00Z")
            self.assertEqual(provenance["sources"][0]["records"], 2)
            self.assertEqual(
                provenance["sources"][0]["sha256"],
                hashlib.sha256(payload).hexdigest(),
            )
            self.assertEqual(provenance["sources"][0]["license"], "CC0-1.0")

    def test_output_must_stay_within_repository_root(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(ValueError, "output must be a relative path"):
                FETCH.refresh_sources(
                    [source(output="../outside.txt")],
                    root,
                    root / "snapshot.json",
                    fetcher=lambda _: b"first.example\nsecond.example\n",
                    retrieved_at="2026-07-13T00:00:00Z",
                )

    def test_duplicate_output_targets_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(ValueError, "duplicate output"):
                FETCH.refresh_sources(
                    [source(id="one"), source(id="two")],
                    root,
                    root / "snapshot.json",
                    fetcher=lambda _: b"first.example\nsecond.example\n",
                    retrieved_at="2026-07-13T00:00:00Z",
                )


if __name__ == "__main__":
    unittest.main()
