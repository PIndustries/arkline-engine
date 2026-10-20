#!/usr/bin/env python3
"""Run this reconstruction's builds in one target with path and disk guards."""
import argparse
import datetime
import fcntl
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import threading
import time

GIB = 2 ** 30
EXPECTED = Path("/Users/thepherm/Documents/Codex/arkline-editor-reconstructed-20261020")
TOOLCHAIN = Path("/Users/thepherm/.rustup/toolchains/1.89.0-aarch64-apple-darwin/bin")


def check_workspace(root, identity=None):
    if not root.is_dir() or root.is_symlink() or root.resolve() != EXPECTED:
        raise RuntimeError("Workspace path changed or is unavailable; refusing to create a replacement")
    stat = root.stat()
    current = (stat.st_dev, stat.st_ino)
    if identity is not None and current != identity:
        raise RuntimeError("Workspace identity changed during operation")
    return current


def check_free(free, threshold):
    if free < threshold:
        raise RuntimeError(f"Free space {free / GIB:.1f} GiB is below required {threshold / GIB:.0f} GiB")


def child_environment(root):
    env = os.environ.copy()
    env.update({
        "CARGO_TARGET_DIR": str(root / ".build" / "target"),
        "CARGO_INCREMENTAL": "0",
        "CARGO_BUILD_JOBS": "1",
        "CARGO_PROFILE_DEV_DEBUG": "0",
        "CARGO_PROFILE_TEST_DEBUG": "0",
        "TMPDIR": str(root / ".build" / "tmp"),
        "PATH": str(TOOLCHAIN) + os.pathsep + env.get("PATH", ""),
        "RUSTUP_TOOLCHAIN": "1.89.0",
    })
    return env


def stop_child(process):
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-only", action="store_true")
    parser.add_argument("--timeout-seconds", type=float, default=0.0)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.timeout_seconds < 0:
        parser.error("timeout must be non-negative")
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    root = Path(__file__).resolve().parents[1]
    try:
        identity = check_workspace(root)
        check_free(shutil.disk_usage(root).free, 50 * GIB)
        if not (TOOLCHAIN / "cargo").is_file():
            raise RuntimeError("Repository-pinned local toolchain is unavailable; no download or fallback")
        env = child_environment(root)
        if args.check_only:
            print(json.dumps({"workspace": str(root), "free_gib": shutil.disk_usage(root).free / GIB,
                              "start_gib": 50, "stop_gib": 30, "target": env["CARGO_TARGET_DIR"],
                              "toolchain": str(TOOLCHAIN), "activated": False}))
            return 0
        if not command:
            raise RuntimeError("Supply a build/check/test command after --")
        if any(Path(arg).name == "package-release.py" for arg in command):
            raise RuntimeError("package-release.py overrides the guarded target and is excluded")
        for i, arg in enumerate(command):
            if arg.startswith("--target-dir=") or arg == "--target-dir":
                raise RuntimeError("Do not override the single guarded CARGO_TARGET_DIR")
        build = root / ".build"
        for path in (build, build / "target", build / "tmp", build / "logs"):
            if path.is_symlink():
                raise RuntimeError(f"Build path is a symlink: {path}")
        build.mkdir(exist_ok=True)
        with (build / "build.lock").open("a") as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise RuntimeError("Another guarded operation already owns this target")
            for path in (build / "target", build / "tmp", build / "logs"):
                path.mkdir(exist_ok=True)
            check_workspace(root, identity)
            for path in (build, build / "target", build / "tmp", build / "logs"):
                if path.stat().st_dev != identity[0]:
                    raise RuntimeError("Build target must remain on the same internal filesystem")
            check_free(shutil.disk_usage(root).free, 50 * GIB)
            stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
            log_path = build / "logs" / (stamp + ".log")
            receipt_path = build / "logs" / (stamp + ".json")
            receipt = {"command": command, "workspace": str(root), "target": env["CARGO_TARGET_DIR"],
                       "start_free_bytes": shutil.disk_usage(root).free, "start_limit_gib": 50,
                       "stop_limit_gib": 30, "toolchain": str(TOOLCHAIN),
                       "timeout_seconds": args.timeout_seconds}
            print(f"Guarded target: {env['CARGO_TARGET_DIR']}; log: {log_path}", flush=True)
            deadline = time.monotonic() + args.timeout_seconds if args.timeout_seconds else None
            process = subprocess.Popen(command, cwd=root, env=env, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT, text=True, start_new_session=True)
            with log_path.open("w") as log:
                def drain():
                    for line in process.stdout:
                        log.write(line)
                        log.flush()
                        print(line, end="", flush=True)
                reader = threading.Thread(target=drain, daemon=True)
                reader.start()
                guard_error = None
                try:
                    while process.poll() is None:
                        if deadline is not None and time.monotonic() >= deadline:
                            raise RuntimeError(f"Runtime limit of {args.timeout_seconds:g} seconds exceeded")
                        check_workspace(root, identity)
                        for path in (build, build / "target", build / "tmp", build / "logs"):
                            if path.is_symlink() or path.stat().st_dev != identity[0]:
                                raise RuntimeError("Build path changed filesystem or became a symlink")
                        check_free(shutil.disk_usage(root).free, 30 * GIB)
                        time.sleep(1)
                except (RuntimeError, KeyboardInterrupt) as error:
                    guard_error = str(error) or "Interrupted"
                    stop_child(process)
                reader.join(timeout=10)
            receipt.update({"exit_code": process.returncode, "guard_error": guard_error,
                            "end_free_bytes": shutil.disk_usage(root.parent).free})
            receipt_path.write_text(json.dumps(receipt, indent=2) + "\n")
            if guard_error:
                print("Stopped: " + guard_error, file=sys.stderr)
                return 75
            return process.returncode
    except (RuntimeError, OSError) as error:
        print("Build refused: " + str(error), file=sys.stderr)
        return 75


if __name__ == "__main__":
    sys.exit(main())
