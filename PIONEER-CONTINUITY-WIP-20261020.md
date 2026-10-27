# Pioneer canonical continuity — unvalidated WIP checkpoint

This record preserves the initial WIP and subsequent continuity repair after user review of build 26.10.20.7. The initial snapshot was **not compiled, tested, packaged, visually accepted or published**; later validation is recorded below. All active production-file owners closed at coherent written boundaries before this snapshot. The existing validated `.7` source checkpoint `6fd5b21`, app, source bundle and desktop evidence remain retained.

## Reproduced in the actual .7 app

The fresh `/tmp/arkline-canonical-matrix-repro-20261020` project and `ViewMatrixQA` pilot reproduce editor Cockpit → Deck2 → Deck1 → Station → Cockpit while paused. Deck shortcuts create legacy avatars at local floor Y=0 without registered traversal, selecting visibly different scenes. Actual Undock changes to the older space cockpit with different eye/FOV/consoles/glazing. Paused camera-selection attempts leave authority world and clock unchanged; only the first External selection was visually confirmed, because the floating selector moved and later hardcoded clicks missed. Those filenames do not prove the requested mode. Evidence is under `.build/packages/pioneer-continuity-20261020/baseline-matrix-os`; earlier attempted captures during an active walk are separate and are not accepted as a clean shortcut matrix.

## Written in this recovery checkpoint

- Compatible editor scene placement now uses validated registered Pioneer geometry, canonical feet/support/binding and explicit migration; it preserves clock, systems, ownership and per-vessel actuator records.
- Canonical deck/room/door-occupancy and equipment-access semantics use the same body/deck conversion. Existing on-foot piloting, swap, remote-control and docking restrictions are preserved.
- UI routing and scene labels distinguish canonical supports, including stair height, and block unresolved canonical registration from silently entering legacy controls.
- Docked and space views consume shared body-metre Pioneer hull cuts, static/dynamic interior, eye, FOV and physical display definitions; the same frame payload reaches color/depth/input/shadow paths. Remote-worker feeds clear another owner's layout payload.
- Authority and UI regressions are written but unrun. A separate new native route-matrix test remains unregistered work in progress and is excluded from this production checkpoint.

## Remaining before acceptance

Compile and fix the integration, add the complete route-matrix native regression, finish canonical cockpit visibility filtering, then visually verify actual app deck/room/camera and Undock/Dock transitions. Do not treat the prior narrow docked walk or new automated checks alone as this matrix acceptance.

The next coherent stage covers a physically fitted wider cockpit/whole pilot station farther aft, visible locomotion, on-foot Shift sprint with seated/text ownership retained, synchronized animated boarding hatches and purposeful shared materials. These are **not implemented in this checkpoint**.

A separate read-only flagship review is reading the actual [Pioneer wiki](https://github.com/PIndustries/arkline-engine/wiki/Ship-Pioneer) and viewing its v05 images, plans and actual Study05 renders. The measured shallow bridge and flat material response are confirmed gaps. Illustration labels are design proposals, not engineering measurements; saved room programs and the distinct upper boarding route must not be silently moved or renamed.

No push, upload, GitHub mutation, merge, release, deployment, external-drive write or file moving/deletion occurred. `.7` remains the running app until a later candidate is built and checked; this WIP is not its runtime source.

## Follow-up checkpoint

WIP b4ff6f5 compiled across all workspace targets (15.14 s; log `.build/logs/20261020T133314622082Z.log`). All 20 filtered editor/collaborative-editor authority tests passed, including the three new canonical regressions (`.build/logs/20261020T133444878481Z.log`). These results do not validate later edits automatically.

The next checkpoint adds independent per-camera filtering and metrics for canonical space-cockpit color submission, while preserving the full depth/input scene, and registers a new whole-Cockpit native route test using actual disposable EditorWorld responses and both renderers. That new native test is not yet compiled or executed at this commit. Actual .7 Undock and camera selection were reproduced; camera-only switches preserved paused authority. Actual Dock return was exercised separately in the baseline app.

## Native route validation and next candidate

The repaired native regression passed 1/1 in 90.61 s (`.build/logs/20261020T134956041563Z.log`). It uses real disposable EditorWorld authority plus the actual Cockpit update and both CGL renderers: registered Deck 2, walk to the actual crew/galley doorway, Deck 1, return to seat, four docked camera modes, physical UI Undock, space cockpit and authority Dock return. It verifies body eye, raw physical screen projection, visible UI/depth/occlusion roundtrips, floor GPU landmarks, stale seated release rejection, immutable source buffers and remote-owner isolation. The evidence world is retained under the path in `native-route-projection-repair/fixture.json`.

The first runtime failure was a test assumption that all screen corners were visible. Both baseline and returned docked views clip the same bottom corners; their geometry ROIs matched pixel for pixel. The test now compares actual raw projected corners, including offscreen points, while preserving visible depth and input checks. The clipping remains a cockpit fit limitation. A 3-second process sample later confirmed expensive triangle-ray work during dense evidence sampling; the run completed normally within its 300-second bound.

After that pass, a real procedural-material mismatch was corrected: canonical flight cockpit/external draws now use the same PaintedHull fallback as docked draws; authored GLB material overrides remain authoritative. The new CPU material regression and native rerun are pending at this checkpoint. Candidate version is 26.10.20.8, still unbuilt and unaccepted on the desktop. The actual running app remains .7 until the bounded replacement step.

The completed reference assessment and measured fit proposal are retained beside the Pioneer documentation. The deeper bay, workstation relocation, sprint/gait, hatch animation and material-role finish remain next-stage work, not implemented here.
