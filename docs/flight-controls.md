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
   approach within 4.5 km, then **Extract selected**. Only that asteroid loses ore.
   Selection alone does not switch the confirmed mining lock.

Distances use meters, kilometers and astronomical units (1 AU = 149,597,870,700 m).
The belt lies 2 AU from the system origin and gates span multiple AU. Local
interaction ranges remain meters. A buffered presentation timeline interpolates
normal flight; warp samples the same analytic trajectory as the server. Rendering
subtracts a nearby origin in double precision before converting GPU coordinates.
These presentation calculations cannot change server state.

The first-person cockpit is the only gameplay view. Full quaternion orientation
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
Asteroid locks are immediate, single-target locks; extraction remains one
server-confirmed batch per activation, not an automatic repeating module.
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

```sh
arkline --pilot TestPilot undock
arkline --pilot TestPilot approach belt 10
arkline --pilot TestPilot orbit belt 1500 20
arkline --pilot TestPilot range pilot:OtherPilot 1000 10
arkline --pilot TestPilot fly 3000 500 2000 10
arkline --pilot TestPilot stop
arkline --pilot TestPilot lock 1
arkline --pilot TestPilot extract 1 25
arkline --pilot TestPilot warp gate:101
# Wait until warp completes, then:
arkline --pilot TestPilot jump 101
```

Keep the GUI and CLI credential stores separate unless deliberately transferring
credentials. These examples use a dedicated command-client pilot.
