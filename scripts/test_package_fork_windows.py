import hashlib
import json
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

from scripts import package_fork_windows as packaging


class ForkPackagingTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.binary = self.root / "input.exe"
        self.binary.write_bytes(b"MZ" + b"test payload")
        self.receipt = {"candidate_commit": "a" * 40, "result": "source_preflight_only"}
        self.patcher = patch.object(packaging, "preflight", return_value=self.receipt)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)

    def test_archive_bytes_and_provenance(self):
        output = self.root / "candidate.zip"
        manifest = packaging.package(self.binary, output, "0.9.3-k0.1.0", "b" * 40)
        with zipfile.ZipFile(output) as archive:
            self.assertEqual(archive.namelist(), ["herdr.exe", "fork-provenance.json"])
            payload = archive.read("herdr.exe")
            self.assertEqual(payload, self.binary.read_bytes())
            self.assertEqual(json.loads(archive.read("fork-provenance.json")), manifest)
        self.assertEqual(manifest["artifact"]["sha256"], hashlib.sha256(payload).hexdigest())
        self.assertEqual(manifest["artifact"]["size"], len(payload))
        self.assertEqual(manifest["source_preflight"], self.receipt)
        self.assertIn("unverified", manifest["assurance"])

    def test_deterministic_and_no_overwrite(self):
        first, second = self.root / "a.zip", self.root / "b.zip"
        packaging.package(self.binary, first, "v", "base")
        packaging.package(self.binary, second, "v", "base")
        self.assertEqual(first.read_bytes(), second.read_bytes())
        before = first.read_bytes()
        with self.assertRaisesRegex(ValueError, "already exists"):
            packaging.package(self.binary, first, "v", "base")
        self.assertEqual(first.read_bytes(), before)

    def test_reject_invalid_binary_and_extension(self):
        self.binary.write_bytes(b"not exe")
        with self.assertRaisesRegex(ValueError, "header"):
            packaging.package(self.binary, self.root / "a.zip", "v", "base")
        self.assertFalse((self.root / "a.zip").exists())
        with self.assertRaisesRegex(ValueError, "extension"):
            packaging.package(self.binary, self.root / "a.txt", "v", "base")

    def test_preflight_failure_creates_no_archive(self):
        output = self.root / "a.zip"
        with patch.object(packaging, "preflight", side_effect=ValueError("bad source")):
            with self.assertRaisesRegex(ValueError, "bad source"):
                packaging.package(self.binary, output, "v", "base")
        self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
