# Timed unloading / client acceptance integration — 2026-10-24

## Scope and source boundaries

Local integration of the explicitly approved work historically tracked in issue #62.
The user subsequently requested no more PR/issue tracking; continue this local
punchlist without reopening or requiring a public claim. Existing code and
branches remain preserved. Base:
`1c068a3aa360a568ab92ae5dc3ac390fb0b65b3a`. No runtime source publication,
merge, deployment, or existing-world migration is included.

Accepted client repair checkpoint: `9d1c1fb27185be6944d9aecffc16097e70ae0295`.
Its exact four-file test-only delta is integrated as `d0206f8`. It corrects
obsolete labels, composite passive-room expectations, foreground window ownership,
scroll-visible pointer targets, and relative mouse capture expectations. It does
not change production camera, command, or render behavior. Independent review
confirmed exact powered-emitter counts and zero-emission passive construction
remain guarded; real command/avatar invariants were not removed.

The restored repository deliberately retains eight unavailable historical proof
blob references. They are unchanged tracked entries, not deleted or fabricated
files. Local commits use `write-tree --missing-ok` and `commit-tree` with the exact
current parent and an expected-old branch update. Each changed blob must exist;
all eight unchanged historical entries must compare identically. Source backups
must state this limitation and retain the actual current Blender/GLB assets.

## Test status at integration start

- Baseline worker reproduced 549 passed / 11 failed / 43 ignored on untouched
  base. Repaired client: **560 passed / 0 failed / 43 ignored**, twice.
- Six editor failures independently reproduce on untouched base and repaired
  source (291 passed / 6 failed / 1 ignored). They are not included in the eleven
  client repairs and remain a separate release gate.
- Non-strict workspace check and Clippy passed; strict warnings-as-errors still
  fails on unchanged protocol/server warnings on both base and repaired source.
- Native legacy foreground-camera proof passed (1/0), with five inspected EGL
  images. The initial harness embedded-mode setup was corrected to use the real
  desktop Ready/Response path; no production code changed. Timed-unload integration
  and native cargo proof remain pending.

## Native proof contract

`native_baseline_foreground_camera_controls` uses an actual in-memory legacy
LinearV1 server, layout-version-1 Connect, real foreground-layer pointer clicks,
real keyboard input and the production EGL painter. It records first/third person,
unchanged avatar position during camera selection, blocked walking while the
workspace owns input, and Escape returning to the unobstructed world. This is not
new DockingV1 walking acceptance.

`native_authoritative_timed_unload_capture` replays actual server-exported Responses
at the declared 100 ms cadence using the production cargo workspace. Disconnect
and Ready are applied through the real client update path. Offline data is retained
cached state, not newly invented private receipts. Per-frame authority and PNG
hashes, visible text and connectivity are retained. No crane/container motion is
claimed. Existing docking motion capture retains its stricter no-replacement-
services-view invariant; explicit UI proof is a separate path.

Final combined commit, terminal command receipts, inspected images and durable
backup identifiers will be recorded after execution. Until then this is a plan and
partial integration record, not an all-green release declaration.

## Integrated cargo checkpoint and visible-control correction

Accepted authority source: `e443229c4793b10581b97c7e1b7959d7fbb98cc8`.
Integrated with the client repairs as `961a2e3`; combined client suite: **564
passed / 0 failed / 45 ignored**. The cargo producer exported 131 after Responses
and 31 retained-legacy before Responses at 100 ms, with actual ledger-verified
ore conservation. These are not physical crane animation or a measured live FPS.

The first complete native replay passed its technical assertions, but pixel review
rejected it as useful visual acceptance: active progress and Cancel/Resume sat
below the stored-fleet rows. Renderer `1625bb8` moves the same guarded status block
directly below the active hold, leaving command semantics unchanged. A 600×500
long-inventory regression passes, and final native replay now requires operation
title and active Cancel to remain clip-visible. Final replay receipts and aggregate
results are still pending; source and renderer provenance are deliberately separate.

## Final combined acceptance (production renderer `1625bb8`)

- Protocol/server/verifier: **386 passed, 0 failed, 10 ignored**, 51 result suites.
- Client aggregate: **565 passed, 0 failed, 45 ignored**. Three nested subprocess
  one-test receipts are not added again to the main client aggregate.
- Native cargo: **131 after + 31 retained-legacy frames**, both tests passed; all
  162 PNG hashes verified. Native operation titles and active Cancel remain visible.
- Earlier shared-capture regression: actual legacy foreground camera proof passed
  (five images); retained DockingV1 seat/walk/reseat/departure proof passed (eight).
- Independent reviewer inspected final PNGs and decoded GIF frames, confirming
  held 50 remains fixed, resume reaches 75 then 100, cancelled second transfer retains
 25/57 with zero reservation and 32 onboard, and exact retry leaves that unchanged.
- Full-resolution GIF is 1280×800, 131 frames/13.1 seconds, every frame 100 ms; legacy
  comparison is 31 frames/3.1 seconds. No timing interpolation or fabricated footage.
  The real disconnect screen is preserved. Native capture is software-rendered
  controlled-clock replay, not a representative release/hardware FPS benchmark.

The independent producer is still `e443229`, not relabeled as the later renderer.
The before comparison is the retained legacy path in that same modified binary,
not historical pre-feature footage. Source/asset backup retains the exact canonical
Blender/GLB files and all changed blobs, with explicit sparse-base limitations.

Remaining gates: six previously reproduced editor failures, workspace formatting
(including some shared insertion sites), strict-Clippy warnings, timed loading,
normal-world adoption, physical cargo equipment/power, durable journal paging,
release/hardware measurement, wider network/long-run tests and unfinished ship art
and physical systems. This checkpoint is not a declaration of finished flagship.

Final workspace `cargo check --workspace --offline` and non-strict
`cargo clippy --workspace --all-targets --offline` both exited 0. Warnings remain;
this does not supersede the explicit strict-Clippy and formatting release gaps.
