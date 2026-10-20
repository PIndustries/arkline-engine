# Physical Pioneer — unvalidated local WIP recovery checkpoint

Status: implemented locally; not accepted, packaged, launched or published. This checkpoint preserves work before remaining geometry, authority and native GPU acceptance. Active file owners froze at coherent written boundaries before the commit.

The validated component package remains build 26.10.20.5, production source 2e2e6fc, recovery checkpoint 1c57505. Its signed app and verified bundle are retained in `.build/packages/pioneer-components-20261020`. The existing 26.10.20.4 app remains untouched; fresh saved-work confirmation is pending for a later handoff.

Implemented in this WIP:

- Shared body/deck/station metre transforms, registered two-deck shells and real stairwell panels, fitted cockpit furniture and folding pilot seat, connector, two independently interlocked boarding hatches, lift and ground approach.
- Exact SHA/range guarded local cuts in three derived GPU buffers. Canonical GLB, all source-object identities and original inspection buffers remain unchanged. The three derived buffers contain 44880, 156510 and 366 vertices.
- Negotiated additive traversal state and vessel/layout bindings, owned docked captain seat-exit and return, walking/jump leases, per-vessel lift/hatch persistence, compatibility projection, and guards against legacy teleport, remote-control and editor-placement bypasses. Existing legacy crew movement remains unchanged.
- One registered rendered scene across seat, decks, hatches, lift and station; shared pilot joints and camera, independent headlook, actual pane transparency, and physical display faces with shared triangle depth and pointer blocking.
- Conservative per-camera frustum selection, separately keyed candidate/submitted counts, CPU selection/submission times and nonblocking native GPU timing. Flight visibility counts cover tested mesh groups, while enclosing timings also cover other passes. Room portal occlusion and LOD are not implemented or claimed here; simulation is never culled.

Validation completed on intermediate working-tree states (local, not published exact-head CI):

- Initial shared protocol suite: 43 passed; log `.build/logs/20261020T085727190817Z.log`. The later eight geometry journey tests were not present in that run.
- Registered client scene/display tests: 4 passed, 1 explicit native GPU test ignored; log `.build/logs/20261020T085934371584Z.log`.
- Docked client/input/source tests: 19 passed, 1 native GPU test ignored; log `.build/logs/20261020T090055595646Z.log`.
- Exact source-opening tests: 5 passed (overlap the preceding filter); log `.build/logs/20261020T090116752422Z.log`.
- Independent conservative frustum tests: 2 passed; log `.build/logs/20261020T090117796423Z.log`.
- Native production crates and WebGL client compiled. Full all-target check identified a verifier fixture field, now updated; the complete all-target check must be rerun. Full formatting/Clippy are still pending.

Queued for the next validation/fix checkpoint: execute the newly saved eight shared geometry journeys and eight authority tests, compatibility tests, native CGL UI/image journeys, full native/Wasm lint and workspace suite; inspect all native evidence; correct any resulting failures. The step-query contract, lift support join, inner-hatch approach and display-front-plane mismatches were corrected during review but still require these journey checks. Native app handoff and actual OS window acceptance remain pending the saved-work answer. No automatic app closure or discard is authorized.

All processes are bounded by `tools/guarded-local-build.py`; no execution transport blocker remains. No push, PR, issue mutation, deployment, external-drive operation or user project modification was performed.
