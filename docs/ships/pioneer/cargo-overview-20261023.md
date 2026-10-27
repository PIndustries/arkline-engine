# Dedicated cargo overview — fixed instrument

Base b19c4ae2604b29f6649b7f1fde913e099df96430 is privately preserved. This client
slice uses permanent starboard surface7, independently of moving camera panels.
Branch design/pioneer-cargo-overview-20261023, registration9 unchanged. No public
runtime upload, old-world migration or nine-panel integration is implied.

Requested glanceable readouts: actual onboard ore units, capacity, free capacity,
wide fillbar and real ship identity/freshness. Source is PilotView.ore and shared
cargo_capacity(ship_class), not station storage. For Pioneer the current static
capacity is1000 generic ore units. No weight, volume, mineral taxonomy, containers,
throughput or loader animation is invented. Overcapacity shows actual units with
clamped bar/free and an explicit invalid-state warning.

Operation context is visually separate: matching ResourceJob phase/target and
cumulative mined/stored are job history, not current hold contents. Depositing is
not fractional transfer progress: current inventory transfers are atomic.
Accepted-response snapshot freshness is reused, without a new observer or
render-frame refresh. Stale/paused/disconnected data remain identified. Existing
capacitor/module readings remain on the permanent power/reactor instruments;
monitor controls and command semantics are unchanged.

Acceptance: pure empty/partial/full/overcapacity/freshness coverage, actual server
cargo request fixtures, actual fixedscreen legibility and camera-stowed persistence,
full applicable regression comparison and private source preservation. Separate
nine-camera geometry and full-face drone telemetry graphics remain later slices.

## Validation and persistence-test correction

Actual request fixtures cover onboard0, mined200, the same200 with all seven
camera monitors stowed, full1000, and real deposit to onboard0/stationstorage1000.
The new ignored server exporter is explicitly executed, not a disabled failure.
Client pure model4tests pass; actual EGL/App proof V2 has5captures and passes15.69s.
Coordinator and parent inspected partial-stowed/full screens; primary used/free/
capacity and fillbar are legible, with subordinate jobhistory separately labeled.
Client525passed, the same11retained baseline failures,34ignored. Verifier13+1pass;
native workspace/clippy pass, no new warnings. Browser target remains unavailable.

The broader server integration run exposed a previously untested assumption in
journey.rs: after returning from warp, the reactor is legitimately still reopening,
so two network snapshots6ms apart differ in its update timestamp. No cargo code
changed that production behavior. A bounded5second actual-mechanism-settled poll
was added before the stable-persistence comparison. Exact non-power PilotView
and restart equality assertions remain unchanged; no reactor fields are stripped.
Midstroke persistence remains independently tested. The focused network journey
passed66.39s after this precondition; a complete server rerun is recorded with the
checkpoint rather than silently labeling the first failure as a known baseline.

Complete final `cargo test -p arkline-server` exited0: **258passed,0failed,7ignored**
across library/integration/doc-test groups, including all4journey tests. The library
subset is106passed/6ignored; the sixth opt-in exporter is the separately executed
cargo fixture generator. One other integration test was already ignored. This
complete rerun closes the newly discovered stable-persistence test precondition
issue without weakening equality or changing production actuator behavior.
