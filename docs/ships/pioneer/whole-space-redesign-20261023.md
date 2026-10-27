# Pioneer whole-space redesign: experimental plan v1

Tracking: [issue 62](https://github.com/PIndustries/arkline-engine/issues/62).
Base: `8da5c35e5d31e3583173bfda23ccbb87b91f6326`.
Branch: `design/pioneer-whole-space-20261023`.

This document records the published registration-5 spatial checkpoint. The
subsequent physical deployment implementation advances to registration 6; see
[physical cockpit mechanisms](physical-cockpit-mechanisms-20261023.md) for its
current contract and verification. No revision-5 save migration is implied.

Pioneer is the flagship development platform. Its previous room shape, cockpit,
seat position and screen arrangement are not immutable requirements. The target
is the approved coherent six-view cockpit and corrected whole-room study, not a
screen-count patch. The ship must eventually account for installed mass, power,
lighting, life support, machinery and access as connected systems.

## Current slice and isolation

This first slice is a whole-space physical blockout with one integrated cockpit,
not completed art or implemented machinery in every room. It runs on dot's own
computer. The normal checkout/build and saved worlds remain untouched. The
experimental registration is **5**; fixtures are newly created in memory or in
fresh disposable databases. No old-world access, relocation or migration is
part of this slice. The old registration-3-to-4 migration is not broadened.

The body frame stays +X forward, +Y up, +Z starboard. The pilot and rear doorway
are at Z=0, the actual ship centerline. The prototype chamber spans X=27..34.9m,
Z=-2.7..2.7m, with floor Y=-0.32m and ceiling Y=2.88m. Its 7.9×5.4m envelope is
a prototype design choice, not a certified compartment. The seated eye is
[31.48,1.35,0]m. A centered rear door joins the two-deck core. A separate aft-port
service exit reaches the existing outer boarding interface without crossing
through the console. Both routes have actual support and collision geometry.

The continuous console wings, forward belt, central plinth, canopy frame,
repeated roof ribs, aisle markings and rear service reservations form one room.
Physical instrument planes are shared by drawing, pointer rays and structure.
Original primary functions remain; port side shows actual drone information and
starboard shows known ship/mining/cargo readings. No fictional green subsystem
indicators stand in for missing simulation. Screen quantity follows function,
not a target count. The wider project includes a meaningful ship overview and
interior cameras.

The two-deck program is represented throughout the registered geometry:
- Industrial/service: EVA, power control, drone workshop, cargo access, stores,
  workshop.
- Crew/command support: NAV/PLANNING (not a second bridge), galley, sensors/comms,
  medical/rest, operations, life support.

Static cabinets, benches and reservations are explicitly blockout fittings.
Their solid bodies are not pass-through props. Their mass, electrical load,
thermal and atmospheric behavior remain unmodeled until registered systems exist.
Current room dimensions and module proportions can evolve after whole-ship fit
review; neither the prior layout nor this first prototype freezes the design.

## Visual and market-research basis

The approved cockpit shows a deep centered chamber, continuous wraparound
instrument belt and side consoles, physical controls, overhead construction,
central seat, clear walking space on both sides, and straight rear entry. The
approved room board uses clear central circulation, wall service bays and shared
structural rhythm. Concept pixels are not measurements or engineering evidence.

Primary developer references inform principles, not copied assets or unsupported
commercial-success claims:
- [CIG Corsair designer Q&A](https://robertsspaceindustries.com/en/comm-link/engineering/17030-Q-A-Drake-Corsair): seat exit paths and crew/service circulation.
- [CIG Alpha 3.10 postmortem](https://robertsspaceindustries.com/en/comm-link/transmission/17787-Alpha-310-Postmortem): simplify information states and reduce cockpit visual noise.
- [Egosoft developer news](https://www.egosoft.com/news/current_en.php): consistent architectural language and walkable human scale.
- [CIG May 2024 ship-art report](https://robertsspaceindustries.com/en/comm-link/transmission/19999-Star-Citizen-Monthly-Report-May-2024): model volume and construction first; lighting should reveal it.

Existing material/enamel, power-backlight and renderer-cache work is outside this
slice. No shader improvement, light-power accounting, photorealism or ray tracing
is claimed. Real scene captures must be reviewed before polishing.

## Systems facts and next integrated behavior

The current code already has ore-dependent acceleration, fixed reactor/capacitor
budgets, warp/mining/drone power debits, and one connected-airlock breaker, lamp,
ventilation/emergency reserve and scalar pressure behavior. These are useful
foundations, not a whole-ship simulation. The room-equipment graph is a typed
catalog rather than a general solver. Continuous-load overload/brownout behavior,
installed equipment/fuel mass ledgers, conserved atmosphere and general thermal
simulation are not established. Boarding service power is not yet sourced from
the actual vessel capacitor.

After the spatial slice is validated, the recommended first systems vertical
slice is one compartment feeder with stable component IDs powering its actual
lamp, vent, door controls, emergency reserve and one interior camera. A trip must
change real behavior, the cockpit overview must show cause/freshness, and restart
must preserve state. A vessel registry should join geometry, installed mass,
circuits, compartments and sensors so later work uses one model rather than
separate decorative interfaces.

Interior camera reservations should identify compartment, body-frame mount,
power circuit and authorized viewing roles. Existing drone/turret feeds locally
render OpenGL textures; they are not network video streams. Interior camera
protocol, read authorization and mobile streaming are not implemented here.
The future feed contract needs timestamps and operating/off/stale/denied states,
followed by a separately authenticated renderer/encoder gateway. Never label a
static preview as live video.

## Seamless docking and cargo roadmap

Current Dock/Undock and ore deposit complete instantly in authority code. A
client animation cannot make those operations genuinely seamless. The follow-on
requires an authoritative approach, reservation, alignment, capture and docking
operation with one continuous ship/camera transform, cancellation/recovery and
swept hull/berth checks. Existing timed hatch/lift/seat machinery can be reused.
The current sector flight berth and local hangar berth need an explicit transform
mapping. Resource-job controllers must wait for real docking/undocking completion
instead of assuming the next snapshot has already changed location.

Cargo unloading should use an actual service interface and timed, conserved
transfer, with access/interlocks, progress and interruption semantics. Durable operation IDs,
reserved units, bound vessel/store identities and idempotent batch receipts must
feed the existing transactional inventory/ledger rather than double-credit
reconnects. Docking needs persisted operation recovery, a reserved collision-safe
station/hangar corridor and explicit station-to-hangar-to-berth transforms. No
cosmetic motion or cutscene should conceal an instantaneous ownership change.
This is subsequent work, not functionality supplied by the blockout.

## Validation plan and honest boundaries

1. Inspect actual seated forward, left/right head-turn, rear-entry, standing-aft
   and overhead/cutaway views. Verify centered ship-relative coordinates, room
   silhouette and console integration before detail.
2. Check rendered/picked/depth planes, side-text readability, clipping, closest
   face selection, authority/freshness states and unchanged central commands.
3. Check continuous chair folding and leg clearance, two side aisles, rear and
   service exits, both stair flights, boarding hatches/lift and private/linear/
   circular placements with full-height capsules.
4. Identify the old exterior-source hash and exact source-cut limits. Full hull
   fit, manufacturing, pressure seal, source/export reconciliation and original
   detailed exterior redesign require their own evidence; bounds alone do not
   establish fit.
5. Run formatting and applicable tests/lint; report new failures, historical
   baseline failures, ignored checks and unrun native journeys separately.
6. Record local source, tested source, push, exact-head CI, merge and release
   separately. Do not call this deployed or complete based on a screenshot.

Initial actual-render review found the large central console occluding lower
instruments and the inherited fixture spawning the ship inside an asteroid.
Those are concrete defects to repair before visual acceptance, not reasons to
weaken the visibility tests. The first captures are coarse spatial blockouts;
they do not yet match the approved concept's construction/detail quality.

## Prototype canopy change

Revision 5 replaces the saved-source `Bridge center mullion` (primitive 1,
indices 2880..3204, source ID
`part:2edf5970536f9fa18916a20dd4ab3c43c403bf71ab9c3ed6121b7512aa994f2b`)
with shared outboard uprights/transoms connecting the chamber to the original
front glazing plane. The saved GLB/export receipt stay unchanged; derived
presentation validates the exact source identity and hash. This is a spatial
proposal, not structural or pressure-vessel certification. Existing door pressure
interlocks are unchanged; current scalar pressure is not a hull integrity model.

The next geometry pass uses continuous inclined wing fascia, an open chair
frame and chamfered roof ribs. No new material, shader or powered backlight is
part of this change. All 61 protocol tests pass before the final canopy pass;
final-head render and test results are recorded separately below.

## Delivery boundaries and evidence ledger

- Implemented in this branch: registration-5 chamber/console/seat placement,
  roof/forward framing, rear/service access, new room-program solid blockouts,
  seven physical functional displays, shared projection/picking geometry.
- Preserved mechanisms: authoritative seat folding, hatches, lift, stairs,
  central command semantics and existing mining/drone/cargo telemetry.
- Not implemented: exterior-wide redesign, a mass ledger for the new fixtures,
  whole-vessel electrical/thermal/gas solver, interior-camera protocol/streaming,
  or timed docking/unloading. Current instant docking/deposit remains current
  behavior, not a feature of this spatial prototype.
- This experimental build must only use fresh registration-5 fixtures. It does
  not route mixed saved layout revisions; old registration admission is refused,
  not silently migrated. The normal registration-4 branch remains available.

Measured acceptance so far: all 61 protocol tests pass including actual capsule
routes and continuous chair motion. The authoritative hatch/lift/station roundtrip
passes using the new rear-port route. A disposable SQLite admission test rejects
registration 4 and preserves the exact saved world bytes. These tests do not
read any existing user world. Full server/client final-pass results remain to be
recorded after source and rendering review.

The exact archived `8da5c35` client baseline was independently compiled/tested:
473 passed, 11 failed, 27 ignored. Its 11 existing room/UI-label assertions must
be distinguished from new layout regressions; they have not been silently
rewritten or disabled to obtain a green total.

Checkpoint validation update:
- Protocol: **62 passed, 0 failed**, including explicit room-program presence,
  solid bounds and clear center checks.
- Server library: **77 passed, 0 failed, 4 existing ignored**. The fresh occupied
  fixture, three placement/reopen paths and old-layout rejection preserve feet,
  binding, jobs, cargo, credits and ledger under their respective assertions.
- The three former revision-4-only server expectations were replaced with
  revision-5 fresh-route and no-migration expectations. Historical tests remain
  in the base commit; no runtime migration was added and no test was ignored.
- Runtime v4 cockpit captures show actual App/HangarView frames with controlled
  disposable telemetry. They are not a live production mining session.
- The initial whole-ship diagnostic cutaway incorrectly omitted Room-cell
  registered furniture. That proof filter is corrected separately; it never
  changed the complete installed scene. Do not use the initial cutaway as room
  program evidence.

Remaining design work is visible in the captures: oversized largely empty wing
fascia and individual tablet-like housings must become segmented integrated
instrument bays/controls; the chair still reads as a rough frame, and adjoining
source window edges remain. The first checkpoint establishes spatial scale,
access and shared mechanics, not visual equivalence to the approved concept.

Reproduce explicit actual-runtime evidence with fresh nonexistent directories:

```sh
ARKLINE_WHOLE_SPACE_EVIDENCE=/tmp/new-cockpit-proof LIBGL_ALWAYS_SOFTWARE=1 \
  cargo test --offline --locked -p arkline-client --lib \
  native_whole_space_cockpit_runtime_views -- --ignored --nocapture
ARKLINE_WHOLE_SHIP_EVIDENCE=/tmp/new-ship-proof LIBGL_ALWAYS_SOFTWARE=1 \
  cargo test --offline --locked -p arkline-client --lib \
  native_whole_ship_registered_blockout_cutaways -- --ignored --nocapture
```

These use the real EGL/OpenGL engine test harness. No renderer-cache redirection,
material override or screenshot editing is involved. The interactive native
window's existing `NoGlutinConfigs` failure is not resolved by this slice.

Clippy for the protocol, server and client libraries completed successfully with
18 existing-style/deprecation/dead-code warnings (17 client, 1 server); no
warning-as-error clean claim is made. `git diff --check` passes. Workspace-wide
format checking reports unrelated pre-existing formatting drift; changed files
are checked separately without reformatting unrelated modules.

The corrected whole-deck proof contains 44 program draws across 11 rooms:
24 on deck 1 (six rooms), 20 on deck 2 (five rooms); the galley retains its
existing recipe. Each is asserted to be an exact draw from the installed scene.
The final central screen moves 12 cm forward to X=32.90 m while retaining its
half-extents, inclination, command semantics and Y=0.95 m. This resolves the
front-bank/case overlap without compromising narrow-aspect safety margins.

## Next interaction requirement (not implemented by this checkpoint)

Implementation plan: [activity-driven cockpit displays](dynamic-cockpit-activity-20261023.md).

The spatial direction was accepted for further development with a request for
event-driven instrument presentation: confirmed drone launch should reveal an
individual camera view for each active drone; firing turrets should reveal two
turret views. The next slice must distinguish an animated display inside a fixed
physical bay from physically deploying solid screen hardware. It must use
confirmed entity/event state, per-feed freshness/signal, reversible activation,
stable input/occlusion, and an honest schedule for the existing four-feed cache.
It must not invent live streams, silently exceed the feed budget, or turn a
command click into authority-confirmed deployment. Mobile sharing remains a
separate authenticated stream architecture.

Final library verification at the checkpoint geometry:
- Protocol **62 passed / 0 failed / 0 ignored**.
- Server **77 passed / 0 failed / 4 existing ignored**.
- Client **476 passed / 11 failed / 29 ignored**. The 11 failure names exactly
  match the independently built base; there are **zero new failure names**.
  The two additional ignored tests are explicit GPU evidence producers, run
  separately and passed, not disabled regressions.
- All five composition checks pass, including **seven faces × 25 exact scene
  depth/input samples × four aspect ratios** and strict 8 px edge clearance.
- Six derived-source tests pass: exact saved identity/bounds, requested source
  removal spans and unchanged remaining triangles. Canonical GLB SHA remains
  `8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123`.

A final standing-view proof correction moves its test camera from behind the
chair to the actual canonical standing feet. The old behind-chair close-up is
not evidence of an unobstructed standing forward view. No runtime geometry was
changed for this evidence correction.

Final cockpit proof is `arkline-whole-space-proof-20261023-v8` (ten PNGs plus
`whole-space-runtime.json`); whole-ship proof is
`arkline-whole-ship-proof-20261023-v7` (five PNGs plus
`whole-ship-cutaway.json`). These local evidence folders are generated artifacts,
not source assets. Forward, side, aft/rear, roof and canonical standing views
were inspected. Canonical standing correctly presents read-only telemetry.
The standing image's lower command plane is cropped by proximity; seated framing
is the strict seven-face input/clearance acceptance view.

Unrelated formatting changes in the two large existing client modules were
removed before checkpoint. Their reconstructed owned changes were normalized
with rustfmt and compared byte-for-byte to the fully formatted versions, so
string literals and semantics were preserved while leaving baseline formatting
outside this change untouched. No whole-workspace formatting-clean claim is made.

## Reactor recovery continuation

The next bounded module is documented in
[reactor configuration](reactor-configuration-20261023.md): four rigid collars
compact around a fixed core with a dedicated truthful pose/capacitor overview.
The lost local implementation is being reconstructed separately from the
published seven-screen base; fresh validation is required. This does not add
whole-ship thermal, mass or fuel simulation, manual captain monitor controls,
or a secure-before-warp interlock.
