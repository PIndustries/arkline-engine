# Flight controls

The cockpit sends navigation intent. The server owns ship position, velocity,
acceleration, range checks and the active navigation mode. The cockpit follows the ship’s full orientation. Direct controls and autopilot
commands operate on the same authoritative flight state.

## Operating a ship

1. Undock and select a station, belt, individual asteroid, gate or local vessel.
2. Choose **Approach** to close the distance, **Orbit** for a circular path, or
   **Keep range** to maintain the chosen separation.
3. Fly directly with W/S (forward/reverse), A/D (strafe), Space/C (up/down),
   Q/E (roll), and arrow keys (pitch/yaw). Hold right mouse over unobstructed
   space to aim. These axes are relative to your ship, including after a roll.
4. Release controls for assisted braking, or use **Stop / Ctrl+Space**. Keyboard
   controls do not run while typing or while the window is unfocused. Manual
   input takes over from autopilot; an explicit autopilot action clears stale input.
5. Warp to the station, belt or selected gate. At the gate, use **Jump** when
   within 1.8 km. Jumping changes systems after the three-second transition.
6. Select a particular asteroid in space or the Mining overview. **Lock target**,
   wait for lock confirmation, approach within 4.5 km, then activate a mining module. Its cycles repeat until stopped; only the confirmed asteroid loses ore.
   Selection alone does not switch the confirmed mining lock.

Distances use meters, kilometers and astronomical units (1 AU = 149,597,870,700 m).
The belt lies 2 AU from the system origin and gates span multiple AU. Local
interaction ranges remain meters. A buffered presentation timeline interpolates
normal flight; warp samples the same analytic trajectory as the server. Rendering
subtracts a nearby origin in double precision before converting GPU coordinates.
These presentation calculations cannot change server state.

Switch between **2D Tactical**, **3D External** and **Cockpit** in the header.
Changing views preserves the selected object and navigation state. Full quaternion orientation
preserves roll independently of movement direction. Manual-flight presentation uses
a shorter interpolation buffer than autopilot. Rotation input expires after 500 ms
without a fresh command; translation then brakes instead of continuing stale thrust.

## Persistence and connection loss

Ships stay in the world when the pilot disconnects. When the last authenticated
session closes, the server stops manual navigation and retains the current ship
position. An already initiated warp follows its durable arrival timestamp. Ship
programs are not running in this build. A network blackhole is detected by timeout,
so the server may not know immediately that a client has become unreachable.

Existing worlds gain flight state on loading without changing cargo, balances,
orders or economic history. Movement is stored independently of economic events;
the economic verifier checks the economy, not the truth of physical trajectories.

## Preview boundaries

Three test systems run inside one server authority:

```text
Junction <----> Ember Reach <----> Glassway
          gates             gates
```

Each system has its own visible rocks and local ships. The overview filters
All / Mining / Travel / Pilots and orders objects by distance. Space brackets and
rows select the same object ID. Gate jumps preserve cargo, credits and position
in the destination system across restart. The economy remains a shared exchange.

There is no collision damage, combat, module fitting, physical mass/inertia
upgrade system, ship-computer runtime or distributed simulation authority.
Asteroid acquisition takes 2–8 seconds, depending on hull sensors and target size.
Each of up to five asteroid locks becomes usable only after server confirmation. Each of the
two mining modules repeats independently while active. Pioneer extracts 25 ore
every 6 seconds per module; Prospector 40/5s, Hauler 20/8s, Vanguard 30/7s.
Cycle progress and stop reasons come from the server. There is no yield input.
Stop cancels the current unfinished cycle without a partial award. The last cycle
may award less when cargo fills or the asteroid is depleted. Lock/range loss,
warp, docking, jumping, last disconnect and server restart stop modules.
Reconnecting does not restart them; suspended time does not accumulate ore.
Warp alignment/warm-up and gate transition durations are fixed preview rules.
Long warps include a six-second local launch and six-second terminal approach.
The ship crosses the first and final 30 km continuously, with matched velocity
at phase boundaries. The direction-aligned tunnel is optical feedback, not a
replacement for moving through authoritative world coordinates. Alignment is a
fixed turn during warm-up; ship mass/agility and warp disruption are not modeled.

## Command-client testing

Navigation commands keep their connection open for the supplied duration (1–300
seconds), then send Stop. Closing the process ends its authenticated session;
when that was the pilot's last session, manual navigation stops server-side.

`mine [SECONDS]` starts module 0 on the confirmed target;
`extract ASTEROID_ID [SECONDS]` names that target explicitly. Duration defaults to
60 seconds and accepts 1–3,600 seconds. These commands hold the connection open
until the duration ends or the module stops automatically, then stop the module.
Duration is not an ore quantity: ending mid-cycle awards no partial ore.
`stop-mining [MODULE_ID]` defaults to module 0.

```sh
arkline --pilot TestPilot undock
arkline --pilot TestPilot approach belt 10
arkline --pilot TestPilot orbit belt 1500 20
arkline --pilot TestPilot range pilot:OtherPilot 1000 10
arkline --pilot TestPilot fly 3000 500 2000 10
arkline --pilot TestPilot stop
arkline --pilot TestPilot lock 1  # waits for confirmed lock
arkline --pilot TestPilot extract 1 30  # asteroid ID, activation seconds
arkline --pilot TestPilot stop-mining 0
arkline --pilot TestPilot warp gate:101
# Wait until warp completes, then:
arkline --pilot TestPilot jump 101
```

Keep the GUI and CLI credential stores separate unless deliberately transferring
credentials. These examples use a dedicated command-client pilot.

## Hull selection and station scale

Use Services → Hangar while docked to board Pioneer (72 m), Prospector (180 m),
Hauler (420 m) or Vanguard (1.1 km). Selection persists on the server and is rejected
while in space. Preview hulls share movement tuning and cargo capacity. The station
is 20 km across and 6 km high. Station ranges identify the docking port.

## Target acquisition and mining turret

The pending target displays a countdown; it cannot be mined before server confirmation.
Individual unlock releases that target. Requesting another target adds an acquisition
up to five total pending/confirmed locks. Select a confirmed target in the rack before
activating each turret; active assignments remain unchanged when browsing other targets. Range loss, depletion, warp, docking and
jumping invalidate targeting. Pending acquisition is cancelled on restart or last
connection loss. The command client keeps its connection open until lock confirmation.

The two mining mounts independently track their authoritative assigned targets with a limited
visual turning rate. Each active module uses its own turret muzzle for the extraction beam. This is a mining
module, not a combat damage system or a general module-fitting inventory.
