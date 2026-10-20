"""Meaningful checks for disk refusal and workspace identity protection."""
import importlib.util
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("guard", Path(__file__).with_name("guarded-local-build.py"))
guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guard)


class BuildGuardTests(unittest.TestCase):
    def test_start_and_stop_limits_refuse_below_reserve(self):
        for threshold in (50 * guard.GIB, 30 * guard.GIB):
            with self.assertRaises(RuntimeError):
                guard.check_free(threshold - 1, threshold)
            guard.check_free(threshold, threshold)

    def test_workspace_move_or_replacement_is_refused(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name).resolve()
            with mock.patch.object(guard, "EXPECTED", root):
                original = guard.check_workspace(root)
                with self.assertRaises(RuntimeError):
                    guard.check_workspace(root, (original[0], original[1] + 1))
            with self.assertRaises(RuntimeError):
                guard.check_workspace(root)

    def test_guard_stops_its_owned_process_group(self):
        child = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(30)"],
            start_new_session=True,
        )
        try:
            self.assertIsNone(child.poll())
            guard.stop_child(child)
            self.assertIsNotNone(child.poll())
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait()

    def test_target_and_temporary_paths_cannot_inherit_external_override(self):
        with mock.patch.dict("os.environ", {"CARGO_TARGET_DIR": "/Volumes/MOVESPEED/other", "TMPDIR": "/tmp"}):
            env = guard.child_environment(guard.EXPECTED)
        self.assertEqual(env["CARGO_TARGET_DIR"], str(guard.EXPECTED / ".build/target"))
        self.assertEqual(env["TMPDIR"], str(guard.EXPECTED / ".build/tmp"))
        self.assertEqual(env["CARGO_INCREMENTAL"], "0")


if __name__ == "__main__":
    unittest.main()
