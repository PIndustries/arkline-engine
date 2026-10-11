# Implementation gaps and acceptance

## Present in the inspected source

- Seven hull bounding dimensions and cargo capacities; five mining tiers.
- Two mining module IDs and per-class prototype power cost/capacity/regeneration values.
- 1.8 m station avatar, 0.3 m collision radius, walking/jump behavior.
- Private station hangar around a hull; 18 × 12 × 3.2 m station quarters beside it.

## Not established / proposed by these dossiers

1. **Walking inside ships:** current station collision excludes the ship hull, and walking state is removed when undocked. Need explicit ship-interior location, local coordinates on moving ships, ownership, authoritative movement, boarding transitions, reconnect persistence and camera collision. Do not assume station walking supplies these.
2. **Complete fitted interiors:** vector rectangles fit only a bounding box. Fit decks to actual hull surfaces; allocate skin, structure, engines, tanks, conduits and service clearance; verify no overlap. Detailed furniture, stairs, collision capsules, navmesh and multiple escape routes are not supplied.
3. **Doors and vertical travel:** implement moving door collision, player clearance, interaction/permissions, lift platforms, landing geometry, animation and multiplayer replication. Pressure/airlock simulation is a separate design choice.
4. **Power rooms and wiring:** existing values describe a scalar energy budget. Reactor meshes, physical placement, capacitor banks, cable paths, per-circuit allocation, thermal rejection, faults, isolation and repair do not follow automatically from that budget. All physical routing in the dossiers is proposed. Hotel loads/life support are not claimed as simulated.
5. **Cargo scale:** define mass and volume per ore/item and stack rules before equating hold capacity to physical bins or handling equipment. Cargo movement, cranes and production lines in the concept art are proposed.
6. **Onboard hangars:** static bay-envelope fits are only preliminary. Need docking permissions, approach volumes, swept collisions, doors, ship attachment, launch/recovery and network handover tests. Larger ships' dispersed service routes remain to be designed.
7. **Art pipeline:** implement bounded GLB/material/texture loading, named articulating nodes, LOD, collision proxies, asset integrity/license manifests and native/WASM parity. Images are not meshes. Holographic inspection is a separate rendering pass and needs selection/data integration.
8. **Performance:** measure textures, draw calls, GPU memory, interior streaming, shadows, transparent holograms and multiple feeds on browser/native targets before selecting production budgets.

## Acceptance sequence

Start with Pioneer: construct a graybox at the source hull dimensions, use the 1.8 m avatar, walk airlock → corridor → bridge → service room, traverse all planned decks, operate doors, verify turret clearance and undock/board/reconnect behavior. Check standing/jump headroom, narrow turns and sliding collision. Then fit the approved visual hull, compare with plan and repeat runtime acceptance. Expand to the other classes only after this loop is demonstrated. Numerical containment alone is not this acceptance.

## Validation of this deliverable

The generator checks human-core/deck envelope containment and minimum room widths in each hull bounding box. Exact coordinates are recorded in JSON; these checks do not prove egress or runtime walkability. The images are conceptual zoning illustrations. If an artistic annotation differs from a JSON value, the JSON/source table wins. No game code or production asset was changed by this design package.

Tracking: https://github.com/PIndustries/arkline-engine/issues/23 (walkable ship interiors), https://github.com/PIndustries/arkline-engine/issues/10 (asset pipeline), https://github.com/PIndustries/arkline-engine/issues/7 (movement/interface stability). Interior implementation is separate from the current lighting release.
