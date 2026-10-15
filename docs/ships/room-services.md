# Room-by-room service layouts

Every room in the seven proposed occupied cores now has an individual SVG plan and explicit fixture coordinates in its ship's rooms.json. This is one identified plan per room and one identified endpoint per fitting, not a generic ship-level diagram.

Includes two normal outlets, an essential outlet, two wired data jacks, an RF access point, smoke/heat/air/pressure/leak/occupancy and door sensors, alarm beacon, normal/emergency lights, supply/return air terminals, a service panel, door control and two candidate suppression heads. Inherited wet-service connections appear only where the systems plan declares them. Coordinates and selected quantities are design proposals, not approved equipment coverage or electrical capacity.

Each fixture lists local XYZ and ship XYZ in meters, assigned upstream reference and room isolation group. The 480 suppression fittings reference proposed branch placeholders rather than declared equipment ports; their topology remains unresolved. Local origin is the room aft/port floor corner; X forward, Y up, Z starboard. Shared wall jacks/outlets differ by mounting height. Figures preserve human-scale clearances based on the 1.8 m avatar, with proposed 1.4 × 2.4 m doors and 3.2 m clear rooms.

Suppression has its own proposed deck branch and non-potable supply, not an assumed connection to drinking water. No agent, pressure, coverage radius, sprinkler count certification or real-world installation specification is implied. Equipment footprints, voltage/current/pinout, cable/pipe sizes, mounting structures, RF coverage, signal protocol, hazard classification, release/inhibit behavior and complete physical routes remain unresolved. Hardware must be fitted to actual room meshes before implementation.

All current room placements are proposals. Pioneer has two walkable docked decks. The physical outlets, full room-sensor networks, fire suppression and distributed fluid circuits described here remain proposals. This package covers the explicitly proposed human core, not every possible compartment on the large industrial hulls.

Browse a ship directory for its complete room index. See systems-reference.md and each ship’s plan.svg for the topology and inherited deck reservations. The ZIP includes every earlier ship concept unchanged; this supplement adds room plans rather than replacing that history.

## Ship room indexes

- [Borealis](borealis/rooms/README.md)
- [Hauler](hauler/rooms/README.md)
- [Monolith](monolith/rooms/README.md)
- [Pioneer](pioneer/rooms/README.md)
- [Prospector](prospector/rooms/README.md)
- [Stratum](stratum/rooms/README.md)
- [Vanguard](vanguard/rooms/README.md)

## In-game viewer

Open **Layouts** from the client service tabs or **L** on the service rail. Choose a hull, deck and room, then filter the service types and select a fitting to inspect its room/ship coordinates and assigned upstream reference. **My ship** selects the player’s hull. This works while docked or in flight and reads the bundled design data without a network download. It is a design reference, not live sensor telemetry; it does not imply that the proposed interiors or fixtures exist in the simulation.

[How the ship subsystems work together](subsystem-behavior.md).
