# Pioneer registration 3: validated candidate and actual launch — 20 October 2026

**Now running: Arkline Editor 26.10.20.9, PID 31290.** Actual CUA OS-window title and version label both confirm `.9`; pilot `Registration3QA` uses fresh disposable project `/tmp/arkline-registration3-candidate9-osqa-20261020`. The Deck 2 scene shortcut was accepted while paused, its UI reports grounded Deck2 support and body Y=-0.32, and read-only SQLite confirms binding registration 3. The final actual OS screenshot shows the light panel joints, deck grip and service-ceiling divisions at corrected metric scale. App is left open in that Deck 2 view for inspection.

## Exact source and checks

Production source `04581a7780eb90af7fd603f5633ae6e550d857f2` passed all requested checks. Package source `9a47ca7c86805f7a74b4fea6ec3638c275f2f513` changes only BUILD_VERSION to `.9`; no production or geometry changes were made in this milestone.

| Check | Result | Log under `.build/logs/` |
|---|---|---|
| Full offline locked workspace | 989 passed / 0 failed / 37 ignored; 58 summaries | `20261020T160109750103Z.log` |
| Formatting check | passed | `20261020T160540888961Z.log` |
| Native workspace/all-target Clippy, warnings denied | passed | `20261020T160541941390Z.log` |
| wasm32-unknown-unknown client Clippy, warnings denied | passed | `20261020T160602072829Z.log` |
| Native Pioneer GPU matrix | 5 passed / 0 failed in 172.85 s | `20261020T160607166938Z.log` |
| Spatial-material Pioneer/paused-feed GPU regression | 1 passed / 0 failed in 2.98 s | `20261020T160911902211Z.log` |
| Copied editor build, offline/locked | passed, allocated 26.10.20.9 | `20261020T160927780275Z.log` |
| Copy-only local app packaging / strict ad-hoc signature | passed | `20261020T161038564331Z.log` |
| CI native Swift speech bridge compile (not run) | passed with warnings-as-errors | `20261020T161040226233Z.log` |

Offline package tests: 8 passed; build-version tests: 14 passed. Receipt `/tmp/pioneer3-validation-aux-20261020.json`. Guard receipts preserve command, fixed target, disk thresholds and success status. All builds used the single `.build/target`, start >=50 GiB / stop <30 GiB; free space remained above 135 GiB during source checks. The GPU driver retains its existing GLD_TEXTURE_INDEX_2D warning, but all assertions pass.

Native matrix retained evidence roots: `/tmp/pioneer3-candidate-native-{routes,hatches,registered,composite,enclosure}-20261020/`, each with receipts/captures. These offscreen tests cover source continuity, screen projection, input/focus and complete hatch/boarding/return. Actual desktop verification here covers launch/version and scene shortcut admission, not a repeated full OS-input hatch journey.

## Candidate artifacts

All matching package artifacts remain under `.build/packages/pioneer-registration3-candidate-20261020/`:

- `Arkline Editor Reconstructed.app`: local ad-hoc signature verified deep/strict, CFBundleVersion 2485.0.9. This is not notarized or a public release.
- `package-receipt.json`: validated/package commit IDs, version, registration and binary hashes.
- `pioneer-registration3-candidate-source.bundle`: verified complete Git history, no prerequisites, exact package commit 9a47ca7. SHA-256 and verification sidecars retained.
- `native-launch-verification.json`: actual window observation, runtime PID/command, QA path, saved authority binding/support and old-world preservation result.
- `launch-request.json`, `validation-summary.json`, `local-package.py`: exact launch request, checks/receipts and reproducible copy-only packaging script. Signed app Resources/SOURCE_MANIFEST.json binds it to the source/registration.

A separate complete post-report recovery bundle `/tmp/pioneer-registration3-launched-recovery-20261020.bundle` preserves this documentation commit; the package's own source bundle continues to identify exactly the built source.

## Compatibility and preservation

Old registration-1 build `.8` remains intact at `.build/packages/pioneer-continuity-20261020/Arkline Editor Reconstructed.app`. Its QA world `/tmp/arkline-canonical8-native-osqa-20261020` retains identical saved world JSON SHA-256 `b7398997db395ec52c3835569805f1978ab3c1145e0cfd44cb58fe7675e1a5f0` before and after the swap. All other earlier build/world directories were left untouched. Registration-2 package launch is still not established; keep each older saved world with its matching build. The passing previous-registration test exercises registration-2 rejection without rewriting durable JSON; no migration/rebinding/reset was performed.

CUA accessibility observation briefly timed out. Later observation of a just-closed `.8` app reopened it with default arguments, so closure was repeated normally and process exit checked separately before the explicit `.9` launch. The original registration-1 QA JSON remained unchanged. Final process inspection confirms one editor: `.9` with the fresh QA arguments; no Cargo/guard worker remains. No forced process kill, whole-Documents permission, MOVESPEED edit, asset/geometry change, deletion/move, upload, publication or GitHub edit occurred.

## Visible changes and unfinished work

The candidate now makes the previously unlaunched deeper fitted cockpit/workstation, central display adjustment, metric panel/grip/ceiling finish, retained moving hatch leaves, fitted galley furniture, sprint and confirmed-displacement gait available in the app. Only Deck 2 placement/finish was visually checked in this actual-window milestone; the other behaviors have the native regression evidence above.

The current screenshot still looks flat and repetitive. Material response, construction depth, purposeful lighting/equipment, broader room roles and exterior v05 fidelity remain unfinished. This validation milestone adds no new visual design. Next: deliberate visual-fidelity work using the retained reference assessment, with actual `.9` inspection as the new baseline. No blocker remains for that work.
