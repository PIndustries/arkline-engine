"""Offline packaging checks; no builds, network access, uploads or signing."""
import importlib.util
import json
import os
import zipfile
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("package_release", Path(__file__).with_name("package-release.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class PackagingTests(unittest.TestCase):
    def test_shared_source_manifest_has_no_platform_fields(self):
        record = package.source_manifest("26.10.16.1", "a" * 40, False)
        self.assertEqual(record["source_archive"], "Arkline-26.10.16.1-source.tar.gz")
        self.assertEqual(record["artifact_kind"], "source")
        for field in ["target", "build_host", "rustc", "apple_signing", "validation_runtime"]:
            self.assertNotIn(field, record)

    def test_source_archive_is_reproducible(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); source = root / "Arkline-26.10.16.1-source"
            source.mkdir(); file = source / "Cargo.lock"; file.write_text("locked")
            package.archive_source(source, root / "one.tar.gz")
            os.utime(file, (123, 123))
            package.archive_source(source, root / "two.tar.gz")
            self.assertEqual((root / "one.tar.gz").read_bytes(), (root / "two.tar.gz").read_bytes())

    def test_windows_cross_uses_target_artifacts_and_explicit_wine(self):
        target, windows, command, directory, runner = package.build_plan("x86_64-unknown-linux-gnu", "Linux", True)
        self.assertEqual(target, "x86_64-pc-windows-msvc")
        self.assertTrue(windows)
        self.assertEqual(command, ["cargo", "xwin", "build", "--target", target])
        self.assertEqual(directory, Path(target) / "release")
        self.assertEqual(runner, ["wine"])
        with self.assertRaises(ValueError):
            package.build_plan("aarch64-apple-darwin", "Darwin", True)

    def test_streaming_checksum_matches_known_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "archive"
            path.write_bytes(b"abc")
            self.assertEqual(package.sha256_file(path),
                             "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")

    def test_native_windows_stays_native(self):
        self.assertEqual(package.build_plan("x86_64-pc-windows-msvc", "Windows"),
                         ("x86_64-pc-windows-msvc", True, ["cargo", "build"], Path("release"), []))

    def test_notices_include_embedded_font_terms(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dependency = root / "font-crate"
            (dependency / "fonts").mkdir(parents=True)
            for name in ["OFL.txt", "UFL.txt", "Hack-Regular.txt", "emoji-icon-font-mit-license.txt"]:
                (dependency / "fonts" / name).write_text("Original notice " + name)
            (dependency / "LICENSE-MIT").write_text("Original license")
            metadata = {"packages": [{"name": "font-crate", "version": "1.0", "source": "registry+example",
                         "manifest_path": str(dependency / "Cargo.toml"), "license": "MIT AND OFL-1.1",
                         "repository": "https://example.invalid/fonts", "license_file": None}]}
            output = root / "ThirdParty"
            with patch.object(package, "run", return_value=json.dumps(metadata)):
                package.notices(root, output)
            inventory = json.loads((output / "dependencies.json").read_text())
            self.assertEqual(len(inventory[0]["notice_files"]), 5)
            self.assertEqual((output / "licenses/font-crate-1.0/fonts/Hack-Regular.txt").read_text(),
                             "Original notice Hack-Regular.txt")

    def test_archive_retains_executable_and_root_directory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            release = root / "Arkline-server"
            release.mkdir()
            binary = release / "arkline-server"
            binary.write_bytes(b"test")
            binary.chmod(0o755)
            output = root / "server.tar.gz"
            package.archive(release, output)
            with tarfile.open(output) as archive:
                member = archive.getmember("Arkline-server/arkline-server")
                if os.name != "nt":
                    self.assertEqual(member.mode & 0o777, 0o755)
                self.assertEqual(archive.extractfile(member).read(), b"test")

    def test_windows_zip_contains_executable_and_versioned_root(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            release = root / "Arkline-server"
            release.mkdir()
            (release / "arkline-server.exe").write_bytes(b"test-executable")
            output = root / "server.zip"
            package.archive(release, output, windows=True)
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(archive.read("Arkline-server/arkline-server.exe"), b"test-executable")

    def test_windows_zip_clamps_cargo_notice_epoch_without_changing_content(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            release = root / "Arkline-client"
            release.mkdir()
            notice = release / "LICENSE"
            notice.write_bytes(b"Original dependency license")
            os.utime(notice, (1, 1))
            output = root / "client.zip"
            package.archive(release, output, windows=True)
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(archive.read("Arkline-client/LICENSE"), b"Original dependency license")
                self.assertEqual(archive.getinfo("Arkline-client/LICENSE").date_time, (1980, 1, 1, 0, 0, 0))

    def test_source_rejects_runtime_data_even_if_tracked(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(package, "run", return_value="data/world.sqlite\0"):
                with self.assertRaisesRegex(RuntimeError, "runtime/build data"):
                    package.copy_source(Path(temporary))



class SourceModeTests(unittest.TestCase):
    def test_source_only_never_builds_native_binaries(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "repo"
            root.mkdir()
            (root / "BUILD_VERSION").write_text("26.10.16.1")
            output = Path(temp) / "output"

            def copy_source(destination):
                (destination / "BUILD_VERSION").write_text("26.10.16.1")

            def notices(source, destination):
                destination.mkdir()
                (destination / "dependencies.json").write_text("[]")

            def run(*args, **kwargs):
                if args[:2] == ("git", "status"):
                    return ""
                if args[:2] == ("git", "rev-parse"):
                    return "a" * 40
                if args == ("cargo", "vendor", "--locked", "vendor"):
                    return '[source.vendored-sources]\ndirectory = "vendor"\n'
                self.fail(f"Unexpected build command: {args}")

            with patch.object(package, "ROOT", root), patch.object(package, "run", run), patch.object(package, "copy_source", copy_source), patch.object(package, "notices", notices), patch.object(sys, "argv", ["package", str(output), "--source-only"]):
                package.main()
            self.assertEqual({x.name for x in output.iterdir()}, {"Arkline-26.10.16.1-source.tar.gz", "RELEASE.json", "SHA256SUMS"})
            self.assertNotIn("target", json.loads((output / "RELEASE.json").read_text()))

    def test_source_modes_and_links(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "source"
            root.mkdir()
            script = root / "build.sh"
            script.write_text("echo fixture")
            script.chmod(0o700)
            output = Path(temp) / "source.tar.gz"
            package.archive_source(root, output)
            with tarfile.open(output) as archive:
                self.assertEqual(archive.getmember("source/build.sh").mode, 0o755)
            (root / "link").symlink_to("build.sh")
            with self.assertRaises(ValueError):
                package.archive_source(root, output)


if __name__ == "__main__":
    unittest.main()
