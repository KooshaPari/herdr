import unittest
from unittest.mock import patch

from scripts import fork_release_preflight as preflight


BASE = "a" * 40
HEAD = "b" * 40


class ForkPreflightTest(unittest.TestCase):
    def git(self, *args):
        if args[0] == "status":
            return ""
        if args[0] == "ls-remote":
            return f"{BASE}\trefs/tags/v0.9.3"
        if args[0] == "rev-parse":
            return HEAD if args[-1] == "HEAD^{commit}" else BASE
        if args[0] == "show" and args[-1].endswith("Cargo.toml"):
            return '[package]\nversion = "0.9.3-k0.1.0"\n'
        if args[0] == "show" and args[-1].endswith("Cargo.lock"):
            return '[[package]]\nname = "herdr"\nversion = "0.9.3-k0.1.0"\n'
        self.fail(str(args))

    def run_preflight(self, version="0.9.3-k0.1.0", base=BASE):
        with patch.object(preflight, "git", side_effect=self.git), patch.object(preflight.subprocess, "run") as run:
            run.return_value.returncode = 0
            return preflight.preflight(version, base)

    def test_valid_fork_receipt(self):
        result = self.run_preflight()
        self.assertEqual(result["fork"], "KooshaPari/herdr")
        self.assertEqual(result["upstream_commit"], BASE)
        self.assertEqual(result["candidate_commit"], HEAD)
        self.assertEqual(result["result"], "source_preflight_only")

    def test_rejects_invalid_versions(self):
        for value in ("0.9.3", "0.9.3-p0.1.0", "0.9.3-k01.1.0", "0.9.3-k0.1.0+build"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                self.run_preflight(value)

    def test_rejects_wrong_remote_base_and_short_sha(self):
        for base in ("c" * 40, "abc"):
            with self.subTest(base=base), self.assertRaises(ValueError):
                self.run_preflight(base=base)

    def test_rejects_dirty_tree_and_non_descendant(self):
        with patch.object(preflight, "git", return_value="dirty"), self.assertRaisesRegex(ValueError, "clean"):
            preflight.preflight("0.9.3-k0.1.0", BASE)
        with patch.object(preflight, "git", side_effect=self.git), patch.object(preflight.subprocess, "run") as run:
            run.return_value.returncode = 1
            with self.assertRaisesRegex(ValueError, "descend"):
                preflight.preflight("0.9.3-k0.1.0", BASE)

    def test_rejects_manifest_or_lock_mismatch(self):
        with self.assertRaisesRegex(ValueError, "Cargo.toml and Cargo.lock"):
            self.run_preflight("0.9.3-k0.2.0")


if __name__ == "__main__":
    unittest.main()
