# Integrated Pioneer cockpit and captain monitor controls — revision 9

## Local workstream and provenance

Branch: `design/pioneer-captain-controls-20261023`.
This local-only continuation combines two separately validated checkpoints:

- Reactor reconstruction `a154a3ca437cc2c139613b7053d009d012908d18`, revision 7.
- Ergonomic wings, fixed drone metric strips and independent camera callback fix
  `4819300073fad27ba6e19ac4042440a35b5b92bb`, revision 8.
- Shared published base `f2f3bb7071447ca7fd063728ec7b0ba1fd4fb068`.

The originals remain preserved in their own clean checkouts. Explicit conflict
resolution retains reactor display 15, ergonomic permanent faces 6/7, revised
physical turret travel, and five fixed cassette strips 16–20. The combined
geometry uses new registration 9 with fresh disposable matching-build worlds.
No old saved world is migrated. Binding checks are not a universal protocol
admission gate for never-traversed worlds or mixed client/server builds.

Public runtime source publication is paused pending specific approval. This is
local implementation and validation, not a GitHub publication, CI or release.
The existing issue 62 is historical scope context; no denied progress post is
retried. New captain controls are authorized project work, not an expansion into
reactor physics, external hull articulation or docking state machines.

Ownership: coordinator handles integration/shared geometry/docs; authority worker
handles protocol request/state plus server controller/persistence/permission tests;
runtime worker handles client controls, display/status and actual engine proof.

## Captain intent versus equipment activity

The seven moving camera monitors each have a persisted preference: AUTO, OPEN or
STOWED. An individual operation addresses stable hardware identity; all-monitor
operations validate every current drone binding before applying atomically.
The active Pioneer owner, locally aboard their own vessel, is the bounded captain
permission model. This does not invent delegated crew-role authorization.

Monitor requests must never stop mining, recall drones, clear targets, change
module cycles or cancel a running resource job. These are physical display
presentation commands, explicitly separate from equipment commands. Permanent
navigation, reactor overview and cassette metric strips stay available.

AUTO follows existing confirmed equipment activity. OPEN requests an extended
monitor even when no camera is assigned, which must honestly show unavailable.
STOWED hides the physical monitor without changing underlying equipment. Warp
forces safe physical stow regardless of preference and preserves the preference
for subsequent normal state. A retired drone binding still retracts before being
reused; manual OPEN must not indefinitely pin a vanished identity. Generation
checks prevent delayed input controlling a replacement drone assignment.

Actual pose and requested mode are distinct: moving, blocked, retiring and warp
priority must not be mislabeled complete. Existing collision sweeps, shared
mesh/glass/depth/input pose, pause/restart holds and stable assignments remain.
Existing warp semantics invalidate the deployed drone fleet; this is not a
new monitor side effect or a successful drone-recall claim. On arrival, manual
OPEN may therefore show unavailable sources rather than resurrecting old drones.
There is no new warp-cancel API or secure-before-warp interlock. Motor energy,
equipment mass and reactor thermal/fuel simulation remain unmodeled.

## Version and verification plan

The optional existing mechanism field receives serde-default AUTO preferences;
older serialized payloads must still parse with explicit test coverage. The new
request requires matched development builds; registration 9 guards geometry
bindings, not generic wire feature negotiation.

Acceptance:
1. Combined protocol geometry and all existing reactor/ergonomic fit tests.
2. Actual server individual/all AUTO/OPEN/STOWED requests; mining/drone/module,
   target, energy/economy and active automation semantics unchanged at equal
   authority time. Wrong owner/vessel/stale assignment/out-of-range target reject
   atomically. Manual control may not enter generic equipment-cancellation flow.
3. Accepted/rejected warp priority; hidden equipment continues; reopen reads
   current assignment/activity; vanished identity retires safely; obstruction,
   reversal, pause/reconnect and restart retain truthful preference and pose.
4. Real client pointer/keyboard focus tests emit only monitor commands, with
   readable requested-mode versus actual-state labels and accessible reopening.
5. Fresh actual-request engine proof: deployed, manually stowing/stowed, equipment
   still active on fixed metrics, reopening/current feeds and warp override.
   Retain reactor overview and ergonomic side views. No screenshot editing or
   generated image substituted for actual rendering.
6. Full applicable protocol/server/client/verifier checks and native workspace
   compilation; report retained baseline failures separately and preserve a
   private source/validation checkpoint. No public upload without authorization.

## Initial integration result

Both source checkpoint refs were present and clean. Conflicts were resolved
explicitly; combined protocol suite passed 72 tests before new mode code. This
establishes integrated geometry only, not completion of captain controls.

## Authority implementation and initial validation

`SetCockpitScreenMode` is a monitor-only request, deliberately excluded from
both normal manual-command automation cancellation and delegated/automated
execution. Each drone-slot request supplies its current assignment generation;
the all-monitor operation supplies all five generations and validates before any
write. Preferences and retiring-slot status are serde-default additions to the
vessel-owned mechanism state. Actual preference changes timestamp the state
without restarting unchanged physical travel; repeated same-mode requests do
not reset it.

Five focused tests passed, then the full server library suite passed **106 tests,
0 failures, 4 ignored**. Coverage includes a real active resource job with five
drones and two mining modules, individual and all-monitor operations, equal-time
equipment/job continuity, actual motion, accepted and rejected warp, atomic stale
or foreign requests, owning on-board permission, retiring IDs under manual OPEN,
obstruction, paused motion and SQLite restart. Registration 4/5/6/7/8 bindings
are refused without migration in the integrated revision-9 build.

A new real-request fixture exports the manual sequence for engine proof; it is
not a live user session. Client controls, full client/protocol/verifier validation
and new rendered evidence are pending at this stage.

## Next requested drone telemetry slice (not implemented here)

The user requested mini-ship metrics and progress bars. Current authoritative
`DroneView` supports identity/class, HP, signal, position/velocity, heading vector,
task mode, one mining-cycle start/end pair and an attack cooldown deadline. It
has no per-drone turret/module list, battery, fuel, thrust force/throttle, cargo
or full attitude/roll. Existing drone mining/attack debits parent-ship power.
A bounded follow-on can therefore show the confirmed single-cycle bar, task and
returning state, HP, speed and heading yaw/pitch, with unsupported power/thrust
explicitly unmodeled. It must not manufacture per-turret cycles or roll.

The audit also found that `DroneView.updated_at_ms` measures motion integration,
not a universal heartbeat: HOLD does not update it. Telemetry availability must
use snapshot/session freshness, with pose age separately labeled, so stationary
healthy drones do not falsely become stale. Cycle progress follows confirmed
simulation time and freezes on pause/disconnection; task MINING without an
active cycle means waiting/approach, not confirmed extraction. This is recorded
for a separate follow-on after the captain checkpoint, not mixed into this slice.

## Integrated client controls and validation

The fixed central computer's Displays page now uses large ALL mode actions, a
seven-bay selector, and large actions for the selected bay with confirmed source
and hardware pose. The expanded Ship → Displays panel retains the full seven-row
view. Fixed navigation/reactor/cassette surfaces remain available when all seven
camera monitors are stowed. A held pointer cannot follow selection, assignment,
session or permission changes onto a different command target. The existing
Ship interaction window is genuinely collapsible/reopenable, with default-open
behavior retained and placement clear of the authority footer and on-foot HUD.
On-foot controls use the expanded owner-authorized panel, not the passive glass.

Final non-GPU validation: protocol **74 passed**; server **106 passed, 4 ignored**;
client **507 passed, 11 retained baseline failures, 32 ignored**; verifier
**13 library + 1 CLI integration passed**; native workspace all-targets compilation
passed. The five focused captain client tests pass, including both tested aspect
ratios, all seven selection targets and held-press identity guards. The full
client suite is not green: its failure names match the previous eleven baseline
failures; none are suppressed or removed. Browser/wasm validation was not run
because the target is absent. No remote CI or public runtime publication is
claimed.

The V5 actual EGL/App proof passed, including real pointer collapse/reopen,
footer-safe placement, no leaked ship command, authorized on-foot policy request
and all seven refreshed camera images after reopening. A final capture repeats
this with the ordinary pointer off controls so hover tooltips do not conceal the
selector; this is a capture-input correction, not screenshot editing. Real
server-request fixtures preserve active mining/job/drone state through manual
monitor commands. They are disposable development evidence, not a live user
session, real-time performance benchmark or assertion that baseline failures
are fixed.

Final V6 proof passed in 42.26 seconds with neutral-pointer capture and real
expanded-panel scroll reaching T2. Coordinator inspected active/deployed,
manually stowed-but-mining, owner-on-foot lower controls, accepted warp override,
permanent reactor display and all-seven refreshed images. Known genuine turret
view obstruction remains visibly labeled; it is not hidden or repaired by a
manufactured camera vantage. Evidence: `arkline-captain-client-proof-v6`, capture
receipt and `/tmp/captain-client-gpu-v6.log`. Runtime source is locally checkpointed
only; public upload remains pending authorization.
