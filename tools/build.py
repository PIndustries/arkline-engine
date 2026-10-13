#!/usr/bin/env python3
"""Allocate a unique local build identity, then run Cargo under the same lock.

Use this for every Cargo operation that can produce a binary. The lock stays
held until Cargo exits so another wrapped build cannot change BUILD_VERSION
while rustc is embedding it.
"""
import argparse
from datetime import date, datetime
import fcntl
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import shutil
from zoneinfo import ZoneInfo

ROOT = Path(__file__).resolve().parent.parent
VERSION_RE = re.compile(r"(\d{2})\.(\d{1,2})\.(\d{1,2})\.(\d+)$")


def allocate(version_file: Path, lock_file: Path, today: date) -> str:
    lock_file.parent.mkdir(parents=True, exist_ok=True)
    with lock_file.open("a+") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        return _allocate_locked(version_file, today)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cargo_args", nargs=argparse.REMAINDER,
                        help="Cargo command and arguments, for example build --workspace")
    parser.add_argument("--artifact-copy", type=Path,
                        help="copy a Cargo-reported executable to this path before releasing the build lock")
    parser.add_argument("--artifact-dir", type=Path,
                        help="copy all Cargo-reported executables here before releasing the build lock")
    parser.add_argument("--version-output", type=Path,
                        help="write the allocated identity to this file before releasing the build lock")
    parser.add_argument("--repo-root", type=Path, default=ROOT,
                        help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not args.cargo_args:
        parser.error("provide a Cargo command (build, test, check, clippy, run, ...) ")
    command = args.cargo_args[1:] if args.cargo_args[0] == "--" else args.cargo_args
    repo_root = args.repo_root.resolve()
    version_file = repo_root / "BUILD_VERSION"
    lock_file = repo_root / ".build-version.lock"
    lock_file.touch(exist_ok=True)
    with lock_file.open("r+") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        today = datetime.now(ZoneInfo("America/Chicago")).date()
        # Allocate while holding the process-wide lock; keep it through Cargo.
        version = _allocate_locked(version_file, today)
        print(f"Arkline build {version}", file=sys.stderr, flush=True)
        result = subprocess.run(["cargo", *command], cwd=repo_root, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        if args.artifact_copy or args.artifact_dir:
            if result.returncode:
                sys.stdout.write(result.stdout)
                return result.returncode
            artifacts = []
            for line in result.stdout.splitlines():
                try:
                    event = json.loads(line)
                except ValueError:
                    continue
                if (event.get("reason") == "compiler-artifact"
                        and "bin" in event.get("target", {}).get("kind", [])
                        and event.get("executable")):
                    artifacts.append((event.get("target", {}).get("name"), Path(event["executable"])))
            if args.artifact_copy:
                artifacts = [item for item in artifacts if item[0] == "arkline-editor"]
            if (len(artifacts) != 1 if args.artifact_copy else not artifacts) or any(not path.is_file() for _, path in artifacts):
                sys.stdout.write(result.stdout)
                print("Could not identify the requested Cargo executable artifact(s)", file=sys.stderr)
                return 1
            if args.artifact_copy:
                args.artifact_copy.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(artifacts[0][1], args.artifact_copy)
            if args.artifact_dir:
                args.artifact_dir.mkdir(parents=True, exist_ok=True)
                for name, path in artifacts:
                    shutil.copy2(path, args.artifact_dir / path.name)
        if args.version_output and result.returncode == 0:
            args.version_output.parent.mkdir(parents=True, exist_ok=True)
            args.version_output.write_text(version + "\n")
        sys.stdout.write(result.stdout)
        return result.returncode


def _allocate_locked(version_file: Path, today: date) -> str:
    previous = version_file.read_text().strip() if version_file.exists() else ""
    match = VERSION_RE.fullmatch(previous)
    sequence = 1
    if match:
        old_year, old_month, old_day, old_sequence = map(int, match.groups())
        if (2000 + old_year, old_month, old_day) == (today.year, today.month, today.day):
            sequence = old_sequence + 1
    version = f"{today.year % 100:02}.{today.month}.{today.day}.{sequence}"
    temporary = version_file.with_name(version_file.name + f".{os.getpid()}.tmp")
    temporary.write_text(version + "\n")
    os.replace(temporary, version_file)
    return version


if __name__ == "__main__":
    sys.exit(main())
