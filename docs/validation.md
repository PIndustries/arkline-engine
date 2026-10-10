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
