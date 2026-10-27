# Boarding-lift gate recovery — 2026-10-20

The actual version 23 screenshot and window showed ERROR with “new layout overlaps a docked traversal capsule” and no usable world viewport. This generic collision-validation message did not mean the authored layout had changed.

A consistent original-world backup is retained under `.build/evidence/load-error23-20261020/`. Account and avatar actuator copies matched exactly; the bound default room layout and registration 4 were unchanged. The avatar was grounded on the boarding platform at station floor 0, near the station-side gate, with the accepted lift target Ship and moving=true.

An exact-pose geometry reproduction proved clearance at floor 0 and collision with `boarding/lift-gate/station` once the platform rose to 0.01 m. Lift advance previously carried the rider before rebuilding the endpoint gate geometry. Final world validation then failed, rolled back the tick, and propagated an error out of the embedded worker. The last safe saved floor 0/moving state remained readable.

The correction checks proposed platform/rider geometry before departure and before each bounded movement step. An occupied gate rejects the request with an instruction to move toward the platform centre; a previously accepted unsafe movement holds the last valid platform/capsule pose. Collision validation and gate geometry remain unchanged. No avatar relocation, layout migration, snapshot rollback, renderer change, or economic state change is required. Normal world open neutralizes outstanding actuator motion as before.

## Focused evidence

- `.build/logs/20261020T221725793369Z.json`: initial red exact-pose diagnostic; station gate identified at 1 cm.
- `.build/logs/20261020T222052730320Z.json`: exact saved-copy recovery passed. The failed first attempt used the copy project directory instead of its data directory; corrected test opens `.arkline-studio`.
- Copy retained at `/tmp/arkline-lift-recovery-copy-20261020-2219`; original world was never opened by the test.

The saved-copy check proves a safe hold, rejection without mutation, centred fixture travel, unchanged authored geometry, ordinary world reopen and running tick, and preserved saved avatar pose/job/ledger. A centred rider is only an isolated in-memory fixture, not a transformation of the user's avatar. Final original-world recovery additionally requires actual loaded viewport/controls verification, not just a version label.
