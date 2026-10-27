# Strict-warning quality gate — 24 October 2026

Local continuation from the durable green-workspace checkpoint
`475eba0b80cca856f1290eb928f3dbf2b361c9f9`. No public issue/PR tracking, source
publication, deployment, world migration or previously denied cache work.

## Bounded changes

Reviewed final worker source `f9ac7efaddc1d654aaabaf8133425c1bc18a00d2` is
integrated as `f6a2089`, with identical tree
`efc5f919f296c8491ad6bf6e66617bcd39ad9505`.
The bounded warning pass touches fifteen files; a separately reviewed editor
fixture synchronization follow-up is documented below. Neither is the later,
separately proposed global formatter.

- Preserve readiness and owner interlocks while simplifying equivalent Option
  booleans, needless borrows, copy clones and nested conditionals.
- Enumerate checked dial slices. The reviewer rejected an initial `.take(count)`
  variant because it could silently truncate a future out-of-range count; the
  final checked slice retains the original bounds-failure guard.
- Replace two deprecated egui enable calls with their exact pinned egui 0.31.1
  implementation: `if !enabled { ui.disable(); }`. The true branch never enables
  a parent-disabled child. Physical widget IDs and absolute rectangles stay fixed.
- Keep test-only navigation/power/walking helpers under test cfg, preserving all
  existing coverage. Retain macOS GPU accessors under their matching macOS-test
  cfg. Remove only a separately audited zero-caller private formatter.
- Move the radar test module intact after production items; its text and test
  coverage are unchanged. Do not mistake the textual move for new functionality.

No warning suppression attributes, broad refactor, telemetry invention or runtime
feature policy change is included. Exact canonical assets and registration remain
unchanged. Cross-platform tests are not inferred from the Linux build.

## Acceptance protocol

A new actual pointer regression exercises UNDOCK and STOP in ready, disabled-parent,
paused, offline, pending and stale states. Ready cases must emit the correct
request; blocked cases must emit none; snapshots remain unchanged.

Final exact-source full workspace tests pass: 1,263 passed, 0 failed, 56 ignored
across 58 top-level suite summaries (three nested subprocess summaries are not
counted twice). Client: 566/0/45; editor: 297/0/1. All-target check and strict
all-target Clippy (`-D warnings`) both exit 0. The actual-pointer regression passes.
The corrected scene synchronization fixture also passed five consecutive focused
runs. The integrated native seat/walk/reseat/departure test also passes with eight
actual engine frames. Inspected secured-stale, reseated-live and departing frames:
physical button placement stays fixed, stale controls remain dimmed, live UNDOCK
and departing HOLD/STOP remain legible. The fixture honestly shows initial stale
state and unassigned drones; it is not a new fleet/mining or performance proof.
Mesa software rendering was used. Other ignored/native and macOS suites are not
inferred green from these checks.

Global formatting is a separate gate: the pinned formatter's read-only dry-run
on the prior checkpoint found 45 Rust files and 6,303 diff lines. It must be an
isolated formatter-only commit after this checkpoint is durable, with full checks
and unchanged asset/source-contract evidence, never mixed into the logic review.

## Exact-rerun fixture race found during validation

A later complete run exposed an intermittent pre-existing editor fixture race:
`scene_state_pending()` can clear when the scene acknowledgement arrives before
its snapshot on a separate channel. The test then inspected the old Cockpit state.
The production worker sends the acknowledgement before independently publishing
that snapshot, so an acknowledgement alone is not proof that the consumer has
observed the requested deck yet.

The approved follow-up changes only the editor test: issue one placement request,
retain its ID, wait for its matching successful acknowledgement and the confirmed
InteriorDeck2 snapshot, then perform the unchanged camera/body/paused-world checks.
No mutation replay, production timing change, increased timeout or weakened guard.
Final acceptance must include this follow-up, not reuse the intermediate green run.
