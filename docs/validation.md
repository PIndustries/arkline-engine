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
