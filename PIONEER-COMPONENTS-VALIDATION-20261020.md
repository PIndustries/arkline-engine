# Pioneer components validation

Build **26.10.20.5** is packaged and locally signed; **it has not been launched**. Build **26.10.20.4**, PID **96422**, remains open per the root checkpoint. The full suite and all ten exact-head native checks passed. Saved-work readiness and actual app acceptance remain pending.

## Code and package

- `8558dd7`: source-object catalog, grouped Models hierarchy and exact ship context.
- `80689ba`: **Focus piece / Fit ship** and same-ID import metadata/availability refresh.
- `2e2e6fc`: version stamp; clean, unchanged head for final checks.

[Package receipt](.build/packages/pioneer-components-20261020/package-receipt.json) records bundle `2485.0.5`, a debug build with debug information and incremental compilation disabled, and deep/strict verification of the local ad-hoc signature. The [artifact build](.build/packages/pioneer-components-20261020/artifact-build-check.json) passed from `80689ba` with the 26.5 stamp subsequently committed as `2e2e6fc`.

## Find and inspect a piece

Open the **Models** workspace tab, then **Ships → Pioneer → Source parts → name-based family → exact source object name**. Pioneer and Source parts open by default; branch counts, breadcrumbs, selected-row emphasis and search reveal the individual pieces. Rooms, decks, equipment, other catalogs, project/repository files and import management remain available.

**Isolate selected** starts each selection with its original materials. **Highlight in ship** uses the verified shared source frame. In that mode, **Focus piece** frames a tiny selected part without changing its placement; **Fit ship** returns to the full ship. Orbit, pan, wheel zoom, Fit model and Reset view remain available. The Focus/Fit CPU control tests and extended native GPU journey passed. The saved fastener Focus piece image visibly shows substantial selected geometry; Fit ship restores the full context.

## Confirmed evidence

- Catalog: **797 runtime entries, 794 available geometric entries, 3 logical unavailable entries**; **686 exact Pioneer source objects**, **103,466 triangles**, **9 material primitives**. Project/file counts are additional and project-dependent.
- [Focused checks](.build/packages/pioneer-components-20261020/focus-cache-checks.json): **23 client tests passed**, 4 ignored; **13 editor browser tests passed**, none failed. Counts come from their linked logs. This receipt does not record a source HEAD and is not final-head acceptance.
- [Final-head checks](.build/packages/pioneer-components-20261020/final-head-checks.json): fmt check, native workspace/all-target Clippy and wasm client Clippy with warnings denied passed on clean, unchanged `2e2e6fc`. The full workspace suite passed: **920 passed, 0 failed, 33 ignored across 55 top-level suites**.
- [Initial native acceptance](.build/packages/pioneer-components-20261020/components-native-check.json), at `8558dd7`: **11 actual source objects across all 9 materials**, shared callback/control journeys and unchanged GPU payloads. [Detailed evidence](.build/packages/pioneer-components-20261020/components-gpu/receipt.json) separates all-686 CPU receipt coverage from representative GPU coverage. It does not validate the later Focus/Fit GPU path or OS/app focus.
- [Extended native acceptance](.build/packages/pioneer-components-20261020/native-final-batch.json), at clean `2e2e6fc`: **1 passed**, test duration **40.75 s**, with schema-2 [component evidence](.build/packages/pioneer-components-20261020/components-gpu-final/receipt.json). Verified **60 PNG files**, with every receipt capture reference present. All 11 representative objects and 9 materials passed; the fastener journey retained source placement, enlarged its geometry through Focus piece, and restored exact whole-context pixels through Fit ship/Reset. Saved fastener Focus pixels were visually inspected. The other nine native checks also passed: original 111-entry catalog scope (108 geometric previews and 24 control journeys), all four real repository GLBs, drone geometry, cockpit depth/display/enclosure, physical feeds, Shift composition and material-gallery source invariance.

## Provenance and remaining acceptance

Source objects preserve exact exported names/ranges; folders group names and are **not authored assemblies**. Material batches can contain disconnected objects. Snapshot IDs bind source hash and exact name, rather than durable cross-revision engineering identities. Deck-local rooms and station associations do not establish physical fit inside the exterior hull. Context mode retains all nine source ship batches. No physical-model, engineering, gameplay/walking, hidden-geometry-cost or frame-time improvement claim is made.

Still required: build 26.5 launch after saved-work readiness, and actual editor selection/context, large-catalog scrolling and disk Reload recovery journeys. The import regressions exercise the browser snapshot boundary. External file edits require Refresh to replace cached file previews.

Validation is local; no published CI, remote push or release is claimed. A verified portable source bundle and its hash are recorded in this package’s `source-checkpoint.json`.
