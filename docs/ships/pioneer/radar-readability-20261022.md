# Radar primary title and authority — 22 October 2026

This bounded followup starts at `f9798db23742c04845661a3e013660d08bc1c51c`.
It enlarges the actual canonical radar's primary title and authority status,
without moving its registered display plane or either comparison camera.

The header now reads `RADAR` beside `LIVE`, `PAUSED`, `STALE` or `OFFLINE`.
The font fits the widest status (`OFFLINE`) to keep its size stable between
states. The secondary `SHIP RELATIVE` legend retains `/ HELD` for paused
snapshots and `/ LAST` for disconnected snapshots. Disconnection takes
precedence over pause; stale starts after the existing 1500 ms freshness window.
The existing standing authority badge is omitted only on radar, where the new
header owns that information. Seated presentation receives the existing pause
state through a transient UI field; no persistent setting or save field is added.

The contact plot moves to the lower left, with exact formatted range and
ship-relative context beside it. Contact filtering, ship orientation, range
normalization, altitude stems, drone markers and three range rings are retained.
The plot radius is preserved rather than traded away for larger type.

## Evidence

The existing actual-runtime all-five-instrument capture test uses the same
1280×720 seated App and standing HangarView fixtures as the previous milestone.
The baseline is `/workspace/scratch/pioneer-five-instruments-final`; the new
capture is `/workspace/scratch/pioneer-radar-readable-final`. These are fixture
snapshots, not live telemetry. The fixture asteroid intersects the seated
exterior background; this pass does not claim to resolve that scene limitation.

The added CPU projection regression paints the production radar on the actual
App's registered surface for all authority states, ranges 1 m, 1000 km and
1e15 m. It checks four complete text rows, no text overlap/clipping/ellipsis,
five clear registered ray samples per row, and at least 14 projected font-em
pixels for title and primary status. Separate actual galley checks cover header
fit at 190×90, 300×180 and 496×180 canvases. The 14 px projection assertion is
for the unchanged canonical seated pose, not arbitrary cameras or smaller
viewports.

| Text | Previous em px | New em px |
| --- | ---: | ---: |
| Seated radar title | 8.58 | 15.02 |
| Seated authority | absent | 15.97 (`LIVE`) |
| Standing radar title | 11.83 | 20.75 |
| Standing authority | 2.70 | 22.56 (`LIVE`) |
| Seated exact range | 8.67 | 9.00 |
| Standing exact range | 12.23 | 12.88 |

The worst-width `OFFLINE` state measures 15.73 em px seated. Primary title/status
exceed 14 em px in the checked canonical seated view, with no clipping or
ellipsis and five clear ray samples. The normal physical canvas is 495.58×180;
the plot radius changes from 34.76 to 35.87 canvas units. Exact calibrated range
remains `1000 km` in both matched captures. Every non-radar text row and the
seated camera pose match the preceding receipt exactly. Font em span is not a
human reading score or a claim about glyph ink height.

The matched screenshots were inspected locally. The title and status are visibly larger in both seated and standing
views, fit inside the original glass, and leave the plot and exact range
separate. The secondary ship-relative legend is still very small (3.27 em px
seated, 4.65 standing); range is still below 14 px seated. These limitations
remain visible rather than being removed by camera changes.

## Validation

- Radar header fit and production authority/range projection: **2 passed,
  0 failed, 0 ignored**.
- Control-bank IDs and commands/readiness: **2 passed, 0 failed, 0 ignored**.
- Physical input: **14 passed, 0 failed, 1 ignored**, with the existing child
  process deadline retained.
- Existing map regressions: **5 passed, 0 failed, 0 ignored**.
- Explicit native actual-runtime all-five-instrument capture: **1 passed,
  0 failed, 0 ignored** (20.94 seconds, software OpenGL).

All checks used the repository's normal test profile. Five existing client
warnings remain. No behavior assertion or timeout was relaxed.

The [machine-readable receipt](radar-readability-20261022.json) records image/report hashes, measured text,
unchanged authority/input/power sources and retained contact code. Run:

```sh
cargo test --locked -p arkline-client --lib cockpit_radar::feasibility_tests:: -- --nocapture
cargo test --locked -p arkline-client --lib cockpit_control_bank_ -- --test-threads=2
cargo test --locked -p arkline-client --lib physical_ -- --test-threads=2
cargo test --locked -p arkline-client --lib cockpit_maps::tests:: -- --test-threads=2
ARKLINE_INSTRUMENT_READABILITY_EVIDENCE=/tmp/new-radar-proof \
cargo test --locked -p arkline-client --lib flight3d::hangar::pioneer_docked_scene::native_tests::native_five_instrument_readability -- --ignored --exact --nocapture
```

No full-client run is claimed; the eleven previously evidenced baseline failures
remain separately documented. Remote CI remains unavailable and was not retried.

## Limits and preserved boundaries

Primary title/status reaching 14 px does not make the entire instrument legible.
The range remains around 9 px seated and the secondary legend around 3.3 px.
Lower standing instruments remain partly outside the unchanged viewport.
Power/footer, materials, backlights, renderer programs and cache behavior are
unchanged. Canonical registration 4, capability 2, source identity, anchors,
colliders, seat, entry path, controls and save semantics are unchanged. No user
save was opened. No Mac deployment, merge or Library upload is included.
