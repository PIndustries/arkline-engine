# Pioneer walkable interior foundation

This feature requires both an updated server and client. It implements one docked, private Pioneer deck. It is not yet a complete ship interior or a utility simulation.

## Use

Dock a Pioneer and choose **Explore Pioneer interior** from station actions. You can also enter from the hangar boarding marker while on foot. Walk with WASD or arrow keys, hold the right mouse button to look, use the wheel to adjust the follow camera, and press Space to jump. The server validates movement and collision.

The airlock, power control, drone workshop, cargo access, stores/service, and workshop connect through the central corridor. Green frames identify open passages; the marked airlock floor is the return point. **Exit airlock to hangar** requires returning within three meters and landing first. From the hangar, **Enter ship** returns to the cockpit; you can then undock. You cannot undock, switch hulls, or control the ship while walking inside it.

An older server does not advertise the `pioneer_interior_available` asset capability. Entry remains disabled with an unavailable message. Other hulls keep their existing hangar/quarters flow.

## Implemented and proposed

| Area | Implemented now | Remaining |
|---|---|---|
| Deck one | 32 × 14 meter local floor plan, six rooms, 2.4 meter corridor and 1.4 meter open door widths from the approved Pioneer plan | Final interior art, operable doors, access control and pressure seals |
| Human movement | 1.8 meter avatar, shared wall/equipment collision, camera wall/ceiling avoidance, jump, leased movement and restart recovery | Full body collision with lintels, ladders and stairs |
| Power control | Confirmed capacitor charge/capacity and reactor output readout in the room | Breakers, buses, load switching, wiring, faults and repair |
| Cargo access | Confirmed active cargo and hull capacity readout | Physical cargo handling and hold traversal |
| Fixtures | Visible grounded equipment enclosures with collision; explicitly proposed/non-operational | Detailed workshop, drone maintenance and storage operations |
| Airlock | Validated transition between the docked private hangar and interior instance | Continuous exterior hatch traversal and pressure simulation |
| Deck two | Design document and layout viewer only | Validated vertical access, bridge, living facilities and all room interactions |
| Life support/water/data | Proposed systems dossier retained | No oxygen, water, heat, RF or electrical network simulation in this change |
| Flight | Existing cockpit after leaving the interior and boarding | Walking aboard during flight, shared multi-person interiors and damage |

The interior uses its own local coordinates, with floor Y=0, X equal to plan U and Z equal to plan V. This implements the published human-core plan as an instance; it does not claim that every room is currently registered inside the exterior procedural mesh. Walls and room fixtures share the authoritative collision description in the protocol crate. The server stores the interior-location flag with the avatar; old avatars default to hangar coordinates. Restart stops walking/jumping while retaining the valid room position.

## Validation

- Server command journey enters all six room doors, collides with a bulkhead, rejects distant exit/undock/hull switch, recovers after restart, exits through the airlock, boards and undocks.
- Legacy JSON defaults to no interior support and a hangar avatar.
- Real egui pointer regression proves the entry button sends a command only with the advertised capability.
- Camera regression checks wall and ceiling limits. An opt-in native GPU capture uses the actual shared room renderer: `ARKLINE_PIONEER_INTERIOR_CAPTURE=/absolute/output.ppm cargo test -p arkline-client native_hangar_renders_real_geometry -- --ignored`.
- Native and browser compilation do not substitute for a final interactive packaged/browser walkthrough. That acceptance remains a merge/release gate.

Track the full interior and systems backlog in [issue #23](https://github.com/PIndustries/arkline-engine/issues/23). This foundation does not close that issue.
