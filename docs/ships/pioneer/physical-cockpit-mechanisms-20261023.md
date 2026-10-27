# Pioneer physical cockpit mechanisms — registration 6

Issue: 62. Branch: `design/pioneer-whole-space-20261023`.
Predecessor: published spatial checkpoint `49c17e9804c06427c423136303b16ea052506d23`,
registration 5. This is a new isolated development registration; neither revision
4 nor revision 5 saved worlds are migrated or opened as revision 6. Existing
admission revisions are stored on traversal bindings, not every never-traversed
world: refusal tests guarantee bound old worlds remain byte-identical. No
universal old-world rejection is claimed without a world-level version marker.

## Current requirement and ownership

Five physical screens lower from ceiling cassettes, one per assigned drone.
Two physical screens rise from center-console cassettes, one per mining turret.
All seven show their respective views simultaneously when fully deployed; paging
is not a substitute. Accepted warp authority stows the overhead five upward and
the console pair downward. Permanent navigation/command/overview instruments
remain available; a dedicated future reactor configuration overview is separate
from these seven mining views.

- Protocol geometry, fit and documentation: cockpit redesign coordinator.
- Authoritative state, persistence, obstruction and server tests: physical screen
  authority worker (`apps/server/src`, new protocol mechanism state).
- Shared-pose client rendering, depth/input, camera budget and actual App proof:
  cockpit display runtime worker (`apps/client/src`).

The previous fixed-glass drawer implementation is superseded. Reusable source
identity and capture-spec correctness work does not itself satisfy physical
motion. Implementation evidence and remaining acceptance gaps are recorded below.

## Geometry contract

`cockpit_mechanism_screen(slot,fraction)` returns body-metre center, half extents
and axes. Slots 0–4 are ceiling screens; 5–6 are turret screens. Fraction 0 is
stowed and 1 deployed. Motion is vertical, with fixed orientation and size.

| Hardware | X | Z | Stowed Y | Deployed Y | Yaw | Glass case width × height |
|---|---:|---:|---:|---:|---:|---:|
| Drone 1 | 32.90 | −1.60 | 2.45 | 1.95 | +60° | 0.64 × 0.36 m |
| Drone 2 | 33.65 | −0.84 | 2.45 | 1.95 | +25° | 0.64 × 0.36 m |
| Drone 3 | 34.10 | 0 | 2.45 | 1.95 | 0° | 0.64 × 0.36 m |
| Drone 4 | 33.65 | +0.84 | 2.45 | 1.95 | −25° | 0.64 × 0.36 m |
| Drone 5 | 32.90 | +1.60 | 2.45 | 1.95 | −60° | 0.64 × 0.36 m |
| Turret 1 | 33.45 | −0.32 | 0.42 | 1.35 | 0° | 0.56 × 0.32 m |
| Turret 2 | 33.45 | +0.32 | 0.42 | 1.35 | 0° | 0.56 × 0.32 m |

Pilot eye remains `[31.48,1.35,0]`. The overhead bank spans roughly ±48° azimuth,
not seven windows in one front row. The upper case lower edge is 1.77 m; lower
cases end at 1.51 m. Fully deployed turret views intentionally occupy the narrow
central mining sightline; a forward upper band remains below the overhead bank.
They retract for warp. The original 1.05 m target failed pilot ray tests behind
the fixed command case, so the deployed height was raised to 1.35 m. Actual front and head-turn captures confirm this arrangement; head turns are
required to inspect outer feeds without foreshortening.

Fixed cassettes have real front/back/side/end walls and an open deployment end.
Glass is hidden behind the front cover when stowed, not made invisible by UI.
Cases, glass, rim and extending stems are only in the dynamic recipe; there must
be no immutable ghost copy. Content plane is just forward of glass at local
X≈−0.044 m, inside the physical rim. Renderer, occluder and input use the exact
same sampled authority fractions, not separate clocks.

`cockpit_mechanism_box` includes the case and stem conservatively.
`cockpit_mechanism_sweep` unions endpoint extents with constant axes: valid for
this strictly vertical translation, not a generic rotation approximation.
Traversal reserves remaining travel, preventing a person from stepping into a
future moving screen after the initial clearance check. Obstruction retains the
target and reports blocked rather than teleporting through crew.

Three passing protocol checks cover 101 fractions, 525 deployed glass rays across seated, mid-rise and standing eye poses,
physical opaque covers at stow, disjoint full sweeps, moving cases
against all existing solid geometry and fixed cassettes, and sampled seated/rear
circulation capsules. Actual seated/standing scene views, all placements and
server authority transitions remain required before acceptance.

## Authoritative behavior and limitations

State belongs to the stable owned vessel, not a local UI session or only the
walking avatar. Five hardware assignments remain sticky; returning drones stay
assigned until actual removal, and a retiring slot retracts before reassignment.
The protocol permits up to fifteen drones across classes: additional IDs must be
reported as unassigned/overflow, not silently removed or represented as if five
screens were fifteen. Hardware slot, drone ID and binding generation are distinct
identities for interaction cancellation.

Both turret screens may deploy when either actual valid module cycle is active;
individual views report active/standby truthfully. There is no authoritative
weapon-shot event, so a cycle is not labelled proof of a shot.

Existing accepted warp immediately enters `InTransit` with a trajectory warmup;
this slice reacts to that state. It does not claim a new "wait for screen secure"
warp interlock, a cancel-warp request, or drone recall. Existing server behavior
invalidates drones on transit. Blocked stow must remain visible as blocked, even
in warp. Recovery follows accepted normal state, not merely an elapsed arrival
timestamp or a locally extrapolated successful landing. Editor pause freezes
physical authority motion; reconnect does not replay launches.

Mechanisms currently have no installed-mass or motor-energy ledger. Stems and
casings are geometry, not certified machinery or simulated power consumers.
The requested reactor housing/configuration motion is a separate subsequent
module; no core shrinking, whole-ship scaling or invented reactor telemetry is
part of these screen actuators.

## Camera budget and evidence

Seven dedicated feeds plus up to four unchanged manual camera views require at
most eleven distinct capture specifications. The implemented cap is eleven, longest
edge remains at most 384 pixels, and initially only one feed capture is granted
per presented frame. Seven simultaneously displayed images does not mean seven
new frames every presentation frame. Show per-feed capture age, source signal
and disconnected/stale states. Capture keys include source/mode/aspect and exact
fixed capture dimensions; physical travel must not churn capture size.

At eleven square 384px color+depth targets, raw attachment storage is approximately
13 MiB before driver overhead; rectangular actual captures are smaller. This is
an estimate, not a measured frame-rate guarantee. Acceptance must record actual
headless frame times, each feed's captured timestamp/dimensions, fairness and
single-grant behavior. Software EGL evidence is not consumer-GPU certification.

Required proof combines real server request tests (launch, module cycles, warp,
rejection, restart, obstruction) with actual App/EGL stowed/partial/deployed and
warp views. Snapshot fixtures alone demonstrate rendering, not live authority.

## Verification checkpoint

- Protocol: 65 passed, including the three mechanism geometry/ray tests.
  Log: `/tmp/pioneer-mechanism-protocol-final.log`.
- Server: 91 passed, 4 existing ignored in the final combined suite, including
  real-request seven-screen mining/warp, obstruction, restart, inactive-vessel,
  paused-editor clock and disconnect/reconnect tests.
  Log: `/tmp/pioneer-mechanism-server-final-v3.log`.
- Verifier: 13 library tests and 1 CLI checkpoint test passed after additive
  optional-state fixture updates. Log: `/tmp/pioneer-mechanism-verifier.log`.
- Client: 489 passed, 11 inherited baseline failures, 30 ignored. The failure-name
  set exactly matches clean `8da5c35`; log `arkline-physical-client-full-v4-final.log`.
- Final actual App/EGL capture passed in 63.22 s, including all seven seated
  feeds, eleven with manual feeds, intermediate and accepted-warp poses, and
  standing forward/port/starboard views. Receipt:
  `arkline-physical-cockpit-proof-20261023-v6-final/receipt.json`.

Protocol wire version remains v1 with a serde-defaulted optional mechanism
field. Geometry registration 6 is the new capability/admission boundary. No
migration runs. Explicit refusal coverage applies to prior registered bindings,
not a world-wide version field that does not currently exist.

### First render diagnostic — not accepted as warp proof

`arkline-physical-cockpit-proof-20261023-v1` rendered seven unique warm camera
textures, eleven with manual views, and asserted at most one capture per frame.
The real casings deploy/retract and glass contains actual local scene views.
However, review found that its synthetic warp pose retained normal mining/drone
state and its freshness clocks disagreed with the App footer. The App status
panel also obscured two upper views. This set is an internal geometry/texture
diagnostic, **not evidence of coherent accepted warp mode or a live session**.
The next proof must consume snapshots from the actual server request journey,
retain truthful freshness and provide supported HUD-off head-turn views plus a
normal App capture. No screenshot editing or label suppression substitutes for
correct state.

### Real authority input for the corrected proof

`arkline-screen-authority-proof-20261023-v2` contains unmodified snapshots from a
fresh in-memory server request journey: undock, warp to belt, lock a real
asteroid, launch and assign five mining drones, start both mining modules, warp
to station and arrive. Eight milestone snapshots plus 24 actual 40 ms advancing
snapshots supply coherent state and capture timing. Warp snapshots correctly
report zero deployed drones and stopped module targets under existing semantics;
arrival does not fabricate new deployments. The optional test export is enabled
only by `ARKLINE_COCKPIT_AUTHORITY_PROOF_DIR`; no user save is read or changed.

The second render set uses those actual authority inputs, real elapsed capture
ages and the real collapsible status header. Review confirms coherent warp
warmup/stowing, stopped modules and no deployed drones. It also exposes a
previously concealed camera-rig issue: turret POVs intersect or look across
own-ship geometry in the genuine mining pose. The feeds are real but obstructed;
texture-count success alone is not useful-view acceptance. The correction must
keep a plausible near-barrel camera and real occluders, distinguishing camera
self-intersection from a genuinely obstructed line of fire. Hiding ship geometry
or fabricating a remote clear viewpoint is not an acceptable fix.

The actual obstructed fixture targets asteroid 15 aft/below the ship. That is a
real line-of-sight limitation, not permission to relocate the camera to a distant
vantage. Keep this diagnostic and label observed own-hull occlusion; demonstrate
clear views with a second legitimate forward-target request journey. Existing
mining authority does not enforce a complete own-hull firing arc, so this screen
slice must not claim physically complete weapon/mining collision or change
resource economics silently.

Manual-flight `LeaveSeat` also has an existing authority interlock requiring an
active owned Pioneer routine. The renderer's on-foot proof must use a real
permitted routine journey or remain explicitly unverified, never a fabricated
standing avatar or disabled interlock.


The forward target33 export (`arkline-screen-authority-proof-20261023-v5-forward-target`)
produces useful distinct turret views with the unchanged near-muzzle camera rig.
The corresponding App `arkline-physical-cockpit-proof-20261023-v3-forward` images
were inspected front/port/warp; own-ship central ray checks report clear for both
camera views. This complements rather than replaces the obstructed target15 case.
The normal status header now genuinely collapses/reopens while retaining current
connection/control state, session access and the persistent authority footer.

A separate actual autonomous `StartResourceJob` journey successfully deployed
five mining drones and both modules before accepting `LeaveSeat`. Unmodified
seated/standing snapshots and full Responses are in
`arkline-screen-authority-routine-20261023-v2`; renderer proof remains pending.
A full client run caught three new reopen regressions from unnecessarily resetting
stationary mechanism timestamps. The server now changes restart state only for
actual motion, with exact serialized stationary/blocked-state preservation tests;
the subsequent client regression rerun below verifies the fix without weakening those tests.


Client regression rerun now reports **489 passed, 11 inherited failures and 30
ignored**. Its failure-name set exactly matches the separately compiled clean
base `8da5c35` (473 passed, 11 failed, 27 ignored); there are no additional
failures. All three newly exposed embedded recovery cases pass after the real
restart-state fix. Logs: `arkline-physical-client-full-v3.log` and
`arkline-baseline-8da5c35-client-tests.log` in the scratch root. This is not a claim
that the repository's entire test suite is green.


### Actual motion export for development preview

`arkline-physical-cockpit-proof-20261023-v4-final` contains 26 actual 1280×720
App-rendered motion frames: `motion-deploy-00..12.png` followed by
`motion-warp-00..12.png`. Each phase consumes the identically named unmodified
server snapshot from the forward-target request journey. Authority samples are
100 ms apart: deployment 22511–23711 ms; warp retraction 24911–26111 ms. There is
no interpolated or generated image motion. `motion-source-manifest.json` records
source/frame hashes, exact timestamps and dimensions. GIF/MP4 packaging may hold
actual frames for readability; this is not a desktop frame-rate benchmark.

Those motion frames completed before the fixture's independent standing-view
assertion failed (five scheduled feeds rather than seven). The seated routine
capture completed; the standing renderer remains under diagnosis. Therefore the
motion export is usable visual evidence, not a claim that the entire capture
fixture or standing acceptance passed.


### Completed standing and capture acceptance

The standing diagnostic was a view-frustum assumption, not missing turret state:
standing forward shows the central three drone bays and both turret bays; the
outer two drone bays require natural head turns. The final fixture preserves the
canonical eye/FOV, retains the exact-seven seated assertion, and verifies all
seven distinct physical bays with their own warm image across actual standing
forward/port/starboard views. All three views were inspected. The v6 complete
receipt supersedes the v4 incomplete overall run; the earlier motion images
remain genuine. Standing interaction overlays still overlap near the lower
center and are not claimed presentation-polished.

Software renderer: llvmpipe LLVM 19.1.7. Headless median frame wall times for
0/1/7/11 sources were 123.3/124.4/143.8/144.8 ms. The seven- and eleven-source
sequences each captured twelve images over twelve presented frames, never more
than one per frame. Actual image ages reached 3258/4251 ms in those respective
capture workflows; visible timestamps are intentional. This is not seven or
eleven full-rate video streams, nor a consumer desktop performance measurement.
The eleven actual color+depth specs total a nominal 3,788,160 bytes at eight
bytes/pixel, before driver/cache overhead; the earlier square-target upper
estimate is not a measured allocation.

### Build/lint boundaries and known CI status

Native strict workspace Clippy still fails on three pre-existing diagnostics:
`pioneer_docked.rs`'s two dial-index test loops and `docked_traversal.rs`'s
nonminimal routine-state boolean. Those exact constructs exist in clean
`8da5c35`; this is not a green-Clippy claim.

The matching official wasm32 standard library and locked browser dependencies
were installed in the existing development toolchain. Actual browser Clippy
reached the client. Four diagnostics attributable to the changed interface were
corrected without changing render behavior: test-only metadata/wrapper gating,
an explicitly justified existing multi-argument adapter, and equivalent optional
scheduler predicate. Final strict browser Clippy reports 16 inherited errors;
every diagnostic message occurs in the clean-base diagnostic warning run. The
base strict run itself stops earlier on two protocol approximate-constant errors;
a separate command-line warning diagnostic allowed only those constants to
collect client warnings, without editing source or claiming a strict pass.
Logs: `/tmp/pioneer-mechanism-browser-clippy-final.log`,
`/tmp/pioneer-baseline-browser-clippy.log`, and
`/tmp/pioneer-baseline-browser-warnings.log`.

`cargo fmt --all --check` still reports legacy formatting in 23 files, versus
25 files in clean `8da5c35`; the two new formatting regressions were fixed.
No newly named failing file remains. This file-name comparison does not claim
that every pre-existing formatting hunk is identical.

The previously published spatial checkpoint `49c17e9` has a failed exact-head CI
run `39906002302` (Linux tests, browser Clippy, macOS formatting). Local checks
of this mechanism change do not retroactively make that run green. Publication,
new-head CI, merge and release remain separate outcomes.

Final lint-tidied tree: workspace `cargo check --all-targets --offline --locked`
passed (`/tmp/pioneer-mechanism-check-final.log`). The final native client rerun
remains 489 passed / 11 inherited failures / 30 ignored
(`arkline-physical-client-full-v5-lint-tidy.log`). The final-code obstructed-target
App proof also passed in 60.45 s; `arkline-physical-cockpit-proof-20261023-v7-obstructed`
retains real own-hull pictures labelled CURRENT RIG OBSTRUCTED, explicitly a
current pose diagnostic alongside independently aged cached images. These lint
changes only gate test metadata and use an equivalent optional predicate; they
do not alter the captured physical layout or animation.

## Local registration-7 recovery continuation

A reconstructed reactor configuration module and independent permanent display
are being validated locally on top of this published registration-6 checkpoint.
See [reactor configuration](reactor-configuration-20261023.md). The original local
reactor commit was lost; this is a newly validated replacement, not recovered
historical test evidence. Existing registration-6 worlds are not migrated.
