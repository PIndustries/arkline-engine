# Pioneer short recovery milestone — 20 October 2026

Inspected the existing repository at `59e811e760d790266de6ef61e5821a496808972e`. No AGENTS.md exists in this repository or inspected ancestor chain. Initial and final pre-document Git status were clean, including untracked files: no inherited uncommitted work needed a WIP source commit. The recovery documentation is committed separately as a local WIP checkpoint. No missing directory was recreated.

## Exact validation state

- Last full workspace pass remains `5b58c7b`: `.build/logs/20261020T150639497010Z.{json,log}`, exit 0, 989 passed / 0 failed / 37 ignored across 58 result summaries. Later production changes have not had a full workspace rerun.
- Latest five-test native matrix: `.build/logs/20261020T153737034589Z.{json,log}`, exit 101, 3 passed / 2 failed. Registered docked scene, composite computer pixels/input/focus, and canonical editor/gameplay/decks/space routes passed. Enclosure failed PORT at 1080x720: central surface fraction 1, required <0.1. Forward image was emitted before port failed; remaining enclosure views/resolution were not accepted. The matrix hatch failure was normalized walk input outside [-1,1], subsequently clamped in `a4f6607`.
- Latest focused hatch run on current HEAD: `.build/logs/20261020T154104131355Z.{json,log}`, exit 101. Opening fractions and inner/outer traversal progressed, then lift arrival assertion at `apps/client/src/pioneer_route_gpu_tests.rs:1264` failed: actual floor 0.6799999999998868, expected 0.0 after fixed 12000 ms. This is no longer the original clock/seat-transition failure. Full hatch journey and return are NOT validated.
- Native evidence: `/tmp/pioneer3-native-routes-final-20261020/receipt.json`; `/tmp/pioneer3-native-enclosure-final-20261020/`; `/tmp/pioneer3-native-hatch-journey-verified-20261020/` (partial, despite directory name).

## Committed fixes confirmed by inspection

`c7e2011`, `2fa8ace`, `a4f6607`, `59e811e` are present after checkpoint `5b58c7b`. Docked/inspection material coordinates now use scene metre scale 1 versus flight scale 100. Central screen authored Y is 0.80 (lowered 2 cm); strict four-corner projectability and 2% viewport margin remain and canonical route passed after this change. Hatch test advances actual EditorWorld in bounded 50 ms ticks; hatch endpoint residue snaps within 1e-9; enclosure rays compare canonical runtime triangles. Latest test camera frames the full hatch from a supported cockpit approach. None of these changes establish finished visual acceptance.

## Processes, versions and preservation

At recovery inspection, active editor PID 19652 runs `.build/packages/pioneer-continuity-20261020/Arkline Editor Reconstructed.app/Contents/MacOS/arkline-editor --project /tmp/arkline-canonical8-native-osqa-20261020 --pilot Continuity8QA`. App plist and `package-receipt.json` confirm version 26.10.20.8, source-before-version-commit `5d1a399`; retained native-route receipt confirms registration 1. Current source registration is 3. `.build-version.state` says 26.10.20.7 and must not be treated as running app truth. Registration 2 package launch remains unconfirmed; registration 3 has no confirmed package/launch. Matching prior builds/worlds were not rebound, reset, or modified.

No surviving Cargo, rustc or guarded-build/test worker was found. Existing macOS runner service PIDs 2462/2524 remain running and were untouched. Read-only ps required sandbox escalation and succeeded. Guard --check-only reports 136.57 GiB free, single target `.build/target`, start 50 GiB / stop 30 GiB. No tests, build, packaging, app restart, MOVESPEED write, deletion/move, upload, push, publication, GitHub edit or persistent Documents permission was performed.

## Next exact step

First fix the hatch fixture's fixed 12-second lift wait: derive the time bound from actual lift geometry/travel at server LIFT_SPEED_MPS=1.0, advance in 50 ms real editor ticks until the lift reports stopped at its target, then retain exact floor/support assertions. The observed residual is consistent with insufficient travel time; confirm using actual starting floor and motion state before concluding. Re-run only `native_pioneer_animated_hatches_boarding_and_return` through `tools/guarded-local-build.py` with a fresh evidence path and bounded timeout. Then inspect the saved port enclosure image and canonical runtime hit to identify which surface blocks the declared window; retain the strict aperture assertion. Parent sequences these work items. Do not package until both failures and subsequent source validation are resolved.

## Source recovery bundle

Planned complete Git bundle: `/tmp/pioneer-short-recovery-20261020.bundle` (HEAD, all reachable history, no prerequisites), with verification output and SHA-256 saved beside it. Bundle covers this WIP recovery document and existing committed source; runtime/build evidence remains at the paths above. No inherited uncommitted work was present.
