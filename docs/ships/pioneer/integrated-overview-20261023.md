# Integrated Pioneer overview — local revision 10

This fresh-world development branch combines three preserved local checkpoints:

- `d3ee75f78f9d0eaad0c57670a521c1735bb36fdd`: nine physical camera mechanisms, including port and starboard hull views.
- `28d52f0cd7f4fd2e1b8068e0bd11f91c7000efa8`: permanent cargo overview.
- `1f665a90631e5ae1d12fbdfdbeae3cb5cce75968`: full-face drone telemetry graphics.

The combined source retains reactor configuration/display, captain per-monitor and all-monitor policies, and five drone/two turret/two exterior camera identities. Registration remains 10; this is not a migration of older saved worlds. Source publication is not authorized by the private backup operation.

## Explicit integration decisions

The GPU test-module merge retains side-camera, cargo and full-face suites. The stable-world persistence setup uses `iter_screens()` across all nine actuators, not the legacy seven-element array. The new transport-presence setup also waits for actual unblocked stationary actuators before comparing whole pilot snapshots; its exact comparisons remain intact. Cargo paint uses the shared confirmed observation clock, while source-specific camera capture age remains independently visible.

No new simulation was introduced during integration. Drone gauges represent actual yaw/pitch, health, signal and the single modeled mining cycle. They do not imply battery, thrust, roll or multiple turret models. Cargo uses generic authoritative units, not invented mass. The reactor remains a rigid mechanical configuration model with unmodeled heat/fuel/motor/mass labels. Exterior cameras require a confirmed flight pose; manually opening them while docked does not fabricate a hangar view.

## Combined acceptance

Combined checks and actual engine captures are pending. Prior branch-specific results are not substituted for combined-tree validation. Fresh revision-10 authority exports are used for cargo and monitor journeys; no saved response is relabeled to imply a different registration. Per-frame proof metadata is cleared before callbacks so a culled or disconnected view cannot inherit a previous frame's evidence.

## Final combined result

- Protocol: 77 passed on the unchanged revision-10 geometry/state source.
- Client: 535 passed, the same 11 explicitly recorded baseline failures, 36 opt-in ignored tests. No newly failing client test.
- Verifier: 14 passed. Native workspace all-targets check and Clippy passed, with baseline warnings.
- Server: all 42 unit/integration targets passed: 262 passed, 0 failed, 7 ignored. The cargo exporter was explicitly run; the historical telemetry exporter and five copied-world checks were not rerun for revision 10.
- The initial combined server command failed at its final rustdoc dependency-resolution stage (E0463), after all assertions passed. With other builds finished, normal serialized `cargo test --doc` passed (0 doctests). Shared-target artifact concurrency is plausible but not proven; the initial error log is retained.
- Actual EGL: 34 combined cockpit captures plus 5 matching cargo captures passed. These include five full-face drone readouts, nine physical bays, both hull cameras, captain controls, warp/arrival, standing, the reactor and fixed cargo. The cargo 200/1000 state has all nine cameras physically stowed while equipment remains active.
- The corrected cargo receipt enumerates all nine actuators. Its five full Responses are byte-identical to the initial matching revision-10 export; no snapshot relabeling was used.

Captured-image ages remain explicit. Frozen authoritative-response replay retains correct pixels while wall-clock cache age rises; these images are not a throughput or desktop-FPS benchmark. All 13 distinct manual-plus-dedicated sources were captured over head turns with at most one fresh capture per frame. No fields, callbacks or geometry were altered to conceal unavailable data or occlusion.

Client integration changes are proof-helper-only (fresh per-frame metadata and additional views); server integration changes are receipt-summary-only. Separate preserved checkpoints remain available, and older saved worlds are not modified.
