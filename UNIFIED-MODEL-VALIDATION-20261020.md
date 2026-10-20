# Unified native model browser

Build **26.10.20.3** is open as **PID 7924** on the preserved ReconstructionQA project. Select **Models** in the primary workspace bar. Search or choose a category, select a model, then left-drag to orbit, right/middle-drag to pan, scroll to zoom, **Fit model** or **Reset view**. Import management is available from the same browser.

[Open the local app](<.build/packages/unified-model-viewer-20261020/Arkline Editor Reconstructed.app>) · [Evidence and hashes](UNIFIED-MODEL-VALIDATION-20261020.json)

## Coverage and behavior

The catalog contains **111 runtime entries: 92 geometry previews and 19 entries with explicit unavailable reasons**. Entries include assemblies, variants and material groups; these counts are not unique mesh asset counts. Ships, drones, station, hangars, interiors, cockpit enclosures, equipment, furniture, characters, exterior fixtures and world geometry use their actual shared runtime recipes and per-draw materials.

Repository and project model files, saved imported blobs, accepted/staged imports and guarded Files snapshots use the same browser and camera. The current project window shows **118 entries**, including four repository GLB paths and three Blender sources. Distinct paths remain choices even when their bytes match. Unsupported or failed selections remain visible and clear the previous preview. Browsing does not accept, save, apply or replace staged work; explicit staging preserves a different existing candidate.

The bounded worker keeps one active job and the latest waiting selection, discards stale results, caches three decoded choices and uploads only on selection completion. Discovery is bounded to 2,048 visited entries, depth 8 and 256 model candidates; GLBs are limited to 16 MiB. Refresh reports partial scans. Case aliases and overlapping roots are deduplicated without collapsing different real source paths.

Planet and terrain entries show actual triangles with plain inspection materials. Separate room records, shared-pane windows and logical equipment without isolated geometry retain explicit unavailable reasons. Blender and other unsupported formats stay in the list. The calibration GLB has no normals and reports `Missing ship normals`. Textures, animation, skins, morphs and external resources are not imported by the existing static-geometry path.

## Saved source and validation

Production source is **57fad16f8727cd254d99f70ed3c60416d60fa549**: client catalog/camera **2257bde**, editor browser **40b6ceb**, bounded worker message **edc6654**, and the build stamp **57fad16**. The implementation is checkpointed locally; no push or publication occurred.

The full workspace passed at that clean production commit before and after the run: **879 passed, zero failed, 31 ignored across 55 top-level suites**. Nested process helper reports are excluded from that total. Formatting, native/all-target Clippy and browser-target client Clippy passed with warnings denied. These are local results, not published exact-head CI.

All **eight named native checks passed for 26.10.20.3**. The shared preview rendered all 92 available runtime entries with matching triangle/batch counts, material uniforms, CPU hashes and retained GPU buffer identities. Eight real egui pointer journeys cover Pioneer, all three drones, station, ore asteroid and two imported fixtures, including pan, orbit, zoom, Fit, Reset, narrow framing, switching and clearing. The other checks cover drone geometry, cockpit draw-target depth, display perspective/state, enclosure apertures, physical camera-feed orientation, whole-App Shift/feed composition and the existing material gallery. They run on Apple M4 Max, OpenGL 4.1 Metal 91.7 in private CGL contexts.

A ninth explicit native check reads all four actual repository GLBs through the imported preview path. All three Pioneer source paths render with verified bounds, all nine scalar-material batches and unchanged source bytes: **103,466 / 78,246 / 103,466 triangles**. The current Pioneer and study05 paths have identical content; the earlier study differs. Calibration fails with its exact missing-normals reason and zero stale geometry. This test-only addition is saved at **607064d78eda54454457567b1f2f2a11ac96e978**; shipped production code remains57fad16. Client/all-target Clippy and formatting also passed after the test addition.

The separate actual owned-window capture confirms the new version, discoverable Models path, 118-entry list and actual 3D preview. Subsequent attempted OS control journeys encountered concurrent navigation into Rooms. Automation stopped to avoid interfering with current use. The files named `pioneer-default.png`, `pioneer-orbit.png` and `pioneer-pan.png` show Rooms/play views and provide **no model control acceptance**. OS search/category/control/resize checks remain incomplete. No keyboard text was injected in this phase.

## Package and preservation

The local app has a verified deep/strict ad-hoc signature and CFBundleVersion **2485.0.3**. Packaged executable SHA-256:

`e76bfecdf411f2178a11b059510b9b66ac646241d928cd443ef3733c7fd96a8c`

Old PID 9337 closed through its normal window control before ordinary `open -a` launched PID 7924 on the same disposable QA project. No process was forced closed. Previous packages, original recovery sources and the shared runner remain retained. All builds use one guarded internal target with deadlines and disk/path checks; about 204 GiB remained free after packaging.

The Library attachment was materialized through its supported Library identity, its actual pixels were viewed, and its metadata and SHA-256 were verified locally. No screenshots, packages or artifacts were uploaded. There were no external-drive writes, moves or deletions, and no PRs, merges or deployments. The source remains a new reconstruction over retained inputs, not an exact recovery of the missing combined46/47 checkout.
