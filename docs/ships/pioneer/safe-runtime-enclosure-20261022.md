# Runtime enclosure finish inside existing solids

The narrow inward-wall study remains isolated. Its five proposed members are
not installed. The runtime instead recesses construction into three existing
registered solids: `cockpit/ceiling`, `cockpit/canopy/header` and
`cockpit/canopy/sill`. The full existing room and boarding space remain available.

## Actual runtime change

`pioneer_docked_scene::panel_parts` replaces those three visual boxes with a
continuous recessed backer and intersecting structural ribs. The recess is at
most 35 mm into the solid; ribs are 60 mm wide, spaced into bays at most 1.4 m
across. The backing closes every bay. No new member protrudes into the room.
Original semantic mesh role, color and emission are retained exactly. Materials,
shaders, lighting/backlights, camera code and canonical assets are unchanged.

This uses the existing presentation-parts path for both walking and seated
canonical scene rendering. No optional persistent setting or mismatched
client/server geometry feature is introduced. The authoritative boxes remain
the same conservative collision proxies, including the shallow visual recesses.
The new finish does not claim exact per-triangle collision.

Registration **4**, traversal capability **2**, source identity
`8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123`, serialized
bindings, deck/seat/door/display transforms and interlocks stay unchanged. No
protocol recipe, schema or migration is edited. The source GLB and the unsafe
study assets remain reference snapshots.

## Preservation evidence

Containment tests transform every actual cube vertex in all three placements
back into its original solid box. They also verify unchanged color, emission and
mesh role. Because each new convex visual box is contained in an existing solid,
it cannot introduce an obstruction at a previously legal occupied position.
This is stronger than proving a finite selection of routes alone. Existing f32
transform roundoff is bounded by the test tolerance.

A second check covers **183 clear and supported grid samples** near the rear,
both sides of the rejected local bay, the original starboard boundary and port
walkway, across private, linear-fleet and circular-fleet placement.

The new server fixture uses real connect/seat-exit/walk/stop commands to reach
both sides of the rejected bay and the actual far rear corner. Each placement
has three SQLite close/reopen boundaries. Feet, binding, jobs, cargo, credits and
ledger rows are checked. Only new disposable `/tmp/arkline-boundary-save-*`
worlds are opened. Circular fleet creation uses the existing scene-aware editor
realm constructor; an initial fixture incorrectly used the ordinary legacy-only
constructor and was corrected without changing production behavior.

Each fixture exports its post-reopen public snapshot. The native runtime check
can load all three into the actual `HangarView::ui` and instrument callbacks,
alongside matched seated, rear-entry, retained-starboard-space and overhead
inspection views. It checks the loaded capability and canonical support/capsule,
retains the source pilot, and captures actual runtime images. The center of the
seated physical computer also completes a projection/pointer roundtrip.

The native harness now supports the existing Linux EGL context as well as macOS
CGL. It changes no renderer program or camera implementation. These are fixture
snapshots and offscreen runtime callbacks, not live telemetry, OS input or a
packaged app launch.

## Reproduction

```sh
cargo test --locked -p arkline-server --lib docked_traversal_tests::registered_boundary_space_reopens_fixture_saves_in_all_placements -- --exact --nocapture
cargo test --locked -p arkline-client --lib recessed_enclosure_ -- --nocapture
```

For explicit native capture, set `ARKLINE_PIONEER_DOCKED_EVIDENCE` to a new
directory and optionally set `ARKLINE_PIONEER_RUNTIME_SNAPSHOTS` to a JSON array
of the three fixture `runtime-snapshot.json` paths. Then run:

```sh
cargo test --locked -p arkline-client --lib flight3d::hangar::pioneer_docked_scene::native_tests::native_registered_enclosure_boundary_views -- --ignored --exact --nocapture
```

The constructor rejects an existing evidence directory. Check the test count and
new `boundary-receipt.json`; a zero-test filter is not a render result.

## Executed results

The [machine-readable receipt](safe-runtime-enclosure-20261022.json) binds the
source files and retained native receipt by SHA-256. Relevant checks report:

- Registered-scene module: **12 passed, 0 failed, 2 ignored**. The new containment
  and 183-sample boundary checks are included.
- Physical input: **13 passed, 0 failed, 1 ignored**, retaining command-routing
  assertions and the unchanged child-process deadline.
- Server traversal module: **19 passed, 0 failed, 1 ignored**, including nine
  fixture save roundtrips, seat/exit/reseat, hatches, lift, stairs, boarding,
  capability compatibility and existing migration guards. The ignored test
  requires a separately supplied copied user world and was not used.
- Explicit native runtime: **1 passed, 0 failed, 0 ignored**, capturing seven
  views, including the three actual post-reopen private/linear/circular snapshots.
  The earlier matched before capture also reports **1 passed**. The other broad
  native acceptance remains ignored; no all-native-suite claim follows.

Actual images were inspected locally in
`/workspace/scratch/pioneer-safe-enclosure-runtime-loaded-saves`, with the before
views in `/workspace/scratch/pioneer-safe-enclosure-runtime-before`. The four
fixed camera positions and yaw/pitch match. The canopy header has visible frame
depth, and the upward aisle view shows recessed overhead construction. The
loaded boundary-save views retain the large room and show the existing far wall;
no inward wall traps those saved pilots. Forward composition remains sparse.

The existing runtime candidate budget rises from **3,266 to 3,305 draws** and
**247,424 to 247,892 triangles** (+39/+468). These are scene submission inventory
counts, not a frame-rate or performance improvement. Five preexisting client
warnings remain. No full-client suite was rerun; the separately evidenced eleven
baseline failures remain documented.

## Limits

The fixed full-size cockpit is still substantially wider than the isolated
study. Inward walls cannot be adopted while preserving every legal registration-4
occupied position. This is a bounded passive construction finish in the actual
game render path, not completion of the approved cockpit concept. It does not
solve the empty forward field or establish improved instrument readability.
No user save, deployment, merge, upload or previously denied operation is used.
