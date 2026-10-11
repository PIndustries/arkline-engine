"""Offline packaging checks; no builds, network access, uploads or signing."""
import importlib.util
import json
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
                self.assertEqual(member.mode & 0o777, 0o755)
                self.assertEqual(archive.extractfile(member).read(), b"test")

    def test_source_rejects_runtime_data_even_if_tracked(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(package, "run", return_value="data/world.sqlite\0"):
                with self.assertRaisesRegex(RuntimeError, "runtime/build data"):
                    package.copy_source(Path(temporary))


if __name__ == "__main__":
    unittest.main()
