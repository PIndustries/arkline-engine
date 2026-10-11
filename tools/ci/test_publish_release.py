#!/usr/bin/env python3
import hashlib
import importlib.util
import io
import json
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

    def test_image_build_requires_isolated_explicit_builder_and_loaded_output(self):
        image = "ghcr.io/pindustries/arkline-server"
        calls = []
        def fake(*args, **kwargs):
            calls.append(args)
            if args[:3] == ("docker", "buildx", "inspect"):
                return b"docker-container\n"
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
        self.assertEqual(calls[2], ("docker", "push", image + ":revision"))
        for builder, driver in [("", b"docker-container"), ("default", b"docker"), ("arkline-publish-123-1", b"docker")]:
            with self.subTest(builder=builder, driver=driver), patch.dict(p.os.environ, {"ARKLINE_BUILDER": builder}), patch.object(p, "command", return_value=driver) as command:
                with self.assertRaises(ValueError):
                    p.private_image("arkline-server", "revision", Path("source"), Path("Dockerfile"), "b" * 40)
                self.assertFalse(any(call.args[:3] == ("docker", "buildx", "build") for call in command.call_args_list))

    def test_publication_urls_are_versioned_and_platform_correct(self):
        env=p.publication_env("26.10.11.6")
        self.assertTrue(env['ARKLINE_CLIENT_WINDOWS_URL'].endswith('-client.zip'))
        self.assertTrue(env['ARKLINE_SOURCE_WINDOWS_URL'].endswith('-source.tar.gz'))
        self.assertEqual(env['ARKLINE_SOURCE_URL'],env['ARKLINE_SOURCE_MACOS_URL'])


if __name__ == "__main__": unittest.main()
