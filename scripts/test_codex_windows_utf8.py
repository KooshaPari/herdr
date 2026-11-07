import json
import os
import pathlib
import subprocess
import tempfile
import time
import unittest


@unittest.skipUnless(os.name == "nt", "Windows PowerShell integration")
class CodexWindowsUtf8Test(unittest.TestCase):
    def test_report_failure_is_private_and_fail_open(self):
        root = pathlib.Path(__file__).resolve().parents[1]
        hook = root / "src/integration/assets/codex/herdr-agent-state.ps1"
        state = pathlib.Path(tempfile.mkdtemp(prefix="herdr-codex-report-"))
        helper = state / "fake-herdr.cmd"
        payload = {"hook_event_name": "SessionStart", "session_id": "private-session", "transcript_path": "private-transcript"}
        env = os.environ.copy()
        env.update({"HERDR_ENV": "1", "HERDR_PANE_ID": "private-pane", "HERDR_BIN_PATH": str(helper), "CODEX_THREAD_ID": payload["session_id"]})
        for code in (0, 7):
            helper.write_bytes(f"@echo private-stdout\r\n@echo private-stderr 1>&2\r\n@exit /b {code}\r\n".encode())
            result = subprocess.run(["powershell", "-NoProfile", "-File", str(hook), "session"], input=json.dumps(payload).encode(), capture_output=True, env=env, timeout=20)
            self.assertEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b"")
            self.assertEqual(result.stderr.strip(), b"" if code == 0 else b"Herdr: session report failed.")

    def test_unicode_and_bom_preserve_session_and_millisecond_sequence(self):
        root = pathlib.Path(__file__).resolve().parents[1]
        hook = root / "src/integration/assets/codex/herdr-agent-state.ps1"
        state = pathlib.Path(tempfile.mkdtemp(prefix="herdr-codex-utf8-"))
        helper = state / "fake-herdr.cmd"
        capture = state / "capture.ps1"
        capture.write_text('param([Parameter(ValueFromRemainingArguments=$true)][string[]]$Rest)\n[IO.File]::WriteAllText($env:WITNESS_MARKER, (ConvertTo-Json -Compress -InputObject @($Rest)), [Text.Encoding]::UTF8)\n', encoding="utf8")
        helper.write_bytes(b'@echo off\r\npowershell -NoProfile -File "%WITNESS_HELPER%" %*\r\n')
        payload = {"hook_event_name": "SessionStart", "session_id": "session-\ud55c\uae00", "transcript_path": "synthetic"}
        for codepage in (437, 949):
            for bom in (False, True):
                marker = state / (str(codepage) + "-" + str(bom) + ".json")
                env = os.environ.copy()
                env.update({"HERDR_ENV": "1", "HERDR_PANE_ID": "synthetic-pane", "HERDR_BIN_PATH": str(helper), "CODEX_THREAD_ID": payload["session_id"], "WITNESS_MARKER": str(marker), "WITNESS_HELPER": str(capture)})
                command = "[Console]::InputEncoding=[Text.Encoding]::GetEncoding(" + str(codepage) + "); & '" + str(hook).replace("'", "''") + "' session"
                before = int(time.time() * 1000)
                data = json.dumps(payload, ensure_ascii=False).encode("utf8")
                result = subprocess.run(["powershell", "-NoProfile", "-Command", command], input=(b"\xef\xbb\xbf" if bom else b"") + data, capture_output=True, env=env, timeout=20)
                after = int(time.time() * 1000)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(marker.is_file(), "synthetic report was not dispatched")
                args = json.loads(marker.read_text(encoding="utf-8-sig"))
                self.assertIn("report-agent-session", args)
                self.assertEqual(args[args.index("--source") + 1], "herdr:codex")
                self.assertEqual(args[args.index("--agent-session-id") + 1], payload["session_id"])
                seq = int(args[args.index("--seq") + 1])
                self.assertLessEqual(before, seq)
                self.assertLessEqual(seq, after)

    def test_invalid_and_mismatched_input_do_not_dispatch(self):
        root = pathlib.Path(__file__).resolve().parents[1]
        hook = root / "src/integration/assets/codex/herdr-agent-state.ps1"
        state = pathlib.Path(tempfile.mkdtemp(prefix="herdr-codex-invalid-"))
        helper = state / "fake-herdr.cmd"
        helper.write_bytes(b'@echo off\r\necho called>"%WITNESS_MARKER%"\r\n')
        for name, data in (("invalid", b"{"), ("mismatch", json.dumps({"hook_event_name":"SessionStart", "session_id":"other", "transcript_path":"synthetic"}).encode())):
            marker = state / (name + ".txt")
            env = os.environ.copy()
            env.update({"HERDR_ENV":"1", "HERDR_PANE_ID":"synthetic-pane", "HERDR_BIN_PATH":str(helper), "CODEX_THREAD_ID":"synthetic-session", "WITNESS_MARKER":str(marker)})
            result = subprocess.run(["powershell", "-NoProfile", "-File", str(hook), "session"], input=data, capture_output=True, env=env, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(marker.exists())


if __name__ == "__main__":
    unittest.main()
