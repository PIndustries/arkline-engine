# Canonical instrument typography and layout — 22 October 2026

This pass measures all five canonical instruments and changes text/layout on the
upper camera/map display, ship radar and compact central computer. Navigation
and power are measured references. The power display, its footer, materials,
backlights, render programs, camera code and caches are unchanged.

## What changes in the actual game

Missing-camera state now uses the face height to size and wrap complete words.
The map reserves separate larger authority, selection and calibrated-range
bands before fitting contact geometry into the remaining field. Authority and
range lines fit in full; secondary long contact names retain the existing
ellipsis policy, with measured distance first. Contact selection/range controls
still use the same IDs, actions and the current plotted positions.

Radar has a larger `SHIP RADAR` label and range readout, with a reserved plot
field between them. Ship-relative projection and underlying confirmed contacts
are unchanged. The separate small authority badge remains a known limitation.

The compact central computer has a larger speed value, labels and delivery
readout. Full phase/value strings fit their column widths on one line instead
of pushing the hold row into workspace keys. Missing routine/target captions are
shortened without inventing numeric readings. `Expand panel` is one line inside
its existing rectangle. Compact control guidance moves to both actual control
tooltips, including disabled controls; detailed inline guidance and the active
routine cancellation warning remain.

The DOCK/UNDOCK, STOP, DETAILS and workspace control rectangles, UI IDs, command
readiness and actions remain in place. No display mount, housing collider,
registered component ID, saved binding, deck, seat, entrance or actuator changes.
Registration remains **4**, capability **2**, canonical source identity
`8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123`.

## Measurement method

The ignored native test
`flight3d::hangar::pioneer_docked_scene::native_tests::native_five_instrument_readability`
uses the real `eframe::App::update` for the seated computer and four peripheral
instruments, then the registered `HangarView` for standing read-only instruments.
It uses the existing headless App fixture and a canonical standing seat-root pose.
The test-only shape observer measures actual egui galleys, clipping, ellipsis,
projected font em span and five exact registered pointer samples per text row.
It neither substitutes a one-display study nor changes production painting.

The same 1280×720 viewport and original camera choices are retained before and
after; unchanged navigation/power rows provide projection checks. The seated
fixture contains a test asteroid at the own ship's belt position: its exterior
background is not evidence of live scene quality. Fixture snapshots are not live
telemetry. Font em height is a geometric measurement, not a human reading score.

Before evidence is retained at `/workspace/scratch/pioneer-five-instruments-before`.
Final images were inspected at `/workspace/scratch/pioneer-five-instruments-final`.
The [machine-readable evidence](instrument-readability-20261022.json) binds both
receipts and unchanged authority/power/input geometry sources by SHA-256.

| Text | Before em px | Final em px |
| --- | ---: | ---: |
| Seated central speed | 11.54 | 15.47 |
| Seated camera state | 1.99 | 12.39 |
| Seated radar label | 2.22 | 8.58 |
| Seated radar range | 2.28 | 8.67 |
| Standing map authority | 3.36 | 9.56 |
| Standing radar label | 3.04 | 11.83 |
| Standing radar range | 3.22 | 12.23 |

These measured changed labels retain all five sampled ray hits and have no ink
clipping or ellipsis. Long-phase and missing-speed tests enforce separation from
workspace keys. Full authority and calibrated-range lines cannot be ellipsized;
only long secondary contact names retain the previous bounded-name policy.

The final ID review found that removing the compact help label also removes an
automatic widget-ID allocation. The compact branch now reserves that one ID slot
without allocating space. A dedicated regression compares subsequent widget IDs
between compact and detailed banks, alongside the existing command/readiness test.
This ID-only followup does not alter the captured visual layout; it was verified
with affected tests rather than repeating all completed rendering validation.

## Regression workflow

```sh
cargo test --locked -p arkline-client --lib cockpit_maps::tests:: -- --test-threads=2
cargo test --locked -p arkline-client --lib compact_ -- --test-threads=2
cargo test --locked -p arkline-client --lib physical_ -- --test-threads=2
cargo test --locked -p arkline-client --lib pioneer_docked_scene:: -- --test-threads=2
```

Explicit actual-runtime capture requires a new directory:

```sh
ARKLINE_INSTRUMENT_READABILITY_EVIDENCE=/tmp/new-instrument-proof \
cargo test --locked -p arkline-client --lib flight3d::hangar::pioneer_docked_scene::native_tests::native_five_instrument_readability -- --ignored --exact --nocapture
```

Tests cover 190×90 through 496×180 text canvases, long contact names, missing
camera/stale states, complete authority/range text, pairwise text separation,
800×650 and 1500×638 App viewports, a long routine phase, missing speed and exact
delivery values. The physical glass-hover journey checks guidance accessibility,
disabled controls, detailed inline help and absence of ship-order dispatch.

## Executed validation

- Map/camera state text-fit and behavior: **5 passed, 0 failed, 0 ignored**.
- Compact layout, long-phase/missing-speed and real-glass hover: **6 passed,
  0 failed, 0 ignored** before the ID-only followup.
- Physical input filter: **14 passed, 0 failed, 1 ignored**, including the unchanged
  20-second child-process deadline. Its ignored helper is exercised by the parent.
- Registered-scene module: **12 passed, 0 failed, 3 ignored**; all five glass
  projections and existing clearance/anchor checks remain intact.
- Explicit actual-runtime five-instrument capture: **1 passed, 0 failed,
  0 ignored**. The broad native tests are not claimed executed.
- ID-only followup: control-bank identity and command/readiness checks **2 passed,
  0 failed, 0 ignored**; actual glass-hover guidance **1 passed, 0 failed, 0 ignored** again on final source.

The capture uses software OpenGL and is not a packaged-app performance measurement.
Five existing client warnings remain. No full-client suite was rerun; the eleven
separately evidenced baseline failures remain. No forbidden remote CI lookup was
retried.

## Remaining limits

Several improved seated labels are still around 8–9 px, below a 12 px reference
target. Small central control captions and the radar authority badge remain
smaller. The unchanged missing-power value measures about 4.47 px seated and
6.14 px standing; no separately denied power work is retried here.

At the unchanged standing pose, lower navigation/power labels fall below the
viewport. They are reported with zero projected span/clear samples rather than
counted as readable. Typography cannot recover an off-viewport face. A different
permitted composition design would be needed to address that limit; the proof
does not move the camera to conceal it.

The empty forward field, heavy existing housing and concept fidelity remain
unresolved. No save migration, user-world access, deployment, merge, Library
upload or Forbidden CI lookup is included. Full-client baseline failures remain
separately documented; this is a bounded readability milestone.
