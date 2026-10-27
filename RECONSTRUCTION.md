# Cockpit reconstruction and native build

The locally accepted physical Pioneer build is **26.10.20.7**; see [the current validation](PIONEER-DOCKED-VALIDATION-20261020.md) and [actual native OS acceptance](PIONEER-NATIVE-OS-ACCEPTANCE-20261020.md). Required local checks, private GPU tests, component viewer and the full docked physical round trip pass; the app remains open, paused and seated on its disposable QA project. The historical reconstruction below describes build 26.10.19.2. This is new integration over surviving source; missing combined builds46/47 were not fully recovered.

## Source and preservation

The baseline is `c4222513b4985918141e5447978f2c816c5a91e9`. Fourteen hash-verified stage files from recovery checkpoint `3e500c9449dee3f7390c1cde2b2fdd0b53a2c07c` were imported at `65927b5`. The four integration files—client `lib.rs`, `embedded.rs`, `flight3d.rs` and editor `main.rs`—were reconstructed against that baseline and the retained patches. Later working-copy fixes are new code, not exact recovery.

The recovery checkout, its original portable bundle and the shared runner remain clean and unchanged. The original bundle still hashes to `f83d7d43978db8b832e062a6a84d5dfac681f9401f9928eac8cd36ed114efa23`. This checkout has no remotes.

## Changes

The actual service UI renders in a persistent secondary context onto a ship-mounted 3D display. Canonical hull triangles provide matching visual and input occlusion. Display painting remains active while Shift is held. Editor text/viewport ownership gates head-look independently of paused simulation commands.

Fixed nested egui input locking on glass clicks, duplicate/replayed platform commands and the editor head-look ownership bypass. A child-process click regression has a deadline and guaranteed termination/reaping. Native testing then found that legacy `DEPTH_BITS` queries raised `INVALID_ENUM` on macOS core OpenGL. All three physical rendering passes now query the current draw attachment without rebinding it. Real tests cover color-only, depth and default targets, plus separate read/draw bindings. The Shift fixture now renders a real parent TextEdit and positively exercises scene input as well as blocked text input.

## Validation at the reconstruction checkpoint

| Local check | Result |
| --- | --- |
| Full workspace tests | 845 passed, zero failed, 29 explicitly ignored |
| Workspace/all-target Clippy with warnings denied | Passed |
| wasm32 client Clippy with warnings denied | Passed; five missing locked crates were fetched, then the offline recheck passed |
| Formatting | Passed |
| Explicit native CGL tests | All five passed for build 26.10.19.2 |
| Build-guard / packaging / publication-boundary Python tests | 5 / 8 / 15 passed |
| Native speech bridge compile, warnings treated as errors | Passed |

The five hardware checks cover depth-query safety, perspective/depth/state restoration, canonical enclosure and apertures, physical feed orientation under head-look/roll, and whole-App Shift/live glyph/telemetry/video composition. Hardware was Apple M4 Max, OpenGL 4.1 Metal91.7. The whole-App journey rendered 20 frames, retained real parent text focus, positively moved head-look outside text focus and kept received fixture snapshots unchanged.

All builds used installed Rust 1.89.0, one internal `.build/target`, one compile job, no incremental compilation and no debug information. The guard requires 50 GiB free at start and stops its own process group below 30 GiB, on path changes or at the operation deadline. The earlier hanging run was stopped; all later checks completed within their bounds. Disposable Git discovery is isolated from the parent checkout.

Detailed commands, logs, hashes, source composition and limits are in [RECONSTRUCTION-VALIDATION.json](RECONSTRUCTION-VALIDATION.json). These are local checks; no published exact-head CI run is implied.

## App and acceptance limits at the26.10.19.2 checkpoint

The app is `.build/packages/reconstruction-native-fixed-20261020/Arkline Editor Reconstructed.app`. Its local ad-hoc signature verifies. The packaged executable was built from runtime commit `dac820668fdd17bfef2f0bdba32e2663b993be1c` with BUILD_VERSION set to 26.10.19.2. The sole later code change is a wasm cfg guard on the native editor setter; native code is unchanged, and native/browser Clippy both pass afterward. Build counters are local to this new reconstruction checkout and use the repository's America/Chicago date policy.

No prior editor was running. LaunchServices returned -10810; directly launching `Contents/MacOS/arkline-editor` with explicit `--project` and `--pilot ReconstructionQA` succeeded. The owned process26743 created its disposable world and ran without startup stderr; it has since been closed normally and superseded by the current delivery. The launch receipt records its exact command.

The original pre-UI checkpoint did not establish OS input acceptance. Subsequent OS tests and remaining limits are recorded in [NATIVE-OS-ACCEPTANCE-20261020.md](NATIVE-OS-ACCEPTANCE-20261020.md); browser GPU execution remains unverified. Native rendered PNGs and JSON receipts are under that package directory. Telemetry/drone changes in the GPU tests are deterministic fixtures, not live authority measurements. Existing user projects were not opened. No push, PR, merge, deployment, external-drive write or storage migration occurred.
