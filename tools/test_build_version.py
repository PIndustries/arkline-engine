import importlib.util
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from zoneinfo import ZoneInfo

SPEC = importlib.util.spec_from_file_location("arkline_build", Path(__file__).with_name("build.py"))
build = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(build)


class BuildVersionTests(unittest.TestCase):
    def test_increments_same_day_and_resets_on_new_day(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            version_file, lock_file = root / "BUILD_VERSION", root / "version.lock"
            version_file.write_text("26.10.13.7\n")
            self.assertEqual(build.allocate(version_file, lock_file, date(2026, 10, 13)), "26.10.13.8")
            self.assertEqual(build.allocate(version_file, lock_file, date(2026, 10, 14)), "26.10.14.1")

    def test_allocates_first_build_of_day_from_invalid_value(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            version_file, lock_file = root / "BUILD_VERSION", root / "version.lock"
            version_file.write_text("not-a-version\n")
            self.assertEqual(build.allocate(version_file, lock_file, date(2026, 10, 13)), "26.10.13.1")

    def test_parallel_allocations_are_unique(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            version_file, lock_file = root / "BUILD_VERSION", root / "version.lock"
            version_file.write_text("26.10.13.0\n")
            with ThreadPoolExecutor(max_workers=8) as pool:
                versions = list(pool.map(lambda _: build.allocate(version_file, lock_file, date(2026, 10, 13)), range(8)))
            self.assertEqual(sorted(int(v.rsplit(".", 1)[1]) for v in versions), list(range(1, 9)))

    def make_fake_cargo(self, root):
        fake_bin = root / "fake-bin"
        fake_bin.mkdir()
        executable = root / "fake-editor"
        executable.write_bytes(b"fake editor executable\n")
        cargo = fake_bin / "cargo"
        cargo.write_text("""#!/usr/bin/env python3
import json, os
from pathlib import Path
print(json.dumps({"reason":"compiler-artifact", "target":{"name":"arkline-editor", "kind":["bin"]}, "executable":os.environ["FAKE_EDITOR"]}))
raise SystemExit(int(os.environ.get("FAKE_CARGO_EXIT", "0")))
""")
        cargo.chmod(0o755)
        return fake_bin, executable

    def run_fake_build(self, root, fake_bin, executable, version_output, artifact_copy, exit_code="0"):
        env = os.environ.copy()
        env["PATH"] = str(fake_bin) + os.pathsep + env["PATH"]
        env["FAKE_EDITOR"] = str(executable)
        env["FAKE_CARGO_EXIT"] = exit_code
        return subprocess.run(
            [sys.executable, str(Path(__file__).with_name("build.py")),
             "--repo-root", str(root), "--artifact-copy", str(artifact_copy),
             "--version-output", str(version_output), "--", "build"],
            cwd=root, env=env, text=True, capture_output=True,
        )

    def test_wrapper_stages_exact_artifact_and_allocated_version(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            today = datetime.now(ZoneInfo("America/Chicago")).date()
            root.joinpath("BUILD_VERSION").write_text(f"{today.year % 100:02}.{today.month}.{today.day}.0\n")
            fake_bin, executable = self.make_fake_cargo(root)
            receipt, staged = root / "stage" / "BUILD_VERSION", root / "stage" / "editor"
            result = self.run_fake_build(root, fake_bin, executable, receipt, staged)
            self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
            version = root.joinpath("BUILD_VERSION").read_text().strip()
            self.assertEqual(receipt.read_text().strip(), version)
            self.assertEqual(staged.read_bytes(), executable.read_bytes())
            self.assertEqual(version, f"{today.year % 100:02}.{today.month}.{today.day}.1")

    def test_failed_wrapper_consumes_version_without_success_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            today = datetime.now(ZoneInfo("America/Chicago")).date()
            root.joinpath("BUILD_VERSION").write_text(f"{today.year % 100:02}.{today.month}.{today.day}.0\n")
            fake_bin, executable = self.make_fake_cargo(root)
            receipt, staged = root / "stage" / "BUILD_VERSION", root / "stage" / "editor"
            result = self.run_fake_build(root, fake_bin, executable, receipt, staged, exit_code="7")
            self.assertEqual(result.returncode, 7)
            today = datetime.now(ZoneInfo("America/Chicago")).date()
            self.assertEqual(root.joinpath("BUILD_VERSION").read_text().strip(),
                             f"{today.year % 100:02}.{today.month}.{today.day}.1")
            self.assertFalse(receipt.exists())
            self.assertFalse(staged.exists())


if __name__ == "__main__":
    unittest.main()
