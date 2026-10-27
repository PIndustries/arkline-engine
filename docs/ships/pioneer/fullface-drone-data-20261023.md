# Full-face drone data panels — client display slice

Base: `28d52f0cd7f4fd2e1b8068e0bd11f91c7000efa8` (validated cargo checkpoint).
Branch: `design/pioneer-fullface-drone-data-20261023`. Local-only work;
public runtime source publication remains unauthorized. Registration 9, physical
geometry, camera surfaces/capture budget, captain commands and cargo remain unchanged.

## Request and visual contract

The user's supplied crop `image(20261023-153021).png` was inspected before editing.
It shows unused black space above the existing lower cassette readouts, not a
request for another camera stream. The five fixed data faces now use that upper
area for a heading schematic and independent pitch ladder on the left, plus
actual HP and signal gauges on the right. The lower glance zone keeps the sticky
source ID/class, availability, task, speed and wide single mining-cycle bar.
These are read-only shapes on the real physical panel, not video or generated art.

The outline rotates only to the accepted heading vector. Compass reference is
world +X forward / +Z right, not magnetic north or ship-relative bearing. Pitch
comes from the same vector; no roll is inferred. Exactly vertical or invalid
heading has no useful yaw and the compass marks it unavailable. Other invalid
heading/velocity fields are not replaced by fabricated zero readings.

HP maximum comes from protocol `DRONE_HIT_POINTS`, the actual launch-health
constant. Gauge validation preserves the raw value separately from a bounded
fraction. Negative, above-maximum or nonfinite values do not become healthy/full
bars. Signal displays the reported 0–1 value, with explicit NO SIGNAL at zero and
invalid/unavailable markings for out-of-range/nonfinite values. Empty or missing
sticky sources have no fabricated health, heading or cycle.

## Time, state and power boundaries

The accepted-response/session freshness and confirmed-time model from the prior
telemetry checkpoint is reused, without another observer or render-frame refresh.
HOLD's old motion timestamp is not a signal timeout. Pause, stale receipt and
disconnection preserve the same confirmed heading, HP, signal and cycle values,
with explicit status and amber gauges while the cockpit is visible. Actual
disconnection hides the cockpit/readouts in the controlled harness, rather than
continuing to display stale glass; underlying confirmed values remain frozen.
Only the disconnect notice is visible there; reconnect controls are not proved. There is no idle waveform, fake pulse,
clock-driven heading motion, next-cycle prediction, extraction award or invented
completion. New accepted samples naturally update the schematic and progress.

Mining task without a valid cycle remains waiting, not proof of extraction.
Returning and HOLD use their real authority modes. The one drone cycle is not
confused with the ship's two mining modules. Existing expanded drone details
retain ship-supplied power, no battery, and unmodeled per-drone power/thrust/roll.
No additional power simulation or gauges were invented for the formerly blank area.

## Evidence

Actual source inputs are the unchanged real Request Responses in
`arkline-drone-telemetry-authority-v1`: mining, HOLD with an old pose timestamp and
fresh snapshot, and Returning. The original Hold command legitimately cancels
its resource job through existing server policy; this display does not issue it.

Final proof is `arkline-fullface-drone-proof-v5/receipt.json`. It uses a fresh
profile, accepted Update::Response/poll delivery and actual App/EGL rendering.
Standard 1280×720 and genuinely rendered narrow 800×650 views use ordinary head
look, not cropped or composited screenshots. Head turns are required to inspect
all five wraparound faces; simultaneous forward readability is not claimed.
The supported status-header shortcut and existing interaction title-bar collapse
are used for the clean preview; default-open images remain in the same proof.
The collapse is checked not to emit an underlying cockpit command. No UI hiding
feature was added. Camera images keep their separate cached-age labels.

No fake damage or signal payloads were used in the real-state captures. Invalid,
zero and maximum HP/signal plus cardinal/vertical/wrapped heading are covered by
pure tests. The stale proof waits actual elapsed receipt time; paused and
disconnected cases are actual App states and assert unchanged gauge/heading/cycle
values. Per-frame proof metadata is cleared before drawing, so the disconnected
state correctly records no visible cassette readouts rather than reusing a
previous frame. Empty-bay text uses separate identity/status cells; a pure
painter test asserts all text stays inside the face without overlap. This
headless llvmpipe replay is not a desktop frame-rate or live-stream
performance claim.

## Validation

The final validation receipt records exact source/log hashes and baseline failure
names. Full client: 531 passed, the exact 11 inherited failures retained, 35 ignored.
The new ignored GPU proof is separately invoked, not a disabled failing test.
Verifier: 13 library + 1 CLI integration passed. Native workspace all-targets check
and client all-targets clippy pass, with no new warning messages versus the cargo
checkpoint. The new/scoped files pass rustfmt and the complete diff passes
whitespace checks. Browser/wasm is not run because that target is unavailable.
Server/protocol production source is untouched; their validated cargo-checkpoint
results are inherited, not represented as freshly rerun here.

The first GPU compile stopped before test execution with OS error 28 (disk full).
The coordinator freed only confirmed inactive historical incremental artifacts;
the normal build was then rerun successfully. No source/proof/backup was removed,
no current build cache was redirected and no denied uniform-cache work occurred.
