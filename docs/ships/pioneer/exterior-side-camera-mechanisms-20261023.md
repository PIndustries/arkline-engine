# Exterior side-camera mechanisms — isolated revision10 prototype

Base b19c4ae2604b29f6649b7f1fde913e099df96430 is privately preserved. Worktree
arkline-side-camera-prototype and branch design/pioneer-side-cameras-20261023
are separate from the registration9 cargo/display branch. Runtime publication
remains unauthorized. Existing saved worlds are not opened or migrated.

The user requested two additional overhead displays between drone1/2 and drone4/5,
showing left and right hull-side views after warp. Physical order becomes
D1,PORT,D2,D3,D4,STARBOARD,D5, plus the two existing console turret screens.
Stable hardware0–6 remains; port7/starboard8 append. Existing display surfaces8–14,
reactor15 and metrics16–20 remain stable; side-camera surfaces append21–22 with
explicit bidirectional mapping, not an assumed hardware+8 relationship.

## Geometry experiment and fit evidence

A same-size midpoint insertion failed measured envelope fit. The prototype
redistributes seven0.64×0.36m overhead screens on radius2.62m around canonical
XZeye[31.48,0], bearings±54,±36,±18,0degrees, facing inward. Five metric faces
remain associated with their drone cassettes. Screen Ytravel2.45→1.95m remains
vertical with shared mesh/glass/depth/picking pose; turret dimensions/travel stay.
The original normaleye/FOV, seat and rear aisle remain unchanged.

First actual solid test detected outerD1/D5 travel intersecting last roof-haunch.
The whole final structural hoop (roof, both haunches and uprights) therefore moves
rearward0.45m, retaining solid physical members and their wall/roof attachments.
No collider is hidden, removed or changed to pass-through. The first fullprotocol
run passed76tests including allnine moving-envelope clearance, disjoint sweeps,
pilot rays and continuous seat/boarding/circulation routes. The stronger fixed-cassette-vs-structure test subsequently passed; actual
render validation is recorded below as it completes.
This is not yet a completed visually accepted fit or structural certification.

## Authority contract

Keep legacy seven-element persisted screen/mode arrays. Add defaulted external
pairs, nine-slot shared accessors and actualfraction view; older payloads still
deserialize. Registration10 fresh matching fixtures reject older bound saves
unchanged. This is not a universal mixed-client admission gate.

AUTO external views deploy only in confirmed normal flight Station/Belt, including
firstload. DockedAUTO is stowed; manualOPEN may deploy if sweepclear. InTransit or
Jumping overrides all nine targets to stow; actual fraction/blocked status remain
truthful. ManualSTOWED persists after arrival, OPEN preference is retained while
warp overrides travel, and accepted normal state—not a local timer—restoresAUTO.
Captain controls remain monitor-only and ownership/binding checked. No drone
recovery, warp-securement wait, camera/motor power or mass simulation is implied.

## Pending runtime acceptance

Actual body-frame exterior camera mounts must show the corresponding hull flank
with honest scene/occlusion, stable vessel+side identities, no drone-ID reuse and
no fake image. Nine dedicated plus four manual feeds require13unique capture
slots, conservatively scheduled and measured with independent freshness labels.
Retain actual captain controls, unsupported-state labels and fixed instruments.
Required proof: nine deployed/partial/stowed, real warp/arrival/manual overrides,
source identity/picking/depth consistency, all-source scheduling, headturn
readability and view-access/aisle clearance. Cargo/full-face drone graphics are
separate source branches until explicit tested integration.

Final geometry/protocol pass at this stage:77tests passed. The stronger housing
test checks all seven redesigned overhead housings against each other and all
registered structure. Its first broad form also detected the unchanged lower
turret cassettes' existing overlapping center housing joint; that unchanged static
joint is explicitly outside the new overhead-housing assertion. All nine moving
sweeps still must clear every solid. No moving collision failure was exempted.
Full authority server suite110passed/5ignored includes actual nine-slot captain
control continuity and external warp/arrival/manual preference behavior. Actual
renderer and camera proof remain pending, so these are not whole-feature passes.

## Runtime evidence in progress

Actual decoded source hull bounds are [-36,-9,-19]..[36,9,19] metres.
The two body-frame camera mounts [15,6,±19.4] are 0.4m outside the
lateral envelope; the central ray reaches the respective hull flank at
18.34635m, beyond the 0.2m near plane. Both real engine views retain hull
geometry. Camera frames explicitly disable cockpit presentation and enable
camera-pass rendering, with no recursive display callbacks.

The actual EGL head-turn sequence has captured all13 distinct sources
(four manual plus nine dedicated) while retaining at most one fresh capture
per frame. The canonical forward view does not show every physical screen;
ordinary head turns reveal the outer faces. This is not a claim of13
simultaneous high-frame-rate video streams. Final v3 evidence includes the
fully completed manual-stow state with five drones and two mining cycles
still active. Earlier v2 stow/arrival images were transition-start diagnostics,
not proof of completed movement.

A separate actual server routine→LeaveSeat→900ms journey provides a revision10
standing snapshot in Belt, seated=false, feet[31.8,8.68,0], all nine fractions1
and both external modesAUTO. Rendering that snapshot and completed post-arrival
reopening remain acceptance work, alongside final full client checks.

Completed authority reopening evidence: the real arrival is at46902ms. Two
accepted external AUTO commands followed by a real Snapshot at48102ms
(arrival+1200ms) confirm both external modesAUTO and all nine physical
fractions1. This does not restore drones invalidated by the existing warp
semantics; open unassigned hardware is not evidence of active drone feeds.
The test-only export passed and preserves the arrival-start sample separately.

## Final acceptance and regression finding

The v5 actual EGL sequence passed26 captures including seated, standing,
manual-stowed active equipment, accepted warp and completed arrival reopening.
Client suite:525passed, the same eleven recorded baseline failures,34ignored;
Clippy all-targets passed with baseline warnings. Camera images have explicit
wall-clock cached age. Replaying one unchanged authoritative snapshot freezes
simulation time, so retaining correct pixels while wall-clock age rises is
intentional, not evidence of live video throughput. No timestamp is hidden.

The complete server suite found a new test setup incompatibility in
`transport_presence_counts_sessions_without_removing_persistent_ships`: after
Undock, external AUTO screens now legitimately deploy for1200ms. The test's
whole-PilotView comparison sampled that moving interval and differed only in
mechanism updated_at_ms. This is not called a baseline failure. A bounded
actual-nine-actuators-settled precondition is being added before transport drops;
all exact pilot, ledger, presence and restart assertions remain unchanged.
Initial full and isolated failures are retained in the validation evidence.
Final corrected full-suite results are pending.

Corrected complete server run: **262 passed, 0 failed, 6 ignored** across43
unit/integration/doc targets (`/tmp/sidecamera-server-complete-v3.log`). All four
journey tests passed. The six opt-in ignores are five historical copied-world
checks and one drone-telemetry exporter; that exporter was previously exercised
on registration9, not rerun as a registration10 export. The current registration10
side-camera, captain and standing exporter tests were separately run successfully.
The focused corrected presence test passed before the complete rerun.

This checkpoint remains a local fresh-world development prototype. It is based
on the drone telemetry checkpoint and does not yet include the later cargo or
full-face data graphics; these are integrated and revalidated separately. Public
runtime source publication remains pending explicit approval.
