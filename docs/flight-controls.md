# Flight controls

The cockpit sends navigation intent. The server owns ship position, velocity,
acceleration, range checks and the active navigation mode. Changing the camera
never moves a ship. Both views display the same authoritative flight state.

## Operating a ship

1. Undock and select a station, the belt or another undocked vessel.
2. Choose **Approach** to close the distance, **Orbit** for a circular path, or
   **Keep range** to maintain the chosen separation.
3. Double-click empty space to fly toward a point. In 3D this picks the horizontal
   plane through your ship; camera pitch changes where the pointer intersects it.
   This is point navigation, not direct joystick steering.
4. Use **Stop** or **Ctrl+Space** to brake. Navigation commands are unavailable
   while docked or in warp. Shortcuts do not interrupt editing a text field.
5. Warp between the station and belt, with charge, launch, cruise and arrival
   feedback. Docking and extraction require being within the server's range limit.

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

This is a single-region flight model. It does not yet include collision damage,
combat, target locking, module fitting, physical mass/inertia upgrades, ship-computer
programs, relay-connected regions or distributed simulation authority. Asteroids
currently represent a shared belt resource rather than individual depleting rocks.

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
```

Keep the GUI and CLI credential stores separate unless deliberately transferring
credentials. These examples use a dedicated command-client pilot.
