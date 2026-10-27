# Pinned formatting quality gate — 24 October 2026

The strict-quality checkpoint c98da6af89e7548bcfe0eae60625a56790cfa377 was
privately preserved before formatting. Isolated source commit
4bd0a0967c102257c2ce22670e18cffcdcd68021 applies only the pinned Rust1.89
`cargo fmt --all`: 47 Rust files, no assets or behavior changes.

An independent restore of the exact pre-format checkpoint followed by that same
formatter matches every one of the 1,157 present tracked files byte-for-byte.
The eight unavailable historical proof blobs remain referenced unchanged.
Canonical Blender/GLB bytes, registration 11 and world compatibility are unchanged.

## Terminal validation

- Full workspace tests: 1,263 passed, 0 failed, 56 ignored across 58 top-level
  summaries; three nested subprocess summaries are not double-counted.
- Client 566/0/45; editor 297/0/1.
- All-target check: exit 0.
- Strict all-target Clippy (`-D warnings`): exit 0.
- Pinned formatter check: exit 0.
- Actual canonical-source normals/winding and opaque aperture tests pass within
  the full suite; source/asset byte identity independently verified.

This is a local quality gate, not runtime publication, deployment or a claim that
ignored native/macOS/release-hardware suites ran. The prior strict checkpoint
contains the eight inspected native seat/walk/reseat/departure frames. No source
semantics changed here, so that evidence is retained rather than relabeled as a
new feature capture. No public tracking items were created or reopened.

Next authorized separate increment: consistent departure-only cargo reservation
explanation and gating across all six departure surfaces, retaining server
authority, STOP/HOLD controls, legal walking and transfer Cancel/Resume.
