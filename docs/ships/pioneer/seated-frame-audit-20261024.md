# Pioneer native frame and camera-age audit — 24 October 2026

This is a bounded diagnostic, not a performance improvement or release-FPS claim.
Production rendering and capture policy remain exactly the seated-readability
checkpoint `b4d86ae`. Only ignored measurement/export tests were added.

## Method and limits

- Cloud Mesa llvmpipe (LLVM19.1.7,256-bit), OpenGL4.5 compatibility,
  Mesa25.0.7-2+deb13u1. Rust unoptimized/debug test build, **not hardware GPU**.
- Fresh in-memory authority, ordinary Undock/StartResourceJob and actual ticks.
  80 unchanged complete Responses at40ms simulation intervals; every response
  verifies five same drones, two targeted modules, same active job and nine fully
  deployed mechanisms. No response timestamp was manufactured.
- Both900×650 and1280×720 use canonical neutral eye/FOV, default visible UI,
  identical16warmup/48measured/16drain frames. Advancing cases replay the same
  ordered Responses. Frozen cases stop delivering after warmup; they are not an
  intentional paused-session benchmark.
- Timers separate App update, tessellation, callback/driver submission and explicit
  `glFinish` wait. No screenshot readback occurs within measured frames. Final
  readback is separate. CPU submission **includes software-render work and driver
  stalls**; it is not evidence of a pure application-CPU bottleneck. Finish wait
  likewise is not total GPU execution time.
- Replay runs at host speed, not real-time network cadence. Because the slow host
  takes longer than40ms per frame, the standalone wall-based authority footer
  correctly grows stale relative to the replayed server clock, even while newly
  received cassette telemetry is LIVE. Do not hide or reinterpret that warning.
- Each source timestamp is a UI-frame-start stamp. Image UI age and represented
  simulation age are separate. Last-retained asynchronous view profiles can be
  old and `drone:unbound` aliases camera types: never sum them into a frame total.
- The default900neutral frustum excludes D1/D5, leaving D2–D4, both turrets and
  both exterior cameras.1280 shows all nine. Thus resolution comparison also
  includes a seven-versus-nine visible-camera workload difference.

## First matched run

48 measured frames per row, actual source/PNG hashes verified. Medians below use
ordinary middle-pair averaging; p95 is nearest-rank. These are one controlled run,
not broad statistical or production performance guarantees.

| Case | Through-finish median | p95 | Actual captures |
| --- | ---: | ---: | ---: |
|900×650 advancing authority,7visible sources|180.78ms|223.87ms|48|
|1280×720 advancing authority,9visible sources|219.87ms|433.61ms|48|
|900×650 settled no-update plateau,42frames|141.60ms|175.92ms|0|
|1280×720 settled no-update plateau,40frames|176.37ms|209.50ms|0|

The frozen cases first finish6/8pending captures, then settle. Do not mix that
transition tail into a purported steady paused-state result.

Advancing App-update medians were about44.8/47.5ms; callback/submission medians
about130.9/163.0ms; finish wait about2.3/2.4ms. Separately timed final readbacks were
under3ms. Phase medians are not additive. This localizes work to App preparation
and the broad callback/driver phase, but does not separate Rust work from llvmpipe
rasterization or synchronous driver behavior enough to select an optimization.

One actual camera capture was admitted in every advancing measured frame. Every
visible source refreshed:6–7times per source at900,5–6times at1280. Oldest image
UI-age medians were1092/1869ms, with p95s1195/2457ms. Oldest represented simulation
ages were240/320ms. No visible-source starvation was demonstrated. Candidate
frustum filtering and callback geometry are equivalent, and callbacks use the
full scene viewport; no offscreen winner mismatch was found.

## Decision

No production optimization is selected from this software/debug result. In
particular, neither the one-capture policy nor accepted visibility/physics is
changed, and the separately denied uniform-cache work is not revisited.

The next performance gate is a release-build, real-GPU measurement with matching
camera workload, real-time authority cadence, frame presentation/input timing
and repeated runs. That hardware is not supplied by this cloud renderer. The
current evidence is useful for identifying refresh-throughput pressure and
retaining an honest reproducible baseline, not claiming finished performance.
