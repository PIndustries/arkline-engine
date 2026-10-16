#!/usr/bin/env python3
"""Build reviewable release archives. Does not upload or publish anything."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def run(*args, cwd=ROOT, capture=False):
    return subprocess.run(args, cwd=cwd, check=True, text=True, encoding="utf-8",
                          stdout=subprocess.PIPE if capture else None).stdout


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def archive(directory, output, windows=False):
    if windows:
        with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED, compresslevel=6, strict_timestamps=False) as handle:
            for path in sorted(directory.rglob("*")):
                if path.is_file():
                    handle.write(path, path.relative_to(directory.parent))
    else:
        with tarfile.open(output, "w:gz", compresslevel=6) as handle:
            handle.add(directory, arcname=directory.name)


def copy_source(destination):
    # Git's ignore rules exclude saves, credentials, caches and compiler output.
    files = run("git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", capture=True)
    for name in files.split("\0"):
        if not name:
            continue
        source = ROOT / name
        relative = Path(name)
        if relative.parts[0] in {".git", "data", "target"}:
            raise RuntimeError(f"Refusing runtime/build data in source: {name}")
        if source.is_file():
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)


def notices(source, destination):
    metadata = json.loads(run("cargo", "metadata", "--locked", "--offline",
                              "--format-version", "1", cwd=source, capture=True))
    packages = sorted((p for p in metadata["packages"] if p["source"]),
                      key=lambda p: (p["name"], p["version"]))
    lines = ["Arkline — third-party dependency notices", "",
             "This inventory covers the locked workspace across all platform targets, including",
             "build dependencies. It is a superset of the code linked into each binary.",
             "Declared license expressions are reproduced, not relicensed. Original license",
             "and notice files are collected below; complete dependency source is in the",
             "matching source archive. This inventory is not a legal compatibility opinion.", ""]
    inventory = []
    for package in packages:
        label = f'{package["name"]}-{package["version"]}'
        directory = Path(package["manifest_path"]).parent
        files = []
        for path in sorted(directory.rglob("*")):
            # Include bundled-code and asset notices, not only top-level Rust licenses.
            if path.is_file() and (re.search(r"(^|[-_.])(license|licence|copying|copyright|notice|ofl|ufl|authors)([-_.]|$)", path.name, re.I)
                                  or (path.suffix.lower() in {".txt", ".md"} and "fonts" in path.parts)):
                relative = path.relative_to(directory)
                target = destination / "licenses" / label / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, target)
                files.append(str(relative))
        declared = package.get("license") or "See package source/license-file"
        license_file = package.get("license_file")
        if license_file:
            path = directory / license_file
            if path.is_file() and license_file not in files:
                target = destination / "licenses" / label / license_file
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, target)
                files.append(license_file)
        inventory.append({"name": package["name"], "version": package["version"],
                          "license": declared, "repository": package.get("repository"),
                          "notice_files": files})
        lines.extend([f"{label}: {declared}", f"  {package.get('repository') or package['source']}",
                      f"  Notices: licenses/{label}/" if files else "  No separate notice file in crate; see complete source."])
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "THIRD-PARTY-NOTICES.txt").write_text("\n".join(lines) + "\n")
    (destination / "dependencies.json").write_text(json.dumps(inventory, indent=2) + "\n")


def build_plan(host, system, windows_cross=False):
    if windows_cross:
        if system != "Linux" or not host.startswith("x86_64-"):
            raise ValueError("Windows cross packaging requires an x86_64 Linux host")
        target = "x86_64-pc-windows-msvc"
        return target, True, ["cargo", "xwin", "build", "--target", target], Path(target) / "release", ["wine"]
    return host, system == "Windows", ["cargo", "build"], Path("release"), []


def source_manifest(version, commit, dirty):
    return {"build_version": version, "source_commit": commit,
            "uncommitted_source": dirty, "artifact_kind": "source",
            "source_archive": f"Arkline-{version}-source.tar.gz",
            "license": "AGPL-3.0-only"}


def write_checksums(output):
    checksums = [f"{sha256_file(path)}  {path.name}"
                 for path in sorted(output.iterdir())
                 if path.is_file() and path.name != "SHA256SUMS"]
    (output / "SHA256SUMS").write_text("\n".join(checksums) + "\n")


def archive_source(directory, output):
    """Stable source bytes: sorted paths, fixed timestamps and portable modes."""
    with output.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as bundle:
                for path in [directory, *sorted(directory.rglob("*"))]:
                    info = bundle.gettarinfo(path, arcname=str(path.relative_to(directory.parent)))
                    if not (info.isdir() or info.isfile()):
                        raise ValueError("Source archive links/devices are not permitted")
                    info.uid = info.gid = info.mtime = 0
                    info.uname = info.gname = ""
                    info.mode = 0o755 if info.isdir() or info.mode & 0o111 else 0o644
                    if info.isfile():
                        with path.open("rb") as stream:
                            bundle.addfile(info, stream)
                    else:
                        bundle.addfile(info)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="New or empty directory outside the repository")
    parser.add_argument("--allow-dirty", action="store_true", help="Local review only; mark source as uncommitted")
    parser.add_argument("--windows-cross", action="store_true", help="Linux x64 cargo-xwin build; Wine compatibility smoke, not native Windows QA")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--source-only", action="store_true", help="Only the shared vendored source; no native binaries")
    mode.add_argument("--binaries-only", action="store_true", help="Native archives referencing the separately built shared source")
    args = parser.parse_args()
    if args.source_only and args.windows_cross:
        parser.error("--source-only is platform-neutral; omit --windows-cross")
    output = args.output.resolve()
    if output == ROOT or ROOT in output.parents:
        parser.error("Output must be outside the repository")
    if output.exists() and any(output.iterdir()):
        parser.error("Output directory must be empty (prevents mixing versions)")
    dirty = bool(run("git", "status", "--porcelain", capture=True).strip())
    if dirty and not args.allow_dirty:
        parser.error("Commit the release source first, or use --allow-dirty for local review only")
    version = (ROOT / "BUILD_VERSION").read_text().strip()
    if not re.fullmatch(r"\d{2}\.\d{1,2}\.\d{1,2}\.\d+", version):
        parser.error("Invalid BUILD_VERSION")
    if not args.source_only:
        target = re.search(r"^host: (.+)$", run("rustc", "-vV", capture=True), re.M).group(1)
        system = platform.system()
        if system not in {"Darwin", "Linux", "Windows"}:
            parser.error(f"Unsupported packaging host: {system}")
        try:
            target, windows, build_command, binary_subdir, executable_runner = build_plan(target, system, args.windows_cross)
        except ValueError as error:
            parser.error(str(error))
        suffix = ".zip" if windows else ".tar.gz"
        extension = ".exe" if windows else ""
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="arkline-release-") as temporary:
        stage = Path(temporary)
        source_name = f"Arkline-{version}-source"
        source = stage / source_name
        source.mkdir()
        copy_source(source)
        # Vendor the complete lockfile, including platform-specific and bundled C sources.
        config = run("cargo", "vendor", "--locked", "vendor", cwd=source, capture=True)
        (source / ".cargo").mkdir(exist_ok=True)
        config_path = source / ".cargo/config.toml"
        if config_path.exists():
            raise RuntimeError("Review existing Cargo configuration before replacing it")
        config_path.write_text(config)
        third_party = stage / "ThirdParty"
        notices(source, third_party)
        shutil.copytree(third_party, source / "ThirdParty")
        source_record = source_manifest(version, run("git", "rev-parse", "HEAD", capture=True).strip(), dirty)
        (source / "RELEASE.json").write_text(json.dumps(source_record, indent=2) + "\n")
        if not args.binaries_only:
            archive_source(source, output / source_record["source_archive"])
        if args.source_only:
            shutil.copy2(source / "RELEASE.json", output / "RELEASE.json")
            write_checksums(output)
            print(f"Prepared shared source {output}; no binaries built or artifacts published.")
            return
        manifest = {"build_version": version, "source_commit": run("git", "rev-parse", "HEAD", capture=True).strip(),
                    "uncommitted_source": dirty, "target": target,
                    "rustc": run("rustc", "-V", capture=True).strip(),
                    "source_archive": source_name + ".tar.gz",
                    "build_host": platform.system(),
                    "validation_runtime": "Wine compatibility only; native Windows GUI/audio not tested" if args.windows_cross else "native host",
                    "license": "AGPL-3.0-only", "apple_signing": "ad-hoc; not notarized" if system == "Darwin" else None}
        (stage / "RELEASE.json").write_text(json.dumps(manifest, indent=2) + "\n")
        # Keep the native receipt inside signed app bundles.
        shutil.copy2(stage / "RELEASE.json", source / "RELEASE.json")
        # Compile from the exact snapshot that will ship, with vendored dependencies.
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = str(ROOT / "target")
        subprocess.run(build_command + ["--workspace", "--locked", "--offline", "--release"], cwd=source, env=env, check=True)
        binaries = ROOT / "target" / binary_subdir
        client = stage / f"Arkline-{version}-{target}-client"
        server = stage / f"Arkline-{version}-{target}-server"
        client.mkdir()
        server.mkdir()
        if system == "Darwin":
            # Script keeps the same source snapshot and reuses the compiled target directory.
            subprocess.run(["bash", str(source / "tools/package-macos.sh"), str(client), "release"], cwd=source, env=env, check=True)
        else:
            for binary in ["arkline-client", "arkline-server", "arkline", "arkline-verifier"]:
                shutil.copy2(binaries / (binary + extension), client / (binary + extension))
        for binary in ["arkline-server", "arkline", "arkline-verifier"]:
            shutil.copy2(binaries / (binary + extension), server / (binary + extension))
        for directory in [client, server]:
            for name in ["LICENSE", "TRADEMARKS.md", "BUILD_VERSION", "README.md"]:
                shutil.copy2(source / name, directory / name)
            shutil.copytree(source / "docs", directory / "docs", dirs_exist_ok=True)
            shutil.copytree(third_party, directory / "ThirdParty")
            shutil.copy2(stage / "RELEASE.json", directory / "RELEASE.json")
        (client / "START-HERE.txt").write_text(
            "Arkline client\n\n" + ("Open Arkline.app.\n"
            "This build is ad-hoc signed, not Apple notarized.\n" if system == "Darwin" else
            "Run arkline-client" + extension + ".\n") +
            "The client defaults to the official production relay over encrypted WSS.\n"
            "Use the connection screen to choose Production, Test, or a custom server.\n"
            "Production and Test have separate pilots and saved worlds.\n"
            "Use --local to start a private local universe; use --server wss://HOST/ws for another relay.\n"
            "Windows requires the x64 Microsoft Visual C++ v14 runtime and Universal CRT.\n"
            "See docs/ for controls, network limits, saved data and server operation.\n"
            "The matching source archive named in RELEASE.json includes vendored dependencies.\n")
        (server / "START-HERE.txt").write_text(
            "Arkline server\n\nRun arkline-server --bind 127.0.0.1:7777 --db world.sqlite\n"
            "Keep the database and credentials outside this extracted download.\n"
            "Raw TCP is local/private only. For browser or public access use the optional WebSocket\n"
            "listener behind a trusted HTTPS reverse proxy or tunnel. Never expose plaintext tokens.\n"
            "See deploy/ in the matching source and docs/ for limits and source-license obligations.\n")
        for binary in ["arkline-server", "arkline", "arkline-verifier"]:
            run(*executable_runner, str(server / (binary + extension)), "--help", cwd=server, capture=True)
        for directory in [client, server]:
            archive(directory, output / (directory.name + suffix), windows)
        shutil.copy2(stage / "RELEASE.json", output / "RELEASE.json")
    write_checksums(output)
    print(f"Prepared {output}; no artifacts were published.")


if __name__ == "__main__":
    main()
