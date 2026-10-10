# Private model validation

This document records evidence for the initial Rust model, not approval to publish.

## Automated local checks

On the development Apple Silicon Mac with Rust 1.89.0:

- Workspace compilation, formatting and Clippy with warnings denied.
- Automated coverage includes: private client credential persistence/resume, shared trajectory math,
  stable pilot placement, geometry normals, beam endpoints and asteroid impacts;
  confirmed extraction cues, expiration and purchase/arrival exclusion; GUI audit checkpoint persistence,
  rejection and concurrent-cockpit locking; server authorization,
  failed-operation rollback, concurrent fill, lifetime DB ownership, in-memory
  isolation, persisted travel/cooldown and unchanged-poll write suppression; TCP two-pilot journey; nine verifier rule
  and tamper checks; checkpoint CLI persistence on failure.
- The TCP journey checks mining and a partial trade, abruptly kills the process,
  resumes both saved identities after restart, checks balances/escrow, cancels the
  remainder, and verifies the exported history with the independent verifier.

This is not a stress test, distributed failure test, or simulated interruption
during an active SQLite commit. CI logs supply separate hosted-run evidence.

The first preview passed hosted macOS and Linux CI. Native macOS inspection of the
3D update confirmed GPU-rendered hulls, station, asteroids and the detailed planet;
the 2D toggle preserved pilot location, balance, cargo and ledger head. Restart
resumed the test pilot at the saved belt. Cmd+Q was checked to terminate both the
hosting cockpit and its child server. Runtime inspection caught and resolved a
GLSL built-in name collision that Rust compilation alone could not detect.

## Owner validation journey

Open the preview, create a pilot, open a second cockpit with another name, and
complete the flight/mining/trading steps in README.md. Check whether the interface
and shared world match the intended Arkline direction. Close/reopen the host and
resume the same identities. The ledger verifier can connect to the local address
shown in the cockpit.

Owner acceptance is pending. Neither a build nor an automated journey constitutes
that acceptance. Keep repository visibility private; selecting a license and
publishing require explicit owner approval.

## Cockpit services preview

Native macOS inspection in an isolated world confirmed the sector overview,
selected-belt warp action, mining with cargo/ledger updates, and independent audit
from the Ledger tab. Repeating the audit matched the locally pinned history.
The expanded shell keeps the real GPU viewport between pilot status and object
information; Cargo, Exchange, Activity and Ledger share that shell. Known pilots
are explicitly labelled as saved identities rather than live presence.

Mutating commands are gated while awaiting a response. This prevents accidental
queueing from repeated clicks; it does not replace server-side command IDs or
resolve an interrupted request whose result is unknown. The existing two-pilot TCP
journey continues to cover partial trades, restart and independent replay.

## Animated feedback preview

Mining effects are derived from confirmed increases in the current pilot's belt
cargo. Purchases at the station and arrival snapshots do not trigger extraction.
Repeated snapshots do not replay effects; effects expire and clear on reconnect or
connection loss. Tests cover these transitions and verify 3D beam geometry and
asteroid impact points across all pilot lanes.

The animation QA app launched, but macOS ScreenCaptureKit returned capture error
-3811 during inspection. Visual sign-off of this animation update remains pending;
the earlier cockpit inspection above does not establish animation appearance.

## Dated build and persistent ships

Build 26.10.9.1 adds shared header/window/package identity, an original application
icon, Emerald/Teal/Amber preferences, viewport object selection and a six-second
warp sequence with two seconds of warm-up. Native inspection confirmed the dated
header/title, black-and-green cockpit, operation phase labels and hidden docked
ship. A live extraction updated cargo and showed MINING LASER ACTIVE; the capture
of that transient was a background-window thumbnail, so detailed particle
appearance is not established by that capture alone.

The new transport test authenticates two sockets for one pilot, verifies that
presence survives the first disconnect but disappears after the last, then checks
that the undocked ship and economic history survive disconnect and restart.
Unresponsive networks still use the read timeout, not immediate failure detection.
Theme tests cover saved choices and invalid-file fallback. Ship computers and
distributed authority are not implemented.

## Authoritative flight — 26.10.9.2

The server now integrates position, velocity and heading at 10 Hz. Automated
checks cover acceleration, bounds, braking, orbit settling, moving-target range
tracking, warp warm-up and arrival, separated berths, and legacy save migration.
An actual TCP client moves, stops, restarts the server and recovers the exact
stopped position without changing cargo, credits or economic ledger history.
Client tests cover tactical coordinate mapping, 3D destination projection,
extrapolation limits and physical mining eligibility after manual movement.

Native macOS inspection confirmed build identity, orbit speed/range feedback,
Ctrl+Space braking to zero, and double-click flight acceptance with acceleration
and changing distance in the 3D cockpit. This does not establish collision or
combat behavior: neither is implemented. Warp cannot yet be cancelled.

## Object targeting and connected systems — 26.10.9.3

The runtime TCP acceptance journey mined asteroid ID 1, checked that only its
reserve decreased, travelled Junction → Ember Reach → Glassway → Ember Reach →
Junction, restarted, and confirmed saved system, cargo, credits and ore depletion.
Gate travel left the economic head unchanged; the independent verifier accepted
the final export. Server tests additionally restart during a pending jump and
reject cross-system targets, unlocked mining and exhausted rocks. Migration
allocates only the old remaining reserve and preserves existing economic history.

The independent verifier also rejects mismatched summed asteroid reserves and
duplicate asteroid IDs. It cannot prove historical per-rock allocation because
legacy economic events contain pilot and quantity, not asteroid ID.

Native macOS inspection confirmed the gate charge state, arrival in Ember Reach
with a different local object list, selecting and locking asteroid #37, and its
reserve decreasing from 926 to 901 after extraction. Scene captures showed the
new gate geometry, individual asteroid field and expanded viewport. Capture of
the transient beam failed with ScreenCaptureKit error -3811, so beam appearance
was not visually signed off by that attempt. Geometry tests verify its exact
asteroid endpoint. The final overview defaults to nearest-first and can reverse
the range sort.

## Full-window cockpit — 26.10.9.4

Native captures confirm that both tactical and flight scenes reach the gameplay
window edges, with the cockpit drawn on floating translucent panels. Cargo opens
in an optional floating Services window. Opening and closing Cargo and switching
to tactical view preserved the exact saved flight state, cargo and credits in an
isolated world. An egui frame test verifies that the CentralPanel spans the screen
and foreground panels block scene pointer input while uncovered space accepts it.
Initial native inspection caught an implicit default-height issue that placed the
ship HUD too high; ship and camera windows now specify compact initial sizes.

Fullscreen is available through F11 or the session overlay. This layout update does
not change the existing physical scale or fixed warp timing.

## Continuous flight and cockpit — 26.10.9.5

The workspace passes 54 tests, formatting and strict Clippy. New checks cover
analytic warp continuity and bounded acceleration, inter-snapshot normal flight,
AU-relative meter precision, migration and exact trajectory recovery after restart.
A separate TCP acceptance run traveled Junction → Ember Reach → Glassway → Ember
Reach → Junction, mined only asteroid 1, retained 25 ore and 1,000 credits, restarted
and passed the independent economic audit.

Native inspection switched Tactical local/system, External and Cockpit views.
Cockpit captures show outside asteroids, readable real speed/cargo/target telemetry,
and forward streaks during a gate warp at 0.506 AU/s. Native QA caught and fixed
bright screen faces, a readout covered by ship controls and a camera-relative planet.
The planet is now world-anchored. Presentation requests a frame every 16 ms, but no
frame-time benchmark or remote-network latency acceptance has been performed.
The cockpit and world assets remain procedural prototype geometry; this is not
final production art or a direct-steering flight simulator.

## Visible warp travel — 26.10.9.6

58 workspace tests pass, including local departure/arrival sampling, velocity
continuity, legacy-path compatibility, alignment and camera rebasing checks.
The real TCP three-system journey passed with the longer travel deadlines.
Native cockpit captures show departure from station and the asteroid field
approaching at 10.9 km, followed by 1.4 km at 1.1 km/s during terminal arrival.
External view shows the ship inside the direction-aligned tunnel. Visual QA caught
a legacy oversized launch hoop; it was removed from launch/arrival and replaced
by a dim, thin warmup ring. Alignment is still a fixed-duration preview rule.

## Direct six-axis cockpit flight — 26.10.10.1

73 workspace tests, formatting and strict Clippy pass. Direct flight checks cover
all positive/negative body translation axes, pitch/yaw/roll, normalized diagonal
input, input expiry, release braking, restart recovery and autopilot handoff.
Client checks cover focus/typing suppression, capture/release commands, latest-input
delivery and presentation continuity when manual input expires.

An independent live TCP run verified forward, strafe and vertical thrust, combined
rotation, normalized orientation, stale-input expiry and orientation recovery after
restart without economic changes. Native inspection confirmed the cockpit-only
interface, direct-flight status and persisted translated/rotated ship state. Native
mouse capture feel has not been fully validated; the client includes a visible
bounded-aim fallback when capture is unavailable. This remains assisted flight:
releasing translation brakes the ship, while the current attitude is retained.

## Meter-scale fleet and station — 26.10.10.2

78 workspace tests, formatting and strict Clippy pass. Mesh bounds match all four
published hull dimensions, rendering and picking share parts, and the station is
bounded to 20 km across and 6 km high with clear berth points. A headless native UI
interaction verifies docked boarding sends the selected hull request, waits for
server confirmation, and cannot change hulls in space. Server tests cover persistence,
legacy defaults and unchanged economic history.

A separate live TCP run selected all four hulls, checked cross-client replication,
rejected selection after undocking, and resumed all classes after restart with the
same ledger hash. Native visual inspection used an isolated staged scene with three
other hulls roughly 5 km away and the station docking port 22.3 km away. It confirmed
clear differences in hull sizes and a substantially larger station behind them.
The staged scene validates visual proportions; it is not evidence of fleet AI.
Cockpit interior geometry is human scale. Hull physics tuning remains shared and
collision response is not implemented.

## Camera modes and timed mining targets — 26.10.10.3

The workspace suite passed 83 tests; a subsequent turret-support regression adds
one client test, and all 41 client tests pass after that geometry fix (84 total).
Formatting and strict Clippy pass. Tests cover mode switches preserving navigation
and selection, disabled extraction during acquisition, finite turret slew, barrel
muzzle position, pedestal attachment, exact lock deadlines, retarget/unlock, range
loss, warp, logout and restart. Both mining APIs reject unconfirmed targets.

A separate live TCP and command-client journey verified rejected early mining,
cancellation, retargeting, successful extraction after confirmation, and unlock.
The command client's lock operation waited 5.18 seconds before reporting success.
Native inspection switched Cockpit to External and showed a 2.7-second acquisition
countdown with both extraction controls disabled. Visual inspection caught a floating
turret mount; a deck-attached pedestal was added and covered by geometry tests.
Native inspection became unavailable before the final extraction capture, so the
complete rendered beam sequence still needs hands-on acceptance. This release adds
one mining module; combat damage and general module fitting are not implemented.

## Persistent station assets — 26.10.10.4

The station overlay opens on docking and exposes owned ships, personal inventory,
corporation membership/storage, and the existing exchange. The private preview
issues one hull of each class once. Stored hulls retain their own cargo and station.
The UI permits remote inspection but only local docked boarding and ore transfers.

Automated coverage includes 44 client tests, durable-data validation, legacy-save
migration, station isolation, corporation invitations and authorization, failed
transfer rollback, and independent replay of all storage buckets. A separate live
TCP journey mines 100 ore, returns and docks, deposits personal ore, changes hull,
creates/invites/joins a corporation, rejects unauthorized withdrawals, restarts the
server, reboards the original hull and passes economic audits before and after.
An isolated copy of a previous preview world also passes migration with unchanged
active cargo, credits, hull selection, system, and ledger history.

Native package validation confirms automatic station landing, boarding a stored
Prospector, corporation creation showing the authenticated owner, and a native
25-ore deposit updating cargo from 100 to 75 and station storage from 0 to 25. These are
functional service overlays; a rendered hangar interior, fitting, generic items,
production, shared hulls and advanced corporation administration are not included.
See station-assets-audit.md for the public ledger and aggregate snapshot limits.

## Anchored overlays and continuous mining — 26.10.10.5

Panels retain edge-relative placement across viewport changes, with local layout
persistence and settings for snapping, pinning, and restoring defaults. Native QA
confirmed the overview remains at the right and ship controls at the bottom when
switching between windowed and fullscreen. Layout tests also cover saved positions,
reflow, and reset behavior.

Mining uses two server-timed modules. Activating a module requires a confirmed,
in-range asteroid lock. Completed cycles award the hull-defined quantity and start
the next cycle automatically. Tests cover repeat cycles, two simultaneous modules,
invalid state, stopping without partial awards, late ticks, failed commands at a
cycle deadline, restart, migration, depletion, full cargo, and independent economic
conservation. Existing trade and travel acceptance now use completed fixed cycles
rather than the retired player-selected quantity commands.

In native cockpit QA a Prospector began with 75 ore and reached 835 through repeated
40-ore cycles without further activation clicks. Both turrets showed active progress,
physical beams, and cockpit instrument countdowns. Explicit stop halted both modules;
the independent verifier accepted the resulting world and ledger.

A five-second pre-change process sample showed substantial time in presentation
synchronization and repeated render work. The renderer now caches uniform handles,
geometry and station batches (161 station draws reduced to six), while focused
rendering requests the next frame without an extra 16-ms delay. Unfocused rendering
is throttled. The header measures actual frame intervals. Native focused station
and mining scenes measured roughly 116-120 FPS (8.3-8.6 ms) on this Mac; this is not a
cross-hardware guarantee or a measured before/after FPS ratio. The separate ignored
CPU preparation benchmark is synthetic and must not be reported as an FPS result.


## Ship-centered 3D tactical overlay — 26.10.10.6

Adds a separate wide-angle 3D Tactical mode alongside the 2D map, external camera,
and cockpit. Range rings use the same camera matrix and ship-relative meter scale
as scene objects. Default rings are spaced every 5 km; zoom changes spacing to 1 or
25 km with a bounded maximum of ten rings and 128 segments per ring. The selected
object guide shows horizontal projection, signed height and full center distance.
The overlay can be hidden, and tactical zoom/orbit is retained across view changes.

Client tests pass (55 passed, one ignored profiling test), including ring geometry,
ship-center projection at astronomical origins, maximum-zoom click-to-fly rays,
and pointer-based switching among
all four views without navigation side effects. Strict workspace Clippy passes.
Native visual appearance and frame rate for this new mode have not been validated:
the development Mac was locked during this run. The prior build's measured FPS does
not establish this mode's performance. Detached tactical panning is not implemented.


## Instrument alignment and transport recovery — 26.10.10.8

The previous preview showed raw macOS os error35 and remained offline after a read
timeout. Its local server was still listening; native Session → Reconnect restored
the saved pilot with 225 ore and 1000 credits. The original timeout cause is unconfirmed.
A later upgrade of the same saved world retained the owner's newer 1000 ore progress.

Instrument text and bezels now share geometry. Display rectangles are derived from
the actual face, clipped, and scaled with viewport dimensions. Tests compare those
rectangles with the 3D projection and reject overlapping screens at multiple sizes.
Telemetry tests check actual body-axis velocities, cargo, balance, range, lock state,
and stale-data marking. Initial native 26.10.10.7 instrument QA showed aligned text
and 119–120 FPS; a final adjustment lowers the dashboard and moves side displays inward
to avoid physical occlusion. That final geometry is packaged in 26.10.10.8 separately
so the owner's active flight need not be interrupted.

Five TCP transport tests cover fragmented responses, total deadlines under trickle
traffic, actual response timeout recovery, saved-identity resume, and no replay of an
uncertain Buy, queued Deposit, or stale flight control. Reconnecting freezes the last
known scene and disables operations; Ready resets presentation history before new
state is applied. Three automatic attempts are allowed per worker session; auth
rejection stops recovery. Full sleep/wake recovery has not been tested.


## Five asteroid locks and recon drone — 26.10.10.9

Five pending/confirmed asteroid locks share a server-enforced cap. Each mining slot
uses its own confirmed target, and individual unlock only stops affected modules.
Legacy scalar lock fields are compatibility mirrors; old saved locks migrate into
the new vectors. Independent acceptance tests verify acquisition limits, two-rock
extraction, selective unlock and economic replay.

A singleton recon drone has authoritative movement, hold and physical recall. The
client offers launch/recall/hold/fly-selected controls and 100 m asteroid stand-off.
Tests reject invalid/out-of-range/unauthenticated commands and verify movement,
recall, migration, lifecycle cleanup and unchanged economics. Automatic safety
recovery on warp/dock/disconnect/restart/range loss is instantaneous prototype policy.

The left physical cockpit display can render the world from the drone's position
and heading using a separate clipped GPU viewport. Camera-matrix tests verify that
moving the drone changes projection, vertical headings remain finite, and the feed
stays within the display. No recording or decorative substitute is used.

The full workspace suite and strict Clippy passed; client tests were 67 passed with
one profiling test ignored. In an isolated packaged local world, native launch
reported a drone 148 m from the ship. TCP acceptance then confirmed five locks,
modules 0/1 assigned to asteroids 20/28, drone travel to the surface stand-off, and
physical recall. Repeated cycles produced 900 ore; the packaged independent verifier
accepted the ledger (1000 credits, 99100 remaining belt ore). Native screen capture then
failed in macOS ScreenCaptureKit despite a fresh inspection session, preventing final
visual/FPS validation of the camera feed. The owner's live save was not used for this
acceptance run. No drone combat/mining payload or fleet management is implemented.

## Fleet and physical cockpit console — 26.10.10.11

This build adds five drones per class (recon, mining, attack), owned per-drone
commands, signal strength, autonomous return after link loss, repeated mining
extraction, and drone-only combat. Independent acceptance covers class/total caps,
cross-pilot command rejection, two mining cycles with exact depletion and ledger
verification, assigned-target damage/destruction, an untouched bystander, and
physical return. Lifecycle tests cover restart and legacy singleton migration;
formation tests cover all hulls, steep pitch and free-slot reuse.

The cockpit has six shared-geometry screen faces, a 70-degree vertical field of
view, a real service console, selectable local 2D and body-relative 3D maps, and a
selected-drone POV/orbit feed. Projection tests cover all six screens at multiple
window sizes, astronomical offsets, orbit target framing, vertical camera headings
and orientation-relative map scale. Full-app pointer testing clicks Fleet and
Launch through the physical console and checks that clicking while holding thrust
does not issue flight input. Noise uses the authoritative distance-based link and
is bounded in screen space; one selected world-camera pass is rendered at a time.

Native inspection used an isolated FleetQA world and the intermediate 26.10.10.10
package. Undock, physical Fleet-tab selection and Launch Recon were clicked through
the console. The selected drone's actual onboard view was visibly rendered inside
the left screen; its readout reported 99 percent link and 148 m range. The focused
fullscreen client's displayed counter settled around 118–120 FPS in this small
single-drone scene. This is an indicative UI counter, not a frame-time benchmark.
The user took over the window before orbit-mode and weak-signal visual checks,
so those visual checks and final .11 native acceptance remain pending. The final
.11 removes the redundant Overview/Flight Controls cockpit windows, adds separated
launch formations and clears camera imagery on stale/missing links. The user's
active session was not replaced.

Scope limits: no drone manufacturing cost or cargo transport hold; no ship combat,
ship destruction or team/permission-based engagement rules. Warp, docking, jump,
last disconnect and restart still clear the fleet under preview recovery policy.
Fifteen simultaneous camera renders, network load performance and Linux native
visual acceptance are not claimed. See public-preview-readiness.md before changing
repository visibility.

## Relay startup — 26.10.10.12

Client tests cover pending, ready and failed startup completion; status labels never
claim an unconfirmed session or world. A full-app test holds thrust during startup
and verifies that no command or control is emitted. All 73 client tests pass (one
profiling test ignored), strict workspace Clippy and formatting pass, and the
release bundle is ad-hoc signed/verified. The animation uses bounded vector geometry
and opens shutters onto the actual cockpit once a pilot snapshot is available.

Native visual inspection of the package was attempted in an isolated BootQA
session. The desktop automation provider reported that the Mac was locked and
could not unlock it, so appearance, transition timing and native Retry clicks are
not claimed verified. The isolated QA process was stopped; existing sessions were
not replaced. FTL relay labels describe the existing connection, not distributed
relay infrastructure or an additional security guarantee.

## Cockpit actions and authoritative power — 26.10.10.13

UNDOCK/DOCK/STOP are pinned above the service tabs and scrollable content. The
physical console is larger and uses the current viewport rather than a cached
previous-frame rectangle. Six primary services plus More replace the crowded tab
row. A full-app pointer test checks the entire Undock button footprint at 900×650
across all eleven service selections, confirms each click sends Undock, and checks
offline clicks send nothing. Client tests also cover server power values, missing
readings, and low-power warp admission only recovering after a new snapshot.
75 client tests pass, with one profiling test ignored.

Independent power acceptance covers capped recharge; successful warp debit;
invalid/insufficient warp preserving charge; turret/drone mining charged only on
actual completion; low-power cycles producing no ore; valid attack shots charged
with damage; empty capacitor producing neither; and persisted charge recovery.
Unit tests additionally cover fractional polling independence, no refund from
previously accumulated full-capacitor idle time, extreme elapsed intervals, legacy
initialization, future timestamps and charge following each physical hull when
boarding. All server test targets passed, including TCP journeys; one historical
legacy-save fixture needed the new power field removed and passed after correction.
Strict workspace Clippy and formatting passed.

The packaged local server was exercised with an isolated PowerQA identity. It
reported 240000 J and 1200 W while docked; Undock returned station space, then accepted
warp reduced charge to 210000 J and entered in_transit. No user world was used.
Native inspection failed with macOS ScreenCaptureKit error -3811 before an app
binding could be established, so packaged button appearance/readability remains
unverified even though the full-app interaction tests pass.

Warp pays an upfront reservation, not repeated continuous charges during transit.
Reactors stay running while parked/offline, capped by capacity. Mining and attack
drones draw from their owner's capacitor as a prototype powered-link rule. No
ship combat guns, heat damage, engine fuel or configurable power buses are claimed.

## Mining fleet, living world and surface construction — 26.10.10.14

The workspace test suite and strict all-target Clippy pass. Client coverage includes
79 passing tests (one profiling test ignored), all seven hulls' measured dimensions,
turret support, screen projection and non-overlap at four viewport sizes, physical
console buttons, hull-envelope docking, per-hull cargo, planet approach selection
and owned-prop commands. Six surface acceptance tests cover continuous control
inputs and ground contact, range/ownership/count limits, persistence, safe hull
migration, larger cargo ledger replay, scaled berths and station/atmosphere geometry.
Traffic tests check identity, continuous routes, hull clearance and unchanged economy.
The final full suite also exercises the existing TCP flight/travel/recovery journeys.

The macOS release package launched as build 26.10.10.14 in a separate local QA world.
Native inspection confirmed the cockpit with transparent forward telemetry, physical
side screens, the permanent Undock control, and a subsequent authoritative undocking
confirmation. Native inspection stopped when the application reported user activity.
Observed frame-rate readings varied (including 12–34 FPS while inspecting the docked
cockpit and 109 FPS after undocking/view change); this is not a controlled frame-pacing
benchmark. Planet appearance throughout descent, every hull's native cockpit,
traffic labels/animation and full-screen performance still need hands-on acceptance.
Do not describe this build as visually or performance-validated for public release.

This is a construction-from-ship prototype on a smooth spherical ground surface:
no walking, rigid-body prop simulation, surface facilities, atmospheric aerodynamics,
terrain elevation or collision against props/stations. Hull flight tuning is shared;
preview ships are free. Background traffic has no economic behavior. MIDI, Stream Deck
and event-driven audio remain design proposals. Separate community relay/universe
hosting, federation and the open-source license are not implemented in this build.

## Resource jobs and remote ship console — 26.10.10.15

The first fleet controller uses ordinary authoritative commands to undock, travel,
lock an asteroid, operate both turrets and five mining drones, recall, return, dock
and deposit. Five new server acceptance tests cover real drone extraction events,
ledger-consistent storage, two concurrent delegated workers, permission rejection,
manual takeover, last-controller disconnect, worker reconnect, revoke, restart,
depleted resources and bounded low-power waits.

An explicitly invoked integration test contacted the configured Spark model through
SSH, validated its 700-unit gathering intent, and sent the resulting request over
real TCP to the packaged server in an isolated universe. The worker disconnected
while its controller stayed connected. The job completed in 90.9 seconds with five
deployed mining drones, actual drone extraction, recalled drones, a docked empty
ship and exactly 700 units in the worker's station storage. The packaged independent
verifier accepted the exported economic history. This is an actual-time journey,
not an accelerated tick simulation. Run it with explicit server/verifier paths:

```sh
ARKLINE_QA_SERVER_BINARY=/absolute/path/arkline-server \
ARKLINE_QA_VERIFIER_BINARY=/absolute/path/arkline-verifier \
cargo test -p arkline-client spark_drives_delegated_drone_haul_over_real_tcp \
  --locked -- --ignored --nocapture
```

Client tests cover bounded model intents, cancellation, stale-session/freshness
checks, cockpit pointer routing and remote camera authorization, system filtering,
large-coordinate origin and module state. A native 1106-by-768 inspection exposed
console overflow; the fix adds a bounded vertical viewport and visible scrollbar.
An exact-size pointer regression scrolls the prompt and Submit control into the
physical screen, clicks the prompt and verifies typed input. Native inspection of
the rebuilt app showed its real exterior remote feed inside the cockpit during an
active gathering job and warp. Full native interactive sign-off remains with the
owner; neither that capture nor the tests prove all cockpit layouts/performance.

Spark is a typed intent interpreter, not a per-frame flight controller. The server
runs the job after acceptance. This release does not add voice, arbitrary fleet
planning, production manufacturing, relay propagation delays or signal-dependent
remote camera degradation. Jobs are one bounded same-system haul per worker;
restart cancels rather than silently resuming. The repository remains private.

## Spoken ship orders — 26.10.10.16

The macOS client adds explicit recording, partial/final transcript display,
Finish/Cancel controls, spoken model replies and confirmed resource-job updates.
Final transcripts follow the same bounded intent path as typed orders. Session,
worker, permission and freshness changes invalidate recording intent; partial
transcripts cannot trigger an action. Recording and playback are mutually exclusive.

All 91 client tests passed, including helper-process framing, cancellation,
deadlines and voice/session invalidation. Strict workspace Clippy and formatting
passed. The bundled speech helper compiled with warnings denied. An actual TTS
request reached its native completion callback and exited successfully. A generated
speech file passed through a separately launched test app and on-device recognition
returned exactly “What is the status of my ship”. No microphone was captured and no
OS permission prompt was automatically accepted. Live microphone/acoustic quality
and external-interface selection remain owner validation items.

The initial command-line recognition probe inherited the coding application's
privacy identity and was terminated by macOS TCC for that parent's missing usage
string. LaunchServices validation succeeded. Launch the packaged app normally from
Finder; the package includes both privacy descriptions and an explicitly targeted
macOS 14 speech helper. No cloud recognition fallback is permitted. Speech bridges
are macOS-only; Rust gameplay and typed instructions remain available elsewhere.

## Player identity and remote piloting — 26.10.10.17

Remote viewing, manual piloting and authenticated player identity are separate.
The main header identifies the player and the exact control/view target, with a
Return to my ship action outside the service panels. Thirteen service destinations
are directly visible in a wrapping grid, replacing More. Native inspection of the
packaged app confirmed the player identity, own-ship label and direct service grid;
full owner-driven remote cockpit usability acceptance remains pending.

All 95 client tests and strict workspace Clippy passed. New client checks cover
watch-only identity, server-confirmed manual routing, return while a request is
pending, account-action exclusion and revocation. Mailbox tests prove old commands
retain their worker target, return clears queued input, and reconnect does not
replay remote thrust. Four server acceptance tests cover explicit permission,
private inventory isolation, denied nested/economic requests, lease switching,
failed-request rollback, reconnect/revoke/logout and restart.

A real TCP journey against the packaged server moved only the delegated worker
with manual thrust while the player stayed docked. EndRemoteControl released the
lease; a stale worker command was rejected, and an ordinary Undock command then
operated the player's own ship through the unchanged authenticated session. This
checks real command routing, rather than inferring it from a camera change.

## Flight, sound and conversational control — 26.10.10.18

Local workspace tests passed, including TCP journeys, server inertia/orbit tests,
resource jobs, authorization and persistence. After the final sound-scene change,
the client suite passed 112 tests with five opt-in tests ignored. Formatting and
workspace Clippy with warnings denied passed. The actual audio-output callback
opt-in test was run separately and passed on this Mac. It proves the device
stream ran, not subjective audibility or mix quality. The actual Spark two-turn
test also passed: a mining expedition clarification followed by “Fill the hold”
yielded the correct cargo-capacity gathering intent without issuing gameplay commands.

Regression coverage includes bounded/recentering Shift-arrow headlook, suppression
of ship rotation while looking, 2D map zoom/pan math, acceleration-based nozzles,
sound transition deduplication and stale-state suppression, preferences, and
conversation/drone intent validation. Both renderer shader programs compiled and
linked in native OpenGL 3.2 validation. The packaged application launched as build
26.10.10.18, connected to an isolated local world and visibly rendered cockpit,
asteroids and live gathering/warp telemetry. The observed HUD reported roughly
119–121 FPS; that observation is not a performance benchmark.

Native inspection stopped sending inputs when the owner began interacting with
the preview. Full native headlook/pan acceptance, device switching, microphone
muting/TTS ducking perception, lighting quality and subjective flight/sound feel
remain owner validation items. The preview remains private and ad-hoc signed.

## Follow-up refinements — 26.10.10.19

Gathering admission now allows dock, normal space and ordinary warp. Tests cover
space initiation with deployed-drone recall, queued warp, carried cargo plus new
mining, an already-complete haul returning directly, and ledger conservation.
Gate transit remains an explicit temporary restriction. Client admission and model
instructions match these server rules. The final client suite passes 115 tests
with five opt-in tests ignored; the six server resource-job tests pass.

Asteroid spin is deterministic, slow and local to each body; render, picking and
beam impact use the same transform. Tests cover center/radius preservation,
rotation timing and geometry agreement. Filtered noise, smoother cue envelopes,
output filtering and additional mix headroom address reported static. Offline
tests cover sample roughness, peak headroom and saturated-voice continuity.
These tests do not substitute for owner listening feedback on the revised mix.

## Multi-trip jobs and drone feeds — 26.10.10.20

The server regression submits a 4,000-ore total to a 1,000-capacity Pioneer and
verifies four real station deposits, cumulative mined/stored counters, final
cargo state and independent ledger replay. All seven resource-job tests pass.
A separate real Spark request interprets the same 4,000-ore goal successfully.

The final client suite passes 119 tests with seven opt-in tests ignored; strict
workspace Clippy passes. The explicitly run native offscreen OpenGL test exercises
actual feed framebuffer rendering, clipped composite pixel readback, state
restoration, reuse and bounded eviction/texture deletion. Camera math, four-feed
paging and real pointer controls are covered separately. The actual audio device
and Spark checks from earlier previews are not implied by the ignored count.

Camera feeds are capped at four visible feeds, 384 pixels on the longest edge,
60 refreshes/second in single mode or 30 per feed in split mode. These are limits,
not measured delivered frame rates. Drone extrapolation is bounded to 350 ms;
freshness and signal checks still suppress unavailable feeds. Gameplay performance
and owner acceptance remain separate from native offscreen correctness.

Packaged native 26.10.10.20 QA connected an isolated pilot, undocked, launched four
recon drones and switched Split feeds → Top down. Four separate labeled scene
feeds were visible within the cockpit screen. The HUD reported roughly118–120FPS
in that scene; this is not a general performance benchmark. Full workspace tests
passed, followed by the final client suite and Clippy after graphics-cache changes.
