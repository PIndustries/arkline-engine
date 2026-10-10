# Flight controls

The cockpit sends navigation intent. The server owns ship position, velocity,
acceleration, range checks and the active navigation mode. Changing the camera
never moves a ship. Both views display the same authoritative flight state.

## Operating a ship

1. Undock and select a station, belt, individual asteroid, gate or local vessel.
2. Choose **Approach** to close the distance, **Orbit** for a circular path, or
   **Keep range** to maintain the chosen separation.
3. Double-click empty space to fly toward a point. In 3D this picks the horizontal
   plane through your ship; camera pitch changes where the pointer intersects it.
   This is point navigation, not direct joystick steering.
4. Use **Stop** or **Ctrl+Space** to brake. Navigation commands are unavailable
   while docked or in warp. Shortcuts do not interrupt editing a text field.
5. Warp to the station, belt or selected gate. At the gate, use **Jump** when
   within 1.8 km. Jumping changes systems after the three-second transition.
6. Select a particular asteroid in space or the Mining overview. **Lock target**,
   approach within 4.5 km, then **Extract selected**. Only that asteroid loses ore.
   Selection alone does not switch the confirmed mining lock.

Distances and speed in the cockpit are gameplay meters and meters per second.
The renderer scales these to its scene coordinates. Its short bounded position
prediction smooths normal travel between snapshots; it cannot change server state.

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
Warp warm-up and gate transition durations are fixed preview rules.

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
