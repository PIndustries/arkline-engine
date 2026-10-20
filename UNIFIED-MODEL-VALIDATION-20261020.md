# Unified native model browser

Build **26.10.20.4** is packaged and validated at **ce8080b0be0fe66e7ccbd8961635a9b032840bc7**. It is **running as PID 96422** after the user authorized discarding the previous unsaved session. The earlier **26.10.20.3** app, PID **7924**, closed normally and is absent. The saved ReconstructionQA project was preserved.

[Prepared app](<.build/packages/unified-all-models-20261020/Arkline Editor Reconstructed.app>) · [Evidence and hashes](UNIFIED-MODEL-VALIDATION-20261020.json)

## Coverage and behavior

The runtime catalog now has **111 entries: 108 geometry previews and three logical equipment endpoints with explicit unavailable reasons**. Counts include assemblies, variants and material groups. All twelve rooms have cutaways using their existing walls, doorways, storage and equipment. Shared floors, ceilings, corridor and access-core context are omitted. Four window previews select exact **36-corner / 12-triangle** ranges from the original Navigation glass buffer, with distinct source bounds and their existing materials. These inspection views do not establish fitted hull interiors or continuous ship-to-station walking.

All **four repository GLB paths** render through the shared imported preview. The three Pioneer sources retain **103,466 / 78,246 / 103,466 triangles** and nine scalar-material batches each. Canonical Pioneer and study05 currently contain identical bytes at distinct paths. Calibration renders its actual twelve triangles using explicitly generated flat normals for preview only; strict import still returns `Missing ship normals`. That preview cannot be staged, accepted or saved, and original asset bytes remain unchanged. Degenerate source geometry is rejected and clears the old preview.

The single Models browser retains search, categories, source identity and import management. Left-drag orbits, right/middle-drag pans, scroll zooms, **Fit model** retains direction while restoring framing, and **Reset view** restores the original camera. Browsing preserves accepted/staged work. Blender sources remain listed with an unsupported-format reason. The bounded worker, latest-selection handling, three-choice decode cache and scan limits remain in place. Planet and terrain inspection uses actual triangles with plain inspection materials; static GLB texture, animation, skin, morph and external-resource limits remain.

## Saved source and validation

The production checkpoints are **2c7e975** for shared room projection, **ef89e7d** for room cutaways and authored window ranges, **53e8400** for explicit preview normals, and **ce8080b** for the 26.10.20.4 build stamp.

[Final-head checks](.build/packages/unified-all-models-20261020/final-head-checks.json) record a clean **ce8080b** before and after: **902 passed, zero failed, 32 ignored across 55 top-level suites**. Formatting, workspace/all-target Clippy and wasm client Clippy passed with warnings denied. These are local results; no published exact-head CI is claimed.

The original five-test hang was stopped and reaped (guard receipt `20261020T041935947018Z.json`, child exit -15). All five named interaction tests now pass in the final workspace log after the saved input-lock correction. New guarded subprocesses have explicit deadlines.

All **nine explicit native GPU checks passed for 26.10.20.4**. The shared preview rendered all 108 available runtime entries and completed **24 actual egui pointer journeys**, including every new room and window, with orbit, pan, zoom, Fit, Reset and narrow framing. Checks retain triangle/batch counts, runtime material uniforms, source hashes and GPU buffer identities. Window fill and wireframe both use the selected source range. The repository-file check reads all four real GLBs; calibration GPU readback verifies 36 finite unit normals following triangle winding and bounds matching its report. Drone, cockpit depth/display/enclosure, physical feed, Shift composition and material-gallery regressions also passed. Evidence uses private CGL contexts on Apple M4 Max, OpenGL 4.1 Metal 91.7.

[Native batch](.build/packages/unified-all-models-20261020/native-final-batch.json) · [Shared preview receipt](.build/packages/unified-all-models-20261020/shared-preview-gpu/receipt.json) · [Repository GLB receipt](.build/packages/unified-all-models-20261020/repository-glbs-gpu/receipt.json)

## Desktop status and historical evidence

A passive startup check verified one owned window titled **Arkline Editor — 26.10.20.4**, its exact packaged executable and preserved project arguments. No focus or input was changed. The passive screenshot caught a Stage Manager thumbnail and is excluded from usable-canvas acceptance. There is **no new OS desktop interaction acceptance for 26.10.20.4**. GPU pointer fixtures do not establish app focus or OS event delivery. The earlier 26.3 capture establishes only that build's title, Models navigation/list and a visible preview. Its later `pioneer-default.png`, `pioneer-orbit.png` and `pioneer-pan.png` files show Rooms/play after concurrent navigation; they are excluded from model-control acceptance. Search, category, control and resize acceptance for the new app remains unverified at the OS-input level; the GPU and egui fixture results above remain separate.

The complete historical 26.3 record is retained under `historical_builds` in the evidence JSON, and its original package receipts remain untouched. Historical coverage and launch observations are not carried forward as current-build results.

## Package and preservation

The prepared app has a verified deep/strict local ad-hoc signature and CFBundleVersion **2485.0.4**, as recorded in its [package receipt](.build/packages/unified-all-models-20261020/package-receipt.json). Packaged executable SHA-256:

`f8cad8f37c40e9a859c49145c7f265a7dcfa17f0b93894e64ef3c8454437ce88`

The source remains a reconstruction over retained inputs. It is not an exact recovery of the missing combined checkout. The current package directory also holds the verified source bundle and its checkpoint receipt.
