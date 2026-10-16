#!/usr/bin/env python3
"""Validate one successful release-build run; publication requires explicit --apply.

No public-version replacement, artifact execution, or archive extraction outside a
fresh staging directory. Secrets are inherited through standard GH/Kubernetes
credentials and never included in the plan or command output.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
import zipfile

REPO = "PIndustries/arkline-engine"
ROOT = Path(__file__).resolve().parents[2]
TARGETS = {"linux": "x86_64-unknown-linux-gnu", "windows": "x86_64-pc-windows-msvc", "macos": "aarch64-apple-darwin"}
MAX_BYTES = 20 * 1024**3
MAX_FILES = 150_000


def command(*args, cwd=ROOT, capture=True, input=None):
    result = subprocess.run([str(a) for a in args], cwd=cwd, input=input,
                            stdout=subprocess.PIPE if capture else None, check=True)
    return result.stdout


def gh(path):
    return json.loads(command("gh", "api", f"repos/{REPO}/{path}"))


def verify_release_tag(version, commit):
    """A maintainer precreates this exact lightweight tag; the job never moves it."""
    try:
        ref = gh(f"git/ref/tags/v{version}")
    except subprocess.CalledProcessError as error:
        raise ValueError("Precreate and verify the exact release tag before publication") from error
    if (ref.get("ref") != f"refs/tags/v{version}"
            or ref.get("object", {}).get("type") != "commit"
            or ref["object"].get("sha") != commit):
        raise ValueError("Release tag must be a lightweight tag at the verified artifact source")


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def safe_name(name):
    path = PurePosixPath(name)
    if not name or "\\" in name or path.is_absolute() or ".." in path.parts or ":" in name:
        raise ValueError("Unsafe archive/checksum path")
    return path


def unpack_artifact(archive, destination):
    with zipfile.ZipFile(archive) as bundle:
        entries = bundle.infolist()
        if len(entries) > MAX_FILES or sum(i.file_size for i in entries) > MAX_BYTES:
            raise ValueError("Artifact exceeds extraction limit")
        seen = set()
        for item in entries:
            name = safe_name(item.filename).as_posix()
            if name in seen or stat.S_ISLNK(item.external_attr >> 16):
                raise ValueError("Duplicate or symlink artifact member")
            seen.add(name)
        # Every member is validated before the first write.
        for item in entries:
            target = destination / item.filename
            if item.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with bundle.open(item) as source, target.open("xb") as out:
                    shutil.copyfileobj(source, out)


def verify_checksums(folder):
    sums = {}
    for line in (folder / "SHA256SUMS").read_text().splitlines():
        match = re.fullmatch(r"([a-f0-9]{64})  (.+)", line)
        if not match:
            raise ValueError("Malformed checksum manifest")
        expected, name = match.groups()
        safe_name(name)
        if name in sums or name == "SHA256SUMS":
            raise ValueError("Duplicate/self-referential checksum")
        path = folder / name
        if not path.is_file() or path.is_symlink() or digest(path) != expected:
            raise ValueError(f"Artifact checksum mismatch: {name}")
        sums[name] = expected
    actual = {p.relative_to(folder).as_posix() for p in folder.rglob("*") if p.is_file() and p != folder / "SHA256SUMS"}
    if set(sums) != actual or not sums:
        raise ValueError("Unmanifested or missing artifact files")
    return sums


def receipt(folder, version, commit, target):
    record = json.loads((folder / "RELEASE.json").read_text())
    if (record.get("build_version") != version or record.get("source_commit") != commit
            or record.get("target") != target or record.get("uncommitted_source") is not False):
        raise ValueError("Artifact version/source/target mismatch or dirty source")
    return record


def tracked_source(commit):
    names = command("git", "ls-tree", "-r", "--name-only", "-z", commit).decode().split("\0")
    return {name: hashlib.sha256(command("git", "show", f"{commit}:{name}")).hexdigest()
            for name in names if name}


def verify_source(archive, expected, version):
    """Read archive members only; compare every tracked source byte to Git."""
    with tarfile.open(archive, "r:gz") as bundle:
        files = {}
        count = total = 0
        for member in bundle:
            count += 1; total += member.size
            if count > MAX_FILES or total > MAX_BYTES:
                raise ValueError("Source archive exceeds bounds")
            parts = safe_name(member.name).parts
            if not member.isdir() and not member.isfile():
                raise ValueError("Source archive links/devices are not permitted")
            if member.isfile():
                relative = "/".join(parts[1:])
                if relative in files:
                    raise ValueError("Duplicate source member")
                files[relative] = member
        for name, expected_hash in expected.items():
            member = files.get(name)
            if member is None:
                raise ValueError(f"Source archive missing tracked file: {name}")
            with bundle.extractfile(member) as handle:
                if hashlib.file_digest(handle, "sha256").hexdigest() != expected_hash:
                    raise ValueError(f"Source archive differs from commit: {name}")
        if bundle.extractfile(files["BUILD_VERSION"]).read().decode().strip() != version:
            raise ValueError("Source archive version mismatch")


def verify_binary_receipt(archive, expected):
    """Inspect inner manifests without extracting executable payloads or links."""
    values = {}
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as bundle:
            members = bundle.infolist()
            if len(members) > MAX_FILES or sum(m.file_size for m in members) > MAX_BYTES:
                raise ValueError("Binary archive exceeds bounds")
            for member in members:
                parts = safe_name(member.filename).parts
                if len(parts) == 2 and parts[1] in ["RELEASE.json", "BUILD_VERSION"]:
                    if member.file_size > 64 * 1024 or parts[1] in values:
                        raise ValueError("Invalid inner release receipt")
                    values[parts[1]] = bundle.read(member)
    else:
        with tarfile.open(archive, "r:gz") as bundle:
            count = total = 0
            for member in bundle:
                count += 1; total += member.size
                if count > MAX_FILES or total > MAX_BYTES:
                    raise ValueError("Binary archive exceeds bounds")
                parts = safe_name(member.name).parts
                if member.isfile() and len(parts) == 2 and parts[1] in ["RELEASE.json", "BUILD_VERSION"]:
                    if member.size > 64 * 1024 or parts[1] in values:
                        raise ValueError("Invalid inner release receipt")
                    values[parts[1]] = bundle.extractfile(member).read()
    if json.loads(values.get("RELEASE.json", b"null")) != expected:
        raise ValueError("Inner binary receipt differs from artifact receipt")
    if values.get("BUILD_VERSION", b"").decode().strip() != expected["build_version"]:
        raise ValueError("Inner binary version mismatch")


def validate_run(run, workflow_id):
    if (run.get("status") != "completed" or run.get("conclusion") != "success"
            or run.get("head_branch") != "main" or run.get("workflow_id") != workflow_id
            or run.get("event") != "workflow_dispatch"
            or run.get("head_repository", {}).get("full_name", "").lower() != REPO.lower()
            or not re.fullmatch("[a-f0-9]{40}", run.get("head_sha", ""))):
        raise ValueError("Require successful main-branch release.yml dispatch from this repository")
    return run["head_sha"]


def kube(*args):
    return command("kubectl", *args)


def rollout(kind, name, namespace, image):
    deployment = json.loads(kube("-n", namespace, "get", kind, name, "-o", "json"))
    containers = deployment["spec"]["template"]["spec"]["containers"]
    container = "web" if name == "arkline-web" else "server"
    if not any(c["name"] == container for c in containers):
        raise ValueError("Unexpected workload container identity")
    kube("-n", namespace, "set", "image", f"{kind}/{name}", f"{container}={image}")
    kube("-n", namespace, "rollout", "status", f"{kind}/{name}", "--timeout=300s")


def prepare_public_tree(root):
    """Only the public web-image staging tree: credentials stay outside it."""
    paths = [root, *root.rglob("*")]
    if any(path.is_symlink() for path in paths):
        raise ValueError("Public image staging must not contain symlinks")
    for path in paths:
        path.chmod(0o755 if path.is_dir() else 0o644)


def private_image(name, tag, context, dockerfile, commit):
    builder = os.environ.get("ARKLINE_BUILDER", "")
    if not re.fullmatch(r"arkline-publish-[0-9]+-[0-9]+", builder):
        raise ValueError("An explicit job-scoped Arkline publication builder is required")
    # Each publication attempt gets distinct registry tags; prior images remain intact.
    image = f"ghcr.io/pindustries/{name}:{tag}-{builder}"
    inspection = command("docker", "buildx", "inspect", builder)
    if isinstance(inspection, bytes):
        inspection = inspection.decode("utf-8", errors="strict")
    drivers = re.findall(r"^Driver:[ \t]*([^\r\n]+)$", inspection, flags=re.MULTILINE)
    if [driver.strip() for driver in drivers] != ["docker-container"]:
        raise ValueError("Publication requires an isolated docker-container builder")
    command("docker", "buildx", "build", "--builder", builder, "--load",
            "--platform", "linux/amd64", "-f", dockerfile,
            "--build-arg", f"SOURCE_COMMIT={commit}", "--label",
            f"org.opencontainers.image.revision={commit}", "-t", image, context, capture=False)
    command("docker", "push", image, capture=False)
    package = json.loads(command("gh", "api", f"orgs/PIndustries/packages/container/{name}"))
    if package.get("visibility") != "private":
        raise ValueError("Registry package privacy not verified; deployment stopped")
    value = json.loads(command("docker", "image", "inspect", image))[0]
    digests = [d for d in value["RepoDigests"] if d.startswith(f"ghcr.io/pindustries/{name}@sha256:")]
    if len(digests) != 1:
        raise ValueError("Image digest unresolved")
    return digests[0]


def public_hash(url):
    request = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 Arkline-Release-Validation/1.0"})
    with urllib.request.urlopen(request, timeout=60) as response:
        if response.status != 200:
            raise ValueError("Public artifact unavailable")
        return hashlib.file_digest(response, "sha256").hexdigest()


def verify_public(url, expected):
    error = None
    for attempt in range(6):
        try:
            if public_hash(url) == expected:
                return
            error = ValueError("Public artifact hash mismatch")
        except (OSError, ValueError) as e:
            error = e
        time.sleep(min(2 ** attempt, 10))
    raise ValueError(f"Public verification failed: {url}") from error


def release_redirect(version):
    return f"location = / {{ return 302 https://play.arkline.network/{version}/$is_args$args; }}\n"


def deploy_verified_web(image, previous, checks, version):
    try:
        rollout("deployment", "arkline-web", "arkline", image)
        for url, expected in checks:
            verify_public(url, expected)
        request = urllib.request.Request("https://play.arkline.network/?network=test",
                                         headers={"User-Agent": "Mozilla/5.0 Arkline-Release-Validation/1.0"})
        with urllib.request.urlopen(request, timeout=30) as response:
            if response.url != f"https://play.arkline.network/{version}/?network=test":
                raise ValueError("Play redirect did not preserve HTTPS/version/query")
    except Exception as failure:
        try:
            rollout("deployment", "arkline-web", "arkline", previous)
        except Exception as rollback_failure:
            raise RuntimeError("Web verification failed AND previous-image rollback failed; operator recovery required") from rollback_failure
        raise RuntimeError("Web verification failed; previous web digest restored. No server or publication link update attempted") from failure


def publication_env(version):
    base = f"https://downloads.arkline.network/{version}"
    env = {"ARKLINE_RELEASE_PUBLIC": "true", "ARKLINE_RELEASE_VERSION": version,
           "ARKLINE_BROWSER_READY": "true", "ARKLINE_PLAY_URL": "https://play.arkline.network",
           "ARKLINE_WORLD_AVAILABLE": "true", "ARKLINE_WORLD_ENVIRONMENT": "production",
           "ARKLINE_CHECKSUMS_URL": base + "/SHA256SUMS", "ARKLINE_LICENSE_URL": base + "/LICENSE"}
    for platform, target in TARGETS.items():
        for kind in ["client", "server"]:
            suffix = ".zip" if platform == "windows" and kind != "source" else ".tar.gz"
            env[f"ARKLINE_{kind.upper()}_{platform.upper()}_URL"] = f"{base}/Arkline-{version}-{target}-{kind}{suffix}"
    for platform in [*TARGETS, None]:
        key = "ARKLINE_SOURCE" + ("_" + platform.upper() if platform else "") + "_URL"
        env[key] = f"{base}/Arkline-{version}-source.tar.gz"
    return env


def patch_publication(desired):
    resource = json.loads(kube("-n", "bastion", "get", "deployment", "bastion", "-o", "json"))
    containers = resource["spec"]["template"]["spec"]["containers"]
    index = next(i for i, c in enumerate(containers) if c["name"] == "app")
    existing = containers[index].get("env", [])
    base = f"/spec/template/spec/containers/{index}/env"
    patches = [{"op": "test", "path": "/metadata/resourceVersion", "value": resource["metadata"]["resourceVersion"]}]
    if "env" not in containers[index]:
        patches.append({"op": "add", "path": base, "value": []})
    for name, value in desired.items():
        indices = [i for i, entry in enumerate(existing) if entry["name"] == name]
        if len(indices) > 1:
            raise ValueError("Duplicate publication environment key")
        path = base + (f"/{indices[0]}" if indices else "/-")
        patches.append({"op": "replace" if indices else "add", "path": path,
                        "value": {"name": name, "value": value}})
    # Payload contains only release URLs/flags; unrelated secret references never enter it.
    with tempfile.NamedTemporaryFile(mode="w", suffix=".json") as patch:
        json.dump(patches, patch); patch.flush()
        kube("-n", "bastion", "patch", "deployment", "bastion", "--type=json", "--patch-file", patch.name)
    kube("-n", "bastion", "rollout", "status", "deployment/bastion", "--timeout=300s")


def select_artifacts(artifacts):
    needed = {f"arkline-downloads-{p}" for p in TARGETS} | {"arkline-browser", "arkline-source"}
    selected = {name: [a for a in artifacts if a["name"] == name] for name in needed}
    if any(len(v) != 1 or v[0]["expired"] for v in selected.values()):
        raise ValueError("All three platform artifacts plus browser and shared source artifacts are required exactly once")
    return selected


def verify_release_artifacts(folders, version, commit, hashes):
    source = folders["arkline-source"]
    expected = f"Arkline-{version}-source.tar.gz"
    record = receipt(source, version, commit, None)
    neutral = {"build_version": version, "source_commit": commit,
               "uncommitted_source": False, "artifact_kind": "source",
               "source_archive": expected, "license": "AGPL-3.0-only"}
    if record != neutral:
        raise ValueError("Shared source receipt must be platform-neutral")
    if {p.name for p in source.iterdir()} != {expected, "RELEASE.json", "SHA256SUMS"}:
        raise ValueError("Unexpected shared source artifact set")
    verify_source(source / expected, hashes, version)
    verify_binary_receipt(source / expected, record)
    for platform, target in TARGETS.items():
        folder = folders[f"arkline-downloads-{platform}"]
        record = receipt(folder, version, commit, target)
        expected = f"Arkline-{version}-source.tar.gz"
        if record.get("source_archive") != expected:
            raise ValueError("Source offer mismatch")
        suffix = ".zip" if platform == "windows" else ".tar.gz"
        required = { f"Arkline-{version}-{target}-client{suffix}", f"Arkline-{version}-{target}-server{suffix}", "RELEASE.json", "SHA256SUMS"}
        for kind in ["client", "server"]:
            verify_binary_receipt(folder / f"Arkline-{version}-{target}-{kind}{suffix}", record)
        if {p.name for p in folder.iterdir()} != required:
            raise ValueError("Unexpected platform artifact set")
    browser = folders["arkline-browser"]
    record = receipt(browser, version, commit, "wasm32-unknown-unknown")
    if record.get("source_archive") != f"Arkline-{version}-source.tar.gz":
        raise ValueError("Browser source offer mismatch")
    if (browser / "BUILD_VERSION").read_text().strip() != version or not (browser / "ThirdParty/dependencies.json").is_file():
        raise ValueError("Browser version/notices missing")
    for name in ["index.html", "bootstrap.js", "arkline-client.js", "arkline-client_bg.wasm", "LICENSE", "TRADEMARKS.md"]:
        if not (browser / name).is_file():
            raise ValueError("Browser payload incomplete")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", type=int, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--upgrade-servers", action="store_true")
    parser.add_argument("--backup-reference", default="")
    args = parser.parse_args()
    if not re.fullmatch(r"\d{2}\.\d{1,2}\.\d{1,2}\.\d+", args.version):
        parser.error("Invalid version")
    if args.upgrade_servers and not args.backup_reference.strip():
        parser.error("Server upgrades require a verified operator backup reference")
    out = args.output.resolve()
    if out == ROOT or ROOT in out.parents:
        parser.error("Output must be outside the repository")
    if out.exists() and any(out.iterdir()):
        parser.error("Output directory must be new or empty")
    out.mkdir(parents=True, exist_ok=True)
    run = gh(f"actions/runs/{args.run_id}")
    commit = validate_run(run, gh("actions/workflows/release.yml")["id"])
    command("git", "cat-file", "-e", commit + "^{commit}")
    if command("git", "show", f"{commit}:BUILD_VERSION").decode().strip() != args.version:
        raise ValueError("Requested version differs from release source")
    if args.apply:
        verify_release_tag(args.version, commit)
        # Refuse overwriting an existing GitHub release even if CDN state differs.
        result = subprocess.run(["gh", "api", f"repos/{REPO}/releases/tags/v{args.version}"], capture_output=True)
        if result.returncode == 0:
            raise ValueError("Release version already exists")
        if b"404" not in result.stderr:
            raise ValueError("Could not verify release absence")
    artifacts = []
    page = 1
    while True:
        batch = gh(f"actions/runs/{args.run_id}/artifacts?per_page=100&page={page}")["artifacts"]
        artifacts.extend(batch)
        if len(batch) < 100:
            break
        page += 1
    selected = select_artifacts(artifacts)
    folders = {}
    for name, choices in selected.items():
        archive = out / (name + ".zip")
        with archive.open("wb") as stream:
            subprocess.run(["gh", "api", f"repos/{REPO}/actions/artifacts/{choices[0]['id']}/zip"], stdout=stream, check=True)
        folder = out / name
        folder.mkdir()
        unpack_artifact(archive, folder)
        verify_checksums(folder)
        folders[name] = folder
    verify_release_artifacts(folders, args.version, commit, tracked_source(commit))
    browser = folders["arkline-browser"]
    plan = {"version": args.version, "source_commit": commit, "release_run": args.run_id,
            "artifacts": {k: v[0]["id"] for k, v in selected.items()},
            "upgrade_servers": args.upgrade_servers, "backup_reference": args.backup_reference,
            "publication": publication_env(args.version)}
    (out / "publication-plan.json").write_text(json.dumps(plan, indent=2) + "\n")
    if not args.apply:
        print("Artifacts/source/notices verified. Plan prepared; no images, releases or deployments changed.")
        return
    publish(args, out, folders, browser, commit, plan)


def publish(args, out, folders, browser, commit, plan):
    # Fetch source into a detached worktree: builds never use the publisher's newer checkout.
    source = out / "source"
    command("git", "worktree", "add", "--detach", source, commit)
    try:
        web = json.loads(kube("-n", "arkline", "get", "deployment", "arkline-web", "-o", "json"))
        old = next(c["image"] for c in web["spec"]["template"]["spec"]["containers"] if c["name"] == "web")
        if not re.fullmatch(r"ghcr.io/pindustries/arkline-web@sha256:[a-f0-9]{64}", old):
            raise ValueError("Current web deployment must use an immutable trusted image")
        command("docker", "pull", old, capture=False)
        container = command("docker", "create", old).decode().strip()
        stage = out / "web-image"; stage.mkdir()
        try:
            for name in ["web", "downloads"]:
                command("docker", "cp", f"{container}:/srv/{name}", stage / name)
        finally:
            command("docker", "rm", container)
        version = args.version
        dest = stage / "downloads" / version
        webdest = stage / "web" / version
        if dest.exists() or webdest.exists():
            raise ValueError("Published version path exists; replacement is forbidden")
        dest.mkdir(); shutil.copytree(browser, webdest)
        for platform, target in TARGETS.items():
            folder = folders[f"arkline-downloads-{platform}"]
            for path in folder.iterdir():
                if path.name not in ["SHA256SUMS", "RELEASE.json"]:
                    shutil.copy2(path, dest / path.name)
            shutil.copy2(folder / "RELEASE.json", dest / f"RELEASE-{target}.json")
        shared = folders["arkline-source"]
        name = f"Arkline-{version}-source.tar.gz"
        shutil.copy2(shared / name, dest / name)
        shutil.copy2(shared / "RELEASE.json", dest / "RELEASE-source.json")
        wasm = dest / f"Arkline-{version}-wasm32-unknown-unknown-client.tar.gz"
        with tarfile.open(wasm, "w:gz") as archive:
            archive.add(browser, arcname=f"Arkline-{version}-wasm32-unknown-unknown-client")
        shutil.copy2(browser / "RELEASE.json", dest / "RELEASE-wasm32-unknown-unknown.json")
        shutil.copy2(source / "LICENSE", dest / "LICENSE")
        sums = {p.name: digest(p) for p in sorted(dest.iterdir())}
        (dest / "SHA256SUMS").write_text("".join(f"{value}  {name}\n" for name, value in sums.items()))
        sums["SHA256SUMS"] = digest(dest / "SHA256SUMS")
        manifest = {"version": version, "source_commit": commit, "release_run": args.run_id,
                    "platforms": sorted(TARGETS), "files": sums}
        (stage / "downloads" / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        shutil.copy2(source / "deploy/web-nginx.conf", stage / "nginx.conf")
        (stage / "release.conf").write_text(release_redirect(version))
        shutil.copy2(source / "deploy/web.Dockerfile", stage / "Dockerfile")
        prepare_public_tree(stage)
        server_image = private_image("arkline-server", commit, source, source / "deploy/Dockerfile", commit)
        web_image = private_image("arkline-web", f"{commit}-release-{args.run_id}", stage, stage / "Dockerfile", commit)
        plan.update(server_image=server_image, web_image=web_image, previous_web_image=old)
        (out / "publication-plan.json").write_text(json.dumps(plan, indent=2) + "\n")
        checks = [(f"https://downloads.arkline.network/{version}/{name}", expected) for name, expected in sums.items()]
        for name in ["index.html", "bootstrap.js", "arkline-client.js", "arkline-client_bg.wasm", "BUILD_VERSION", "RELEASE.json", "SHA256SUMS", "ThirdParty/THIRD-PARTY-NOTICES.txt", "ThirdParty/dependencies.json"]:
            checks.append((f"https://play.arkline.network/{version}/{name}", digest(browser / name)))
        deploy_verified_web(web_image, old, checks, version)
        if args.upgrade_servers:
            for name in ["arkline-test", "arkline"]:
                rollout("statefulset", name, "arkline", server_image)
            for host in ["relay", "test-relay"]:
                request = urllib.request.Request(f"https://{host}.arkline.network/health", headers={"User-Agent":"Mozilla/5.0 Arkline-Release-Validation/1.0"})
                with urllib.request.urlopen(request, timeout=20) as response:
                    if not json.load(response).get("simulation_ready"):
                        raise ValueError("World health failed; publication stopped")
        notes = out / "release-notes.txt"
        notes.write_text(f"Arkline {version}\n\nExact source: {commit}\nRelease-build run: {args.run_id}\n\nDownloads include matching vendored source and third-party notices. Windows is cross-built and Wine-tested; native Windows GUI/audio acceptance remains separate. macOS is ad-hoc signed, not notarized.\n")
        command("gh", "release", "create", f"v{version}", "--repo", REPO, "--target", commit,
                "--verify-tag", "--title", f"Arkline {version}", "--notes-file", notes, *sorted(dest.iterdir()), capture=False)
        patch_publication(plan["publication"])
        print("Public artifact hashes verified; GitHub release and Bastion download links updated. Runtime/browser acceptance remains separate.")
    finally:
        command("git", "worktree", "remove", "--force", source)


if __name__ == "__main__":
    main()
