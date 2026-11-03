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
        self.base = self.root / "herdr-fork-packaging"
        self.commit = "a" * 40
        self.directory = self.base / self.commit
        self.directory.mkdir(parents=True)
        self.binary = self.directory / "herdr.exe"
        self.output = self.directory / "candidate.zip"
        self.binary.write_bytes(b"MZ" + b"test payload")
        self.receipt = {"candidate_commit": self.commit, "result": "source_preflight_only"}
        for patcher in (patch.object(packaging, "preflight", return_value=self.receipt),
                        patch.object(packaging.tempfile, "gettempdir", return_value=str(self.root))):
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_archive_bytes_and_provenance(self):
        manifest = packaging.package("0.9.3-k0.1.0", "b" * 40)
        with zipfile.ZipFile(self.output) as archive:
            self.assertEqual(archive.namelist(), ["herdr.exe", "fork-provenance.json"])
            payload = archive.read("herdr.exe")
            self.assertEqual(payload, self.binary.read_bytes())
            self.assertEqual(json.loads(archive.read("fork-provenance.json")), manifest)
        self.assertEqual(manifest["artifact"]["sha256"], hashlib.sha256(payload).hexdigest())
        self.assertEqual(manifest["artifact"]["size"], len(payload))
        self.assertEqual(manifest["source_preflight"], self.receipt)
        self.assertIn("unverified", manifest["assurance"])

    def test_no_overwrite(self):
        packaging.package("v", "base")
        before = self.output.read_bytes()
        with self.assertRaisesRegex(ValueError, "already exists"):
            packaging.package("v", "base")
        self.assertEqual(self.output.read_bytes(), before)

    def test_reject_invalid_binary(self):
        self.binary.write_bytes(b"not exe")
        with self.assertRaisesRegex(ValueError, "header"):
            packaging.package("v", "base")
        self.assertFalse(self.output.exists())

    def test_preflight_failure_creates_no_archive(self):
        with patch.object(packaging, "preflight", side_effect=ValueError("bad source")):
            with self.assertRaisesRegex(ValueError, "bad source"):
                packaging.package("v", "base")
        self.assertFalse(self.output.exists())

    def test_reject_traversal_commit_and_file_symlink(self):
        with self.assertRaisesRegex(ValueError, "commit"):
            packaging.candidate_directory("../outside")
        other = self.root / "outside.exe"
        other.write_bytes(b"MZoutside")
        self.binary.unlink()
        try:
            self.binary.symlink_to(other)
        except OSError as error:
            self.skipTest(f"symlink unavailable: {error}")
        with self.assertRaisesRegex(ValueError, "non-symlink"):
            packaging.package("v", "base")
        self.assertFalse(self.output.exists())

    def test_reject_candidate_directory_symlink(self):
        outside = self.root / "outside"
        outside.mkdir()
        other_commit = "c" * 40
        try:
            (self.base / other_commit).symlink_to(outside, target_is_directory=True)
        except OSError as error:
            self.skipTest(f"symlink unavailable: {error}")
        with self.assertRaisesRegex(ValueError, "symlink or reparse"):
            packaging.candidate_directory(other_commit)

    def test_cli_does_not_accept_arbitrary_paths(self):
        with patch("sys.argv", ["package_fork_windows.py", "--version", "v", "--upstream-commit", "b", "--binary", str(self.binary)]):
            with self.assertRaises(SystemExit):
                packaging.main()


if __name__ == "__main__":
    unittest.main()
