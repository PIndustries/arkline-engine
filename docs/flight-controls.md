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

Switch between **2D Tactical**, **3D Tactical**, **3D External** and **Cockpit** in
the header or **More → Settings & view mode**.
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

There is no collision damage, ship combat, module fitting, physical mass/inertia
upgrade system, ship-computer runtime or distributed simulation authority. Attack
drones can damage other deployed drones; that limited model does not include ships.
Asteroid acquisition takes 2–8 seconds, depending on hull sensors and target size.
Each of up to five asteroid locks becomes usable only after server confirmation. Each of the
two mining modules repeats independently while active. Pioneer extracts 25 ore
every 6 seconds per module; Prospector 40/5s, Borealis 75/5s, Stratum 100/4s,
Monolith 100/3s. The utility hulls remain Hauler 20/8s and Vanguard 30/7s.
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

Use **Ship** in the physical console or **Hangar** from the service rail while
docked. Only hulls owned at the current station can be boarded. Selection persists,
and each inactive hull keeps its own cargo and capacitor charge.

| Mining tier | Hull | Length × width × height | Cargo ore | Ore / cycle per turret |
|---|---|---|---:|---:|
| 1 | Pioneer | 72 × 38 × 18 m | 1,000 | 25 / 6 s |
| 2 | Prospector | 180 × 92 × 64 m | 2,500 | 40 / 5 s |
| 3 | Borealis | 640 × 260 × 180 m | 8,000 | 75 / 5 s |
| 4 | Stratum | 1,800 × 800 × 480 m | 24,000 | 100 / 4 s |
| 5 | Monolith | 5,200 × 2,200 × 1,400 m | 80,000 | 100 / 3 s |
| Utility | Hauler | 420 × 180 × 140 m | 16,000 | 20 / 8 s |
| Utility | Vanguard | 1,100 × 380 × 260 m | 4,000 | 30 / 7 s |

The preview grants one of each class, including migration of existing pilot saves;
newly issued inactive hulls start at Junction. Movement tuning is currently shared,
while cargo, extraction and power specifications vary. Mining rates are per turret;
energy, target validity, remaining rock ore and cargo room still limit output.

The station is 20 km across and 6 km high. Docking uses the docking-port marker and
an orientation-independent enclosing hull sphere with radius equal to half the
hull diagonal. **DOCK** is available within 1.8 km of that envelope, so large hulls
need not put their centers into the port. Berth placement scales with hull size to
keep the entire hull outside the station's front plane. This interaction envelope
is not a general collision model. Gate and asteroid interaction rules remain their
own server checks; docking's envelope does not expand mining range.

## Verdant approach and surface sandbox

Return to Junction station before attempting planet approach from the belt or
another system. In **Flight → Travel**, select **Verdant** and choose **Approach
planet**. The client sends normal flight to `[0, -115000, 0]` meters, approximately
5 km above the spherical ground. From the home station this takes roughly three
minutes at the preview ship speed. This is continuous flight, not a planetary
warp destination or teleport. At arrival, use manual thrust to descend and brake;
body-relative down is **C** only while the ship's up axis points away from the
surface. Check the authoritative altitude and ground-clearance readings rather
than assuming your attitude is level.

Verdant has a 6,000 km radius and a 60 km atmosphere boundary. The home station is
about 120 km above the surface, so returning by station warp leaves the ship outside
the atmosphere. Planet approach then crosses that boundary in normal flight; descend
the final 5 km manually. The server prevents the oriented
hull support from going below the spherical surface and removes inward velocity
at contact. Surface state reports altitude, hull ground clearance and whether the
ship is grounded. It does not implement aerodynamic flight, walking or full rigid
body/collision physics.

At ground clearance ≤100 m, **More → Surface sandbox** enables crates, pylons and
platforms. East/north offsets choose a location; the server anchors props to the
sphere. Spawn and movement destinations must be within 500 m of the hull envelope;
moving/deleting also requires the original prop to be within that range and owned
by the pilot. Limits are 32 owned props and 256 total. Position and ownership persist
through restart. There is no terrain deformation, building economy or walking
mode; this is the first bounded construction interface.

Station shuttles, freight tenders and patrols are decorative server-timed traffic.
Their lights and movement convey background activity; they do not participate in
the economy, mine, trade, create credits or appear as connected players.

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


## Cockpit console and drone fleet (26.10.10.11)

Cockpit operations are available through the fixed physical console tabs. Flight
contains target selection and navigation; Modules assigns miners to independently
locked asteroids; Drones launches and controls up to five drones of each class.
More → Settings also exposes all view modes. Overview and flight-help overlays are hidden
in cockpit mode to preserve the canopy view. Instrument clicks suppress flight
input, and maps do not capture right-button aiming.

The upper side screens contain local 2D and ship-relative 3D maps. Scroll controls
real-distance range; click contacts to select them. The lower left screen displays
the selected drone. Choose POV or Orbit camera from Drones. Signal is authoritative
and distance-based, with increasing noise as the link weakens. Only one selected
camera is rendered at once. Recon is observation/navigation; mining drones repeat
real ore extraction; attack drones currently fight other drones only.

## Relay startup (26.10.10.12)

Startup presents the P.Industries relay sequence with animated rings, particles and
staged status text. The relay/session check is earned by the real connection
response; world synchronization requires the signed-in pilot in a server snapshot.
Animation time alone cannot claim success. A short opening-shutter transition
reveals the cockpit after synchronization; Enter cockpit skips the introduction
once ready. Without a pilot, the introduction leads to the normal sign-in form.
Failed connections stay on the relay screen with Retry and pilot/server settings.
Flight commands are blocked while that screen is active. FTL relay is in-universe
presentation of the existing TCP session, not a new network service or security
protocol. Reconnection after playing keeps the existing recovery UI.

## Permanent cockpit actions and power (26.10.10.13)

UNDOCK is always in the physical console header when docked, independent of active
tab or content scrolling. In space that control becomes DOCK; STOP sits beside it.
The visible capacitor gauge reports current server energy. Flight navigation and
power remain accessible without finding the Station tab. Six primary tabs and a
More menu replace the crowded row of eleven services.

Warp charges its reservation once, before departure. It does not continuously
charge the same trip again. Turret mining, drone mining and attack-drone shots draw
energy when work actually happens. The reactor recharges by elapsed server time,
including while parked/offline, capped at hull capacity. A low capacitor blocks
warp and stops unfunded mining/firing. Hover the gauge for per-action costs. Ship
combat guns, thermal damage and customizable power routing remain unimplemented.
