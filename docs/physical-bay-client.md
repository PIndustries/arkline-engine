# Source-bound physical bay client preview

This is an explicit local inspection consumer of the existing controlled-world
factory, not a public server, a normal account mode, or a fleet/warp admission.
The ordinary native/browser startup does not choose it.

`PhysicalBayClient::new` and `run_physical_bay_preview` accept
`PhysicalBayClientOptions`: exact genesis and bay/control bindings, world path,
client preferences path, caller-provided owner token and accepted starting clock.
The token is not generated, printed, or persisted by the client bridge. The server
retains its existing owner-credential verification. A saved world must be reopened
with its original policy and an admissible clock; reopening freezes prior intents,
not off-line extraction. No listener is opened and no remote transport fallback is
used if this mode fails. Back/Disconnect signal the authority worker to stop and
revoke presentation/input without joining on the UI thread. An in-flight
transaction finishes atomically; queued commands do not start after the worker
observes stop. Dropping the preview joins the worker. Reopening requires the
explicit local preview with the original owner/source policy, not generic reconnect.

This inspection bridge advances exactly one accepted 100 ms segment, then waits
another 100 ms after completed work. It never catches up by inventing skipped
machinery motion. Debug work adds to wall duration: a capture clock is simulation
time, not an achieved real-time cadence or FPS measurement.

Periodic presentation observations have one outstanding slot. While that slot is
occupied, authority ticks and their transactions continue but no further periodic
Snapshot request is generated. Commands, their responses and failures remain
ordered and lossless; an explicit Snapshot command still receives its response.
This is observation backpressure, not skipping simulation or cargo events. Each
physical response carries its production timestamp internally; consumption does
not make an old response fresh. Invalid input or worker failure revokes the session
and ends the current response batch rather than allowing later queued state to
restore it.

The immutable compiled 1704 registration is hashed/validated once and cached as an
owned-clone result (including sticky failure). Every actual asset load still
checks its source bytes and binding. Every delivered response still undergoes
source/owner/pose validation. Older source revisions retain their existing path.

The existing Cockpit consumes responses through its normal update channel and
sends the existing Launch, Lock, Assign, Hold and Recall requests. The physical
response is additive, explicit in cm³ and real capacitor joules; legacy ore fields
are never used as volume. A missing, stale, contradictory, wrong-owner or changed
source receipt fails closed. Flight and unregistered controls are not available.
Monitor Hide/Show is presentation only and does not issue an authority request.

The renderer appends exact source geometry after legacy mesh slots. Registered
moving roots and the two drone inlet panels use confirmed source poses. It does
not advance contacts, valves or cargo from display time. The same resolved parts
feed color, depth and pointer geometry. Existing 1701–1703 assets and worlds retain
their own gates. Geometric volume is not a mass or actuator rating.

## Acceptance boundaries

Automated response/command tests, a local in-process authority bridge, native
Mesa/EGL rendering, an OS-window interactive session, packaged Mac behavior and
performance are distinct evidence. Report each separately. The native capture
harness drives genuine authority commands and snapshots; its inspection camera is
not evidence of a newly installed onboard camera. Fixed target, authored rates,
frozen unrelated machinery and the single Mining bay remain explicit limits.

The next connected gates after this consumer are user-operated native acceptance,
all-bay/fleet concurrency, movement/docking/warp, frame pacing and input latency.
No public admission or publication follows merely from passing this preview.

## Live measurement harness

The ignored `live_physical_bay_frame_and_bridge_measurement` test runs the existing
local bridge, real egui pointer controls and actual EGL `Renderer::paint` with
`glFinish`, without per-frame screenshots or documentary snapshot exports. It
requires a fresh `ARKLINE_LIVE_OUTPUT` directory and explicit
`ARKLINE_CAPTURE_SOURCE`; `ARKLINE_LIVE_SECONDS` controls the observation window.
The report records renderer/profile, cold startup and RSS, per-frame poll/UI/paint
cost, cadence, source-produced age, positive command dispatch/acknowledgement,
Hide/Show state changes and shutdown. It saves incomplete evidence before failing
if the five-action sequence or source scene did not complete. This is a fixed
inspection camera and control panel, not the whole native window or onboard feed.
Separate pointer lifecycle tests cover Back/Disconnect/reopen and rejected batches.

On the 2026-10-26 cloud host, no desktop display was connected. Starting installed
Xorg/dummy with TCP disabled could not create its local socket; AF_UNIX was denied
by the sandbox. The supported escalation failed during sandbox mount setup before
Xorg ran. No security setting, protected path, user Mac or public listener was
changed. OS-window acceptance therefore remains open; EGL evidence is not a
substitute claim of compositor-visible responsiveness.
