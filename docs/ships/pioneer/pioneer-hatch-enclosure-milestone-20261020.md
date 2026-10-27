# Pioneer bounded hatch and enclosure milestone — 20 October 2026

Continues recovery checkpoint `b820e2a`; supersedes the two-failure status and next-step section in `pioneer-short-recovery-20261020.md`. Only test fixtures and this report changed. Registration remains 3; the source GLB, authored geometry/collision, seat and all saved worlds/builds remain unchanged.

## Results

- **Hatch / boarding / return: PASS.** `.build/logs/20261020T154756096390Z.{json,log}`: 1 passed, 0 failed in 30.03 seconds. Evidence `/tmp/pioneer3-native-hatch-arrival-r2-20261020/receipt.json`, opening-stage PNG/JSON files, `station-exit.png`, and `returned-reseated.png`.
- The fleet berth stroke is 12.68 m. At the actual server speed of 1 m/s it needs 12,680 ms; the bounded 50 ms editor ticks reach each end in **12,700 ms**, within the derived **14,000 ms** deadline. The fixture now waits for reported target arrival, checks target, grounded lift support and pilot/platform floor agreement on every tick, rejects early stops, and retains exact terminal floor and later station/seat-return assertions. No authority state is edited to force arrival.
- **Enclosure: PASS.** `.build/logs/20261020T155813282830Z.{json,log}`: 1 passed, 0 failed in 12.68 seconds. Evidence `/tmp/pioneer3-enclosure-seated-pane-interiors-20261020/receipt.json`: 13 views at both 1080x720 and 1500x960, 26 captures total. Strict patch thresholds (<10% opaque for windows; >90% for walls), 3 cm canonical ray/GPU depth agreement and retained GPU buffers remain enforced.
- `cargo fmt --all` and `git diff --check` passed. No full workspace or broad native matrix rerun was performed; the last full workspace pass remains the earlier source checkpoint, as recorded in the recovery report. Native hatch output retains the known GLD_TEXTURE_INDEX_2D driver warning while assertions pass.

## Port-window diagnosis and what changed

The retained reference assessment describes a deeper fitted bay, the one-metre aft workstation move, preserved outer hull/glazing, shared side boundaries and unchanged measured boarding opening. The original test still declared a cardinal 90-degree side look to be an open pane. From shared seated eye body (33.22,1,3), that ray hits the deliberately fitted port-aft-wall at Z=-6.8, distance approximately 9.78 m. The corresponding screenshot shows the wall, boarding approach and existing glazing forward of it. This was a stale test expectation after fitting, not an opaque plate across an actual pane.

Cardinal port and starboard views remain tested as solid walls, with strict depth/ray agreement. Two additional window probes aim through existing canopy glazing from the same seated eye: port yaw 65 degrees/pitch 0.12 rad; starboard yaw -45 degrees/pitch 0.25 rad. Every view replays the normal head-look updater and checks that the requested angle is reachable. Every expected opening must additionally intersect actual authored navigation-glass triangles, preventing a missing hull from masquerading as a window. Port probe central opaque fraction is 0 at both resolutions; glass distance is approximately 4.58 m. No tolerance was relaxed and no wall, mullion, camera or source mesh was removed. Earlier intermediate probes landed on real mullion/upper-frame edges; their failed logs and captures remain retained.

This does not promise a 90-degree side-window view from the deeper seat. Changing the bay's solid sidewalls to create that view would require a separate geometry/registration design decision; it was not justified by the existing fit contract and is outside this bounded fixture repair.

## Visual inspection and limits

Visually inspected final port/starboard enclosure PNGs and returned-seat PNG, plus `/tmp/pioneer3-native-routes-final-20261020/editor-deck2-registered-corridor.png` and the prior forward/port enclosure captures. Corrected metric seams are now human-scale panel joints, with distinct deck grip and ceiling divisions; the old 100x density is absent. The corridor remains repetitive and flat, the forward enclosure view uses unpopulated instrument faces, and lighting/material/equipment refinement remains unfinished. These offscreen native CGL tests do not certify an OS-window journey or package release.

The matching registration-1 editor 26.10.20.8 remains running (PID 19652); registration-2/3 package launch is still unconfirmed. No app restart, package, source asset modification, world migration/rebinding, MOVESPEED write, deletion/move, upload, publication or GitHub edit occurred. Guarded single target `.build/target` retained >136 GiB free against start >=50 GiB / stop <30 GiB. No build worker survived completion.

## Handoff

The two focused failures are resolved at fixture scope. Next parent milestone: source validation covering the post-5b58c7b production changes, then bounded visual/material refinement or the parent-selected acceptance work. Packaging remains deferred to that decision. Complete source recovery bundle: `/tmp/pioneer-hatch-enclosure-validated-20261020.bundle`, with `.verify.txt` and `.sha256` sidecars; it contains the committed source/report and complete reachable Git history. Runtime evidence remains at the explicit paths above.
