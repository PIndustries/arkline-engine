#!/usr/bin/env python3
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location("publish", Path(__file__).with_name("publish-release.py"))
p = importlib.util.module_from_spec(spec); spec.loader.exec_module(p)


class PublicationSafety(unittest.TestCase):
    def test_run_requires_own_main_success_and_correct_workflow(self):
        run = dict(status="completed", conclusion="success", head_branch="main", workflow_id=7,
                   event="workflow_dispatch", head_repository={"full_name": p.REPO}, head_sha="a" * 40)
        self.assertEqual(p.validate_run(run, 7), "a" * 40)
        for field, value in [("head_branch", "fork"), ("conclusion", "failure"), ("workflow_id", 9), ("event", "pull_request"), ("head_repository", {"full_name": "someone/fork"})]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                p.validate_run(dict(run, **{field: value}), 7)

    def test_release_tag_is_precreated_exact_lightweight_and_never_changed(self):
        good = {"ref": "refs/tags/v26.10.11.7", "object": {"type": "commit", "sha": "a" * 40}}
        with patch.object(p, "gh", return_value=good) as request:
            p.verify_release_tag("26.10.11.7", "a" * 40)
            request.assert_called_once_with("git/ref/tags/v26.10.11.7")
        for ref in [{}, dict(good, ref="refs/tags/other"), dict(good, object={"type": "tag", "sha": "a" * 40}), dict(good, object={"type": "commit", "sha": "b" * 40})]:
            with patch.object(p, "gh", return_value=ref), self.assertRaises(ValueError):
                p.verify_release_tag("26.10.11.7", "a" * 40)
        with patch.object(p, "gh", side_effect=p.subprocess.CalledProcessError(1, "gh")), self.assertRaisesRegex(ValueError, "Precreate"):
            p.verify_release_tag("26.10.11.7", "a" * 40)

    def test_zip_traversal_symlink_and_duplicates_rejected_before_writes(self):
        for case in ["traversal", "symlink", "duplicate"]:
            with self.subTest(case=case), tempfile.TemporaryDirectory() as temp:
                root = Path(temp); artifact = root / "input.zip"; out = root / "out"; out.mkdir()
                with zipfile.ZipFile(artifact, "w") as z:
                    z.writestr("good", "safe")
                    if case == "traversal": z.writestr("../outside", "bad")
                    elif case == "duplicate": z.writestr("./good", "duplicate")
                    else:
                        entry = zipfile.ZipInfo("link"); entry.create_system = 3
                        entry.external_attr = (stat.S_IFLNK | 0o777) << 16
                        z.writestr(entry, "../outside")
                with self.assertRaises(ValueError): p.unpack_artifact(artifact, out)
                self.assertEqual(list(out.iterdir()), [])

    def test_zip_size_limit_prevents_extraction(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); archive=root/"input.zip"; out=root/"out"; out.mkdir()
            with zipfile.ZipFile(archive,"w") as z: z.writestr("large", b"x" * 20)
            old=p.MAX_BYTES; p.MAX_BYTES=10
            try:
                with self.assertRaises(ValueError): p.unpack_artifact(archive,out)
            finally: p.MAX_BYTES=old
            self.assertEqual(list(out.iterdir()),[])

    def test_checksums_cover_every_file_and_reject_tampering(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); (root/"notice").write_text("license")
            checksum=hashlib.sha256(b"license").hexdigest()
            (root/"SHA256SUMS").write_text(f"{checksum}  notice\n")
            self.assertEqual(p.verify_checksums(root),{"notice":checksum})
            (root/"extra").write_text("unlisted")
            with self.assertRaises(ValueError): p.verify_checksums(root)
            (root/"extra").unlink(); (root/"notice").write_text("modified")
            with self.assertRaises(ValueError): p.verify_checksums(root)

    def test_receipt_rejects_dirty_wrong_commit_or_target(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); record=dict(build_version="26.10.11.6",source_commit="a"*40,target="linux",uncommitted_source=False)
            for key,value in [(None,None),("uncommitted_source",True),("source_commit","b"*40),("target","windows")]:
                candidate=dict(record)
                if key:candidate[key]=value
                (root/"RELEASE.json").write_text(json.dumps(candidate))
                if key:
                    with self.assertRaises(ValueError): p.receipt(root,"26.10.11.6","a"*40,"linux")
                else:p.receipt(root,"26.10.11.6","a"*40,"linux")

    def test_source_compares_tracked_bytes_not_only_claimed_commit(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); archive=root/"source.tar.gz"
            with tarfile.open(archive,"w:gz") as t:
                for name,data in {"BUILD_VERSION":b"26.10.11.6\n","code.rs":b"original"}.items():
                    info=tarfile.TarInfo("source/"+name); info.size=len(data);t.addfile(info,io.BytesIO(data))
            expected={"code.rs":hashlib.sha256(b"original").hexdigest()}
            p.verify_source(archive,expected,"26.10.11.6")
            with self.assertRaises(ValueError):p.verify_source(archive,{"code.rs":hashlib.sha256(b"changed").hexdigest()},"26.10.11.6")

    def test_inner_binary_receipt_must_match_outer_claim(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp); archive=root/"client.zip"
            record={"build_version":"26.10.11.6","source_commit":"a"*40}
            with zipfile.ZipFile(archive,"w") as z:
                z.writestr("client/RELEASE.json",json.dumps(record))
                z.writestr("client/BUILD_VERSION","26.10.11.6\n")
            p.verify_binary_receipt(archive,record)
            with self.assertRaises(ValueError):p.verify_binary_receipt(archive,dict(record,source_commit="b"*40))

    def test_bastion_patch_never_rewrites_unrelated_secret_environment(self):
        resource={"metadata":{"resourceVersion":"7"},"spec":{"template":{"spec":{"containers":[{"name":"app","env":[{"name":"SECRET","valueFrom":{"secretKeyRef":{"name":"private","key":"key"}}},{"name":"ARKLINE_RELEASE_VERSION","value":"old"}]}]}}}}
        captured=[]
        def fake(*args):
            if "get" in args:return json.dumps(resource).encode()
            if "--patch-file" in args:
                captured.extend(json.loads(Path(args[args.index("--patch-file")+1]).read_text()))
            return b""
        with patch.object(p,"kube",fake):p.patch_publication({"ARKLINE_RELEASE_VERSION":"new","ARKLINE_PLAY_URL":"https://play.arkline.network"})
        self.assertEqual(captured[1]["path"],"/spec/template/spec/containers/0/env/1")
        self.assertEqual(captured[2]["path"],"/spec/template/spec/containers/0/env/-")
        self.assertNotIn("SECRET",json.dumps(captured))
        self.assertEqual(captured[0]["op"],"test")

    def test_play_redirect_is_absolute_https_and_preserves_query(self):
        self.assertEqual(p.release_redirect("26.10.11.6"),"location = / { return 302 https://play.arkline.network/26.10.11.6/$is_args$args; }\n")

    def test_failed_public_hash_restores_previous_web_digest(self):
        calls=[]
        with patch.object(p,"rollout",lambda *args:calls.append(args)), patch.object(p,"verify_public",side_effect=ValueError("bad hash")):
            with self.assertRaisesRegex(RuntimeError,"previous web digest restored"):
                p.deploy_verified_web("new@digest","old@digest",[("https://downloads.arkline.network/file","expected")],"26.10.11.6")
        self.assertEqual([call[-1] for call in calls],["new@digest","old@digest"])

    def test_failed_rollback_requires_operator_recovery(self):
        with patch.object(p,"rollout",side_effect=[None,RuntimeError("rollback unavailable")]), patch.object(p,"verify_public",side_effect=ValueError("bad hash")):
            with self.assertRaisesRegex(RuntimeError,"rollback failed"):
                p.deploy_verified_web("new@digest","old@digest",[("url","hash")],"26.10.11.6")

    def test_public_staging_is_readable_with_private_umask_without_touching_credentials(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); stage = root / "public"
            previous_umask = os.umask(0o077)
            try:
                stage.mkdir()
                folder = stage / "web"; folder.mkdir()
                page = folder / "index.html"; page.write_text("public")
                config = stage / "nginx.conf"; config.write_text("public config")
                secret = root / "credential"; secret.write_text("private")
            finally:
                os.umask(previous_umask)
            self.assertEqual(stat.S_IMODE(config.stat().st_mode), 0o600)
            p.prepare_public_tree(stage)
            for directory in [stage, folder]: self.assertEqual(stat.S_IMODE(directory.stat().st_mode), 0o755)
            for file in [page, config]: self.assertEqual(stat.S_IMODE(file.stat().st_mode), 0o644)
            self.assertEqual(stat.S_IMODE(secret.stat().st_mode), 0o600)
            (stage / "link").symlink_to(secret)
            with self.assertRaises(ValueError): p.prepare_public_tree(stage)
            self.assertEqual(stat.S_IMODE(secret.stat().st_mode), 0o600)

    def test_image_build_requires_isolated_explicit_builder_and_loaded_output(self):
        image = "ghcr.io/pindustries/arkline-server"
        calls = []
        def fake(*args, **kwargs):
            calls.append(args)
            if args[:3] == ("docker", "buildx", "inspect"):
                return b"Name: arkline-publish-123-1\nDriver:        docker-container\n"
            if args[:2] == ("gh", "api"):
                return json.dumps({"visibility": "private"}).encode()
            if args[:3] == ("docker", "image", "inspect"):
                return json.dumps([{"RepoDigests": [image + "@sha256:" + "a" * 64]}]).encode()
            return b""
        with patch.dict(p.os.environ, {"ARKLINE_BUILDER": "arkline-publish-123-1"}), patch.object(p, "command", fake):
            result = p.private_image("arkline-server", "revision", Path("source"), Path("Dockerfile"), "b" * 40)
        self.assertEqual(result, image + "@sha256:" + "a" * 64)
        build = calls[1]
        self.assertEqual(build[:7], ("docker", "buildx", "build", "--builder", "arkline-publish-123-1", "--load", "--platform"))
        self.assertEqual(calls[2], ("docker", "push", image + ":revision-arkline-publish-123-1"))
        for builder, driver in [("", b"Driver: docker-container"), ("default", b"Driver: docker"), ("arkline-publish-123-1", b"Driver: docker"), ("arkline-publish-123-1", b"Name: missing driver"), ("arkline-publish-123-1", b"Driver: docker-container\nDriver: docker-container")]:
            with self.subTest(builder=builder, driver=driver), patch.dict(p.os.environ, {"ARKLINE_BUILDER": builder}), patch.object(p, "command", return_value=driver) as command:
                with self.assertRaises(ValueError):
                    p.private_image("arkline-server", "revision", Path("source"), Path("Dockerfile"), "b" * 40)
                self.assertFalse(any(call.args[:3] == ("docker", "buildx", "build") for call in command.call_args_list))

    def test_publication_urls_are_versioned_and_platform_correct(self):
        env=p.publication_env("26.10.11.6")
        self.assertTrue(env['ARKLINE_CLIENT_WINDOWS_URL'].endswith('-client.zip'))
        self.assertTrue(env['ARKLINE_SOURCE_WINDOWS_URL'].endswith('-source.tar.gz'))
        self.assertEqual(env['ARKLINE_SOURCE_URL'],env['ARKLINE_SOURCE_MACOS_URL'])



class SharedSourceRelease(unittest.TestCase):
    version = "26.10.16.1"
    commit = "a" * 40

    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        names = ["arkline-source", "arkline-browser"] + ["arkline-downloads-" + x for x in p.TARGETS]
        self.folders = {name: self.root / name for name in names}
        self.source_name = f"Arkline-{self.version}-source.tar.gz"
        for folder in self.folders.values():
            folder.mkdir()
        record = dict(build_version=self.version, source_commit=self.commit,
                      uncommitted_source=False, artifact_kind="source",
                      source_archive=self.source_name, license="AGPL-3.0-only")
        self.record(self.folders["arkline-source"], record)
        self.archive(self.folders["arkline-source"] / self.source_name, record)
        for platform, target in dict(p.TARGETS, browser="wasm32-unknown-unknown").items():
            key = "arkline-browser" if platform == "browser" else "arkline-downloads-" + platform
            folder = self.folders[key]
            native = dict(record, target=target)
            native.pop("artifact_kind")
            self.record(folder, native)
            if platform == "browser":
                for name in ["BUILD_VERSION", "index.html", "bootstrap.js", "arkline-client.js", "arkline-client_bg.wasm", "LICENSE", "TRADEMARKS.md", "ThirdParty/dependencies.json"]:
                    path = folder / name
                    path.parent.mkdir(exist_ok=True)
                    path.write_text(self.version if name == "BUILD_VERSION" else "fixture")
            else:
                for kind in ["client", "server"]:
                    suffix = ".zip" if platform == "windows" else ".tar.gz"
                    self.archive(folder / f"Arkline-{self.version}-{target}-{kind}{suffix}", native)
        self.checksums()

    def record(self, folder, record):
        (folder / "RELEASE.json").write_text(json.dumps(record))

    def archive(self, path, record, locked=b"locked"):
        files = {"RELEASE.json": json.dumps(record).encode(),
                 "BUILD_VERSION": self.version.encode(), "Cargo.lock": locked}
        if path.suffix == ".zip":
            with zipfile.ZipFile(path, "w") as archive:
                for name, data in files.items():
                    archive.writestr("bundle/" + name, data)
        else:
            with tarfile.open(path, "w:gz") as archive:
                for name, data in files.items():
                    info = tarfile.TarInfo("bundle/" + name)
                    info.size = len(data)
                    archive.addfile(info, io.BytesIO(data))

    def checksums(self):
        for folder in self.folders.values():
            entries = [f"{p.digest(x)}  {x.relative_to(folder)}" for x in sorted(folder.rglob("*"))
                       if x.is_file() and x.name != "SHA256SUMS"]
            (folder / "SHA256SUMS").write_text("\n".join(entries) + "\n")

    def verify(self):
        for folder in self.folders.values():
            p.verify_checksums(folder)
        hashes = {"Cargo.lock": hashlib.sha256(b"locked").hexdigest()}
        p.verify_release_artifacts(self.folders, self.version, self.commit, hashes)

    def test_complete_set_and_source_urls(self):
        self.verify()
        env = p.publication_env(self.version)
        for key, value in env.items():
            if key.startswith("ARKLINE_SOURCE"):
                self.assertEqual(value, f"https://downloads.arkline.network/{self.version}/{self.source_name}")

    def test_missing_duplicate_and_expired_source(self):
        items = [dict(name=x, expired=False) for x in self.folders]
        self.assertEqual(len(p.select_artifacts(items)), 5)
        source = items[0]
        for bad in [items[1:], items + [source], [dict(source, expired=True)] + items[1:]]:
            with self.subTest(artifacts=bad), self.assertRaises(ValueError):
                p.select_artifacts(bad)

    def test_mismatched_receipts_and_stale_source_names(self):
        for key, folder in self.folders.items():
            original = json.loads((folder / "RELEASE.json").read_text())
            changes = dict(build_version="wrong", source_commit="b"*40,
                           uncommitted_source=True, source_archive="Arkline-old-target-source.tar.gz")
            if key == "arkline-source":
                changes["target"] = "linux"
            for field, value in changes.items():
                with self.subTest(folder=key, field=field):
                    self.record(folder, dict(original, **{field: value}))
                    self.checksums()
                    with self.assertRaises(ValueError):
                        self.verify()
                    self.record(folder, original)
            self.checksums()

    def test_source_checksum_tamper(self):
        source = self.folders["arkline-source"] / self.source_name
        source.write_bytes(source.read_bytes() + b"tampered")
        with self.assertRaises(ValueError):
            self.verify()

    def test_source_bytes_tampered_with_fresh_checksum(self):
        folder = self.folders["arkline-source"]
        record = json.loads((folder / "RELEASE.json").read_text())
        self.archive(folder / self.source_name, record, b"tampered lock")
        self.checksums()
        with self.assertRaises(ValueError):
            self.verify()

    def test_extra_platform_source_is_rejected(self):
        folder = self.folders["arkline-downloads-linux"]
        (folder / self.source_name).write_bytes(b"unexpected duplicate")
        self.checksums()
        with self.assertRaises(ValueError):
            self.verify()


if __name__ == "__main__": unittest.main()
