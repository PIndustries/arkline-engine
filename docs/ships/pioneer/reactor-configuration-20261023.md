# Pioneer reactor configuration — recovered development prototype

## Recovery and ownership

This is a newly reconstructed replacement for the lost local reactor checkpoint,
not a byte-identical recovery of commit `20bd7ea` or its original test results.
The published base is `f2f3bb7071447ca7fd063728ec7b0ba1fd4fb068`; local replacement
branch is `recovery/reactor-rebuild-20261023`. Work remains within issue 62's
Pioneer development scope. Public progress/source publication is paused pending
specific authorization; recovery is local only. No old world is migrated.

The coordinator owns shared geometry and documentation; the authority worker owns
server integration and protocol state; the runtime worker owns client rendering,
verifier literals and actual-engine proof. No manual captain display controls,
side-monitor redesign or external hull/weapon articulation is included here.

Nine client files were recovered exactly from previously uploaded Git blobs.
Reactor authority/state, shared geometry and scene integration were reconstructed
from retained source output and specifications. Original source bytes, test logs
and GPU fixtures were lost; new validation is required and recorded separately.

## Authored mechanical configuration

This is a fictional mechanical blockout, not a nuclear engineering design.
Four rigid segmented collars translate axially around a fixed core. Neither the
core nor moving parts scale. Body-frame module center is `[-5.55,-2.82,5.90]`.
The core is 2.6 m long. Collar centers move from X offsets
`[-1.20,-0.40,0.40,1.20]` to `[-0.42,-0.14,0.14,0.42]`.
The collar bank therefore compacts while the fixed core/rail envelope remains.
Hollow shoes surround the two rails; solid collars may not cut through rails.

Installation is gated to the canonical D1-S1 power-room placement and dimensions.
Edited or unsupported room layouts do not receive a silently misplaced module.
Static core, plinth, brackets and rails plus actual dynamic collar geometry and
remaining swept volumes participate in collision. Obstruction stops the whole
synchronized assembly with a truthful blocked state rather than passing through
crew. Existing save bindings below registration 7 must refuse without migration.
Registration is not a universal marker for never-traversed worlds or a universal
mixed-build connection guard: use fresh disposable worlds and matched builds.

## Authority and display

Stable owned-vessel state controls a 1.2-second full movement. Accepted warp or
jump state targets compact configuration; actual normal state targets expansion.
Rejected requests must not move the mechanism. Reversal starts at the actual
fraction. Restart, pause and inactive-vessel behavior must preserve safe poses.
Existing warp does not wait for successful closure; blocked closure is not
reported as secured and no new warp interlock is implied.

Permanent display 15 is aft-starboard, independent of the seven mining screens.
Its schematic draws the same fixed core and actual collar poses, with honest
target/moving/blocked/completed states. Capacitor charge/capacity and regeneration
rating are existing authority data. Heat, fuel, motor energy and installed mass
remain explicitly UNMODELED. No reactor power/thermal physics or extra energy
debit is invented. The screen is raised on real support struts so its lower
canvas/footer clears the retained sloping console fascia.

## Replacement acceptance plan

1. Protocol: canonical installation gate; constant part sizes and core; no collar
   overlap throughout 101 poses; rail fit; full glass-edge sight lines.
2. Authority: real accepted/rejected warp, reversal, obstruction, pause/reconnect,
   restart/database persistence, no additional capacitor debit, binding revisions
   4/5/6 refused byte-identically, fresh 7 accepted.
3. Runtime: same geometry/pose for rendering, occlusion and input; fresh actual
   engine captures of expanded/mid/compact and reopen states, plus readable
   configuration display. Diagnostic room camera must be identified as such,
   not passed off as a player walking route.
4. Re-run protocol/server/client/verifier checks. Separate inherited failures from
   attributable regressions. Historical lost-checkout passes are not evidence for
   this replacement tree. No published/CI/merged/deployed claim without proof.

Replacement validation results follow; this is not historical lost-checkout evidence.

### Replacement results (2026-10-23)

- Final full protocol suite: 71 passed, including three added interpolation,
  hold and serialization checks.
- Native client `cargo check --tests`: passed.
- Native client full library suite: 496 passed, 11 failed, 31 ignored. The eleven
  failure names match the retained published-base failure list; this is not an
  all-green client suite. Both reconstructed scene checks and all five reactor
  sampler tests passed. No historical result was substituted for this run.
- Initial focused reactor authority suite: 9 passed. Newly recreated tests use
  real requests for warp/arrival, pause/reconnect and SQLite reopen. Additional
  optional-field compatibility coverage and strengthened full-room capsule
  preconditions are included in the final server rerun.
- Fresh real authority fixture: 29 snapshot JSONs, 29 complete response JSONs and
  receipt exported. This is fixture evidence, not a live player session.
- Full server library suite: 101 passed, 0 failed, 4 ignored; includes all ten
  reactor authority/compatibility tests. Server Clippy passed with the existing
  traversal boolean warning.
- Fresh actual-engine EGL proof: passed in 48.13 seconds, 38 captures at
  1280×720. Expanded, half and compact room views plus six cockpit milestones
  were captured from the fresh real-request fixture. Image files decode and
  declared raw RGBA hashes match. Coordinator inspected expanded/compact oblique
  views and cockpit half/fully reopened states: rigid core/collar dimensions,
  readable unmodeled footer and retained console fascia are visible. This is
  diagnostic mechanical blockout evidence, not polished art or desktop FPS.
- Workspace all-targets native check: passed. The previously missed verifier
  integration-test PilotView literal was corrected; final full verifier rerun
  passed: 13 library tests plus one checkpoint CLI integration test.
- Final post-format client rerun retained 496 passed / 11 same failures / 31
  ignored. Browser target is not installed in the
  restored toolchain, so no new browser compilation claim is made.

Validation logs and PNG/receipt evidence are kept outside the source checkout.
No public reactor branch update, CI success, merge or deployment is claimed.
