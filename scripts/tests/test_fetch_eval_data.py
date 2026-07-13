import csv
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile


SCRIPT = Path(__file__).parents[1] / "fetch_eval_data.py"
SPEC = importlib.util.spec_from_file_location("fetch_eval_data", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
FETCH = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = FETCH
SPEC.loader.exec_module(FETCH)


def archive(files: dict[str, str]) -> bytes:
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as bundle:
        for name, content in files.items():
            bundle.writestr(name, content.encode("utf-8"))
    return output.getvalue()


class ImportTests(unittest.TestCase):
    def test_sms_labels_map_to_normalized_records(self) -> None:
        payload = archive(
            {"SMSSpamCollection": "ham\thello there\nspam\tBUY NOW\n"}
        )
        records = FETCH.parse_uci_sms(payload, "sms")
        self.assertEqual(
            records,
            [
                {
                    "id": "sms:1",
                    "text": "hello there",
                    "context": "other",
                    "spam": False,
                },
                {
                    "id": "sms:2",
                    "text": "BUY NOW",
                    "context": "other",
                    "spam": True,
                },
            ],
        )

    def test_youtube_csv_preserves_quoted_commas_and_newlines(self) -> None:
        content = io.StringIO(newline="")
        writer = csv.DictWriter(
            content,
            fieldnames=["COMMENT_ID", "AUTHOR", "DATE", "CONTENT", "CLASS"],
        )
        writer.writeheader()
        writer.writerow(
            {
                "COMMENT_ID": "abc",
                "AUTHOR": "user",
                "DATE": "2015-01-01",
                "CONTENT": "hello,\nworld",
                "CLASS": "0",
            }
        )
        records = FETCH.parse_uci_youtube(
            archive({"Youtube01-Test.csv": content.getvalue()}), "youtube"
        )
        self.assertEqual(records[0]["id"], "youtube:Youtube01-Test:abc")
        self.assertEqual(records[0]["text"], "hello,\nworld")
        self.assertEqual(records[0]["context"], "comment")
        self.assertFalse(records[0]["spam"])

    def test_youtube_exact_duplicate_is_counted_once(self) -> None:
        content = (
            "COMMENT_ID,AUTHOR,DATE,CONTENT,CLASS\n"
            "same,user,,duplicate text,1\n"
            "same,user,,duplicate text,1\n"
        )
        records = FETCH.parse_uci_youtube(
            archive({"Youtube01-Test.csv": content}), "youtube"
        )
        self.assertEqual(len(records), 1)

    def test_youtube_conflicting_duplicate_is_rejected(self) -> None:
        content = (
            "COMMENT_ID,AUTHOR,DATE,CONTENT,CLASS\n"
            "same,user,,first text,0\n"
            "same,user,,different text,1\n"
        )
        with self.assertRaisesRegex(ValueError, "conflicting duplicate comment id"):
            FETCH.parse_uci_youtube(
                archive({"Youtube01-Test.csv": content}), "youtube"
            )

    def test_invalid_label_is_rejected(self) -> None:
        payload = archive({"SMSSpamCollection": "maybe\thello\n"})
        with self.assertRaisesRegex(ValueError, "unsupported SMS label"):
            FETCH.parse_uci_sms(payload, "sms")

    def test_checksum_mismatch_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
            FETCH.verify_sha256(b"payload", "0" * 64)

    def test_failed_normalization_preserves_existing_destination(self) -> None:
        payload = archive({"SMSSpamCollection": "ham\thello\n"})
        source = {
            "id": "sms",
            "url": "https://example.invalid/sms.zip",
            "sha256": hashlib.sha256(payload).hexdigest(),
            "importer": "uci_sms",
            "expected_records": 2,
        }
        with tempfile.TemporaryDirectory() as temporary:
            output_dir = Path(temporary)
            destination = output_dir / "sms.jsonl"
            destination.write_text("existing\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "expected 2 records, parsed 1"):
                FETCH.normalize_source(source, output_dir, fetcher=lambda _: payload)
            self.assertEqual(destination.read_text(encoding="utf-8"), "existing\n")

    def test_successful_normalization_writes_jsonl_atomically(self) -> None:
        payload = archive({"SMSSpamCollection": "ham\thello\nspam\tbuy\n"})
        source = {
            "id": "sms",
            "url": "https://example.invalid/sms.zip",
            "sha256": hashlib.sha256(payload).hexdigest(),
            "importer": "uci_sms",
            "expected_records": 2,
        }
        with tempfile.TemporaryDirectory() as temporary:
            destination = FETCH.normalize_source(
                source, Path(temporary), fetcher=lambda _: payload
            )
            records = [
                json.loads(line)
                for line in destination.read_text(encoding="utf-8").splitlines()
            ]
            self.assertEqual(len(records), 2)
            self.assertEqual(records[1]["spam"], True)


if __name__ == "__main__":
    unittest.main()
