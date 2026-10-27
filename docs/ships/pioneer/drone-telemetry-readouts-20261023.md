# Drone telemetry readouts — bounded display slice

Base: locally validated captain integration b76ce0f474c254fc6a8daf4d5fdee5722dcadaef,
privately preserved before this work. Branch: design/pioneer-drone-telemetry-20261023.
Runtime source publication is still not authorized. This is client display work,
not new drone simulation or geometry; registration9 remains unchanged and only
matching fresh development fixtures are used.

## User intent and truthful boundary

Each drone is intended to evolve into a miniature ship. The immediate request is
mining-cycle progress, task/return state, health, power, thrust and orientation.
Current DroneView supplies one mining cycle, HP, signal, position/velocity,
heading vector, mode and motion-update time. It does not contain multiple mining
modules/turrets, batteries, fuel, thrust force/throttle or full attitude/roll.
The parent ship's two modules must not be mislabeled as drone turrets. Current
mining/attack energy is supplied by the parent ship; that fact is useful but not
an invented per-drone watt reading or battery gauge.

This slice will show the one confirmed cycle as a bounded progress bar, explicit
task/Returning/Hold, HP, speed and heading yaw/pitch, with ship-supplied energy and
unmodeled power/thrust clearly distinguished. Task Mining without an active cycle
means task/waiting/approach, not proof of beam firing. UI cannot award extraction,
advance the next cycle or infer recovery/destruction from a missing drone.

## Freshness correction

DroneView.updated_at_ms records motion integration; HOLD intentionally does not
advance it. It must not be used as a universal heartbeat. Availability follows
accepted snapshot/session freshness, while pose age is separately identified if
shown. No render-frame observation refresh may manufacture fresh telemetry.
Cycle progress uses confirmed simulation time, freezes on pause/stale/disconnect,
and clamps to0–100%. Heading supports yaw/pitch but not measured roll.

## Ownership and acceptance

Client worker owns source changes/tests/GPU proof. Coordinator owns this scope
document, review, local checkpoint and private preservation. No server, geometry,
material, lighting or forbidden uniform-cache work is in scope. Keep all captain
monitor controls and actual feed behavior intact. Acceptance requires pure state
and freshness/progress tests, actual fixed-strip and detail-pane legibility,
full applicable client/protocol/verifier/workspace regression comparison, and
explicitly retained baseline failures. Unsupported mini-ship simulation remains
future work rather than decorative fabricated gauges.

## Initial validation and real fixtures

The model/App receipt suite passes14 tests. Full server ordinary tests remain
106passed; ignored count rises4→5 only because the new explicitly invoked fixture
exporter `export_actual_mining_hold_and_returning_telemetry` is opt-in. That exporter
was separately run and passed; no failing test was disabled. The test-only helper
produces real Request Responses for five active mining cycles, selected drone HOLD
with unchanged pose timestamp26502 and fresh snapshot29502, then the same drone
Returning after accepted Recall. Actual Hold is an equipment command and retains
existing job-cancellation semantics; this differs deliberately from captain
monitor-only modes. Logs: `/tmp/drone-telemetry-authority-export.log` and
`/tmp/drone-telemetry-server-full.log`; fixtures `arkline-drone-telemetry-authority-v1`.

## Implemented readouts and evidence

The five fixed cassette bands retain sticky identity, actual HP/task/speed and
now show a distinct single-cycle progress bar. LIVE means fresh accepted snapshot
availability, not recent movement; NO SIGNAL/invalid-signal and missing-source
states remain explicit. Selected drone detail adds actual yaw/pitch and labels
heading-only/roll unmodeled, no separate drone cargo, ship-supplied capacitor/no
drone battery, and per-drone power/thrust unmodeled. The accepted-response receipt
stamp already present in the App is reused; no independent observation clock or
render-refresh mechanism was introduced. Progress uses only confirmed server
time. Intentional long pause remains PAUSED unless disconnected, not false
staleness merely from the absence of motion updates.

Final actual App/EGL proof V4 passed27.67seconds with13captures from unchanged
real server Response payloads delivered through Update::Response/poll. The proof
uses supported header collapse, ordinary head turns and actual panel scrolling.
Coordinator inspected mining bars, HOLD, Returning and the complete lower detail.
Pause/stale/disconnect preserve the same confirmed31.25% cycle sample. Camera
images retain their independent cached-age labels; this is not a real-time video
or desktop FPS claim. Evidence: `arkline-drone-telemetry-client-v4/receipt.json`.

Exact-tree client regression:521passed, the same11retained baseline failures,
33ignored. The extra ignored client GPU journey was explicitly run successfully;
no failing test was disabled. Existing captain input/identity/AI-preservation tests
pass. Verifier13library+1CLI integration pass. Geometry/protocol did not change;
registration9 remains in force. Server106ordinary tests pass and the fifth ignored
exporter was separately run as described above. Native workspace and clippy final
results are recorded in the accompanying validation archive. Browser/wasm target
remains absent and is not claimed tested; public runtime source is not published.

Native workspace all-targets compilation passed. Native clippy completed with
warning messages matching the b76 baseline and no new warnings. Four new/changed
telemetry modules pass rustfmt checks and the complete diff passes whitespace
checks. All checks are terminal; the source is frozen for a private checkpoint.
