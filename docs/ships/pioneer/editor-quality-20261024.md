# Editor acceptance repair — 24 October 2026

Local release-quality continuation after the durable timed-unloading checkpoint
`37929b53558be91ec584106e9e28832a3101caa9`. No new issue/PR, runtime publication,
merge, deployment, world migration or production refactor is included.

## Independent baseline and repair

The exact parent independently reproduced **291 editor tests passed, 6 failed,
1 ignored**. Accepted worker checkpoint `ce94f4c597be76b79b6f0d3910861028291b71de`
changes four editor test files only. Integration `ec4c50c` has the identical tree
`cb0d4dc549d60b87a81ef318b0ff4352d7b93d6c`.

- Current labels are Play, Resume simulation and Mining operations. Tests follow
  the actual navigation route back to Editor when resuming intentionally enters
  Play; they still verify paused/running state and positive clock controls.
- The canonical mesh has an asymmetric raised-bridge envelope. Import validation
  now checks its exact hash and independently recorded authoring/adoption bounds,
  retaining transformed WEDGE/SPIRE no-fitting checks. The modeled height is
  20.35 m; the 23 m ship specification is a conservative origin-centered envelope,
  not the mesh height.
- The first rendered scene initializes its body camera. The focus-retention test
  now settles that initialization before explicitly choosing Third Person and
  recording its baseline. Camera, avatar and paused-world invariants remain.
- The compact room inspector is genuinely scrollable. Tests use real wheel input
  until the entire name field is visible at the original 1080×720 viewport, then
  real pointer/text input. Full text clipping, edited-name and paused-clock guards
  are retained; the screen is not inflated and offscreen text is not clicked.
- Session repeats the pilot name in background UI. Tests target the foreground
  Session window/layer and scroll as needed before clicking the actual text field.
  Keyboard ownership and F5 negative/positive clock controls remain asserted.

No assertions were suppressed, no tests were newly ignored and no production
code or canonical assets changed. The eight unavailable historical proof-asset
references remain identical, with all changed test blobs present.

## Acceptance

Focused full editor run: **297 passed, 0 failed, 1 ignored**. Standard full
workspace test run: **1,262 passed, 0 failed, 56 ignored**, across 58 top-level
result suites. Three nested one-test client subprocess receipts are excluded from
that aggregate. Client is **565/0/45**; editor is **297/0/1**. The full workspace
command exited 0 on worker `ce94f4c`, whose exact tested tree is identical to
integration `ec4c50c`. Subsequent receipt changes are documentation only.

All-target workspace check and ordinary Clippy exited 0. Explicitly ignored
native/fixture tests are not falsely counted as executed by the standard run;
prior timed-unload native proof remains separately attributed below.

Seven authored test functions pass isolated rustfmt checks. Workspace formatting
and strict-Clippy warnings remain separate release gates; successful non-strict
checks must not be described as a warning-free or style-clean release.

The prior timed-unloading native evidence remains attributed to its actual
producer `e443229` and renderer `1625bb8`. Test-only editor repairs do not create
new footage or imply a finished flagship art/physics model.
