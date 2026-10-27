#!/usr/bin/env python3
"""Run the isolated imported-mesh proof without a desktop or saved world."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TEST = "flight3d::imported_asset::mesh_proof_tests::native_authored_forward_bay_proof"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--telemetry", required=True, type=Path,
                        help="Previously confirmed recorded values; never a live connection")
    parser.add_argument("--output", required=True, type=Path,
                        help="New evidence directory; existing paths are refused")
    args = parser.parse_args()
    telemetry = args.telemetry.resolve(strict=True)
    output = args.output.absolute()
    if output.exists() or output.is_symlink():
        parser.error("--output must be a new path")
    data = json.loads(telemetry.read_text())
    for name in ("charge_j", "ore"):
        if type(data.get(name)) is not int or not 0 <= data[name] <= 2**64 - 1:
            parser.error(f"recorded {name} must be an unsigned 64-bit integer")
    if not isinstance(data.get("location_kind"), str) or not data["location_kind"].strip():
        parser.error("recorded location_kind must be a nonempty string")
    study = ROOT / "assets/studies/pioneer-forward-bay-proof"
    env = dict(os.environ, ARKLINE_FORWARD_BAY_GLB=str(study / "forward-bay-proof.glb"),
               ARKLINE_FORWARD_BAY_MANIFEST=str(study / "manifest.json"),
               ARKLINE_FORWARD_BAY_TELEMETRY=str(telemetry),
               ARKLINE_FORWARD_BAY_EVIDENCE=str(output))
    source = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT))
    command = ["cargo", "test", "--locked", "-p", "arkline-client", "--lib", TEST,
               "--", "--ignored", "--exact", "--nocapture", "--test-threads=1"]
    subprocess.run(command, cwd=ROOT, env=env, check=True)
    # Requiring a receipt also catches an incorrect test filter running zero tests.
    receipt = output / "receipt.json"
    receipt.read_bytes()
    (output / "run.json").write_text(json.dumps({
        "source_commit": source, "source_dirty": dirty, "command": command,
        "receipt_sha256": hashlib.sha256(receipt.read_bytes()).hexdigest(),
        "scope": "Isolated study using recorded values; no live world or visual acceptance",
    }, indent=2) + "\n")
    print(f"Isolated proof evidence: {output}")


if __name__ == "__main__":
    main()
