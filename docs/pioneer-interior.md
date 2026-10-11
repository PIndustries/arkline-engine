# Pioneer walkable interior foundation

This feature requires both an updated server and client. It implements both docked, private Pioneer decks. It is not yet a complete ship interior or a utility simulation.

## Use

Dock a Pioneer and choose **Explore Pioneer interior** from station actions. You can also enter from the hangar boarding marker while on foot. Choose **First person** or **Third person** in On-foot navigation. Walk with WASD or arrow keys, hold the right mouse button to turn/look, and press Space to jump. Third person follows behind the avatar; the wheel adjusts follow distance. Backward movement and strafing preserve facing direction. Ship flight, mining, drones, hull switching and AI job controls are unavailable on foot; return through the airlock and board to use them. Inventory and room plans remain accessible. Servers without the advertised interior capability explicitly show entry as unavailable. The server validates movement and collision.

Deck one contains the airlock, power control, drone workshop, cargo access, stores/service, and workshop. Deck two contains bridge/navigation, crew/galley, sensors/comms, medical/rest, operations, and life support. Both use the approved central corridor layout. Walk to the marked access-core landing at the corridor end and choose **Use lift to deck 2** or **Use lift to deck 1**. The server requires grounded proximity within 1.2 meters, validates the destination deck, and uses a fixed landing. This is a controlled instantaneous lift transition, not an animated or physically simulated lift. Green frames identify open passages; the marked airlock floor is the return point. **Exit airlock to hangar** is available only on deck one and requires returning within three meters and landing first. From the hangar, **Enter ship** returns to the cockpit; you can then undock. You cannot undock, switch hulls, or control the ship while walking inside it.

An older server does not advertise the `pioneer_interior_available` asset capability. Entry remains disabled with an unavailable message. Other hulls keep their existing hangar/quarters flow.

## Implemented and proposed

| Area | Implemented now | Remaining |
|---|---|---|
| Both decks | Two 32 × 14 meter local floor plans, twelve rooms, 2.4 meter corridor and 1.4 meter open door widths from the approved Pioneer plan | Final interior art, operable doors, access control and pressure seals |
| Human movement | 1.8 meter avatar, shared wall/equipment collision, camera wall/ceiling avoidance, jump, leased movement and restart recovery | Full body collision with lintels, ladders and stairs |
| Power control | Confirmed capacitor charge/capacity and reactor output readout in the room | Breakers, buses, load switching, wiring, faults and repair |
| Cargo access | Confirmed active cargo and hull capacity readout | Physical cargo handling and hold traversal |
| Fixtures | Visible grounded equipment enclosures with collision; explicitly proposed/non-operational | Detailed workshop, drone maintenance and storage operations |
| Airlock | Validated transition between the docked private hangar and interior instance | Continuous exterior hatch traversal and pressure simulation |
| Deck access | Server-validated fixed lift landing in the reserved access core; deck identity persists through restart | Animated lift/stairs, continuous vertical geometry, bridge and living-facility operations |
| Life support/water/data | Proposed systems dossier retained | No oxygen, water, heat, RF or electrical network simulation in this change |
| Flight | Existing cockpit after leaving the interior and boarding | Walking aboard during flight, shared multi-person interiors and damage |

The interior uses its own local coordinates, with floor Y=0, X equal to plan U and Z equal to plan V. This implements the published human-core plan as an instance; it does not claim that every room is currently registered inside the exterior procedural mesh. Walls and room fixtures share the authoritative collision description in the protocol crate. The server stores the interior-location flag with the avatar; old avatars default to hangar coordinates. Restart stops walking/jumping while retaining the valid room position and deck.

## Validation

- Server command journey enters all twelve room doors across both decks, collides with a bulkhead, rejects distant exit/undock/hull switch, recovers after restart, exits through the airlock, boards and undocks.
- Lift tests reject distant, airborne, same-deck and invalid-deck requests; deck-two airlock coordinates cannot exit the ship. Restart retains deck two.
- Legacy JSON defaults to no interior support and a hangar avatar on deck one.
- Real egui pointer regression proves the entry button sends a command only with the advertised capability.
- Camera regression checks wall and ceiling limits. An opt-in native GPU capture uses the actual shared room renderer: `ARKLINE_PIONEER_INTERIOR_CAPTURE=/absolute/output.ppm cargo test -p arkline-client native_hangar_renders_real_geometry -- --ignored`.
- Native and browser compilation do not substitute for a final interactive packaged/browser walkthrough. That acceptance remains a merge/release gate.

Track the full interior and systems backlog in [issue #23](https://github.com/PIndustries/arkline-engine/issues/23). This foundation does not close that issue.

### Input-driven application acceptance

Run `cargo test --locked -p arkline-client both_decks_pointer_keyboard -- --ignored --nocapture` for the deterministic full-application journey. It runs `Cockpit::update` through real egui pointer press/release and held-key events, drains the actual command/control mailboxes at a 20 Hz control cadence, and applies those emitted requests to the authoritative server with a simulated clock. It visits all twelve rooms, clicks both lift transitions, returns through the airlock, boards, and undocks. It also checks that changing decks preserves camera look direction. The updated camera/control journey emitted 1,100 client requests.

This is stronger than injecting walking commands directly into the server, but it is **not** an operating-system keyboard/window, TCP transport, or GPU UI-compositing test. The separate native GPU captures inspect the actual room/character geometry; they do not prove OS event delivery. Native accessibility could activate entry during review, but short injected key pulses did not produce a complete walkthrough. Do not report the full native OS journey as passed from these tests alone.

The regular application-input regression clicks both on-foot camera modes, walks forward/backward with WASD and arrow keys, checks preserved heading, turns the authoritative avatar with an unobstructed RMB drag, and verifies ship-only requests cannot escape the on-foot command boundary. It also checks ship-operation service tabs show on-foot navigation instead of operational controls.

## Older clients and safe rollback

Connect now includes optional `supports_pioneer_interior` (default false). The
current visual client declares support; old clients and terminal clients do not.
Only after valid resume-token proof, an incompatible reconnect safely moves an
interior avatar to its existing hangar boarding marker, stops walking/jumping,
and clears seating. It preserves the ship, cargo, storage and economic ledger.
A capable reconnect retains the interior deck and position.

A conservative per-pilot compatibility latch blocks interior entry while any
session remains after an incompatible connection. Closing just the old connection
cannot identify and clear this latch: close older/terminal connections, then
reconnect **all** remaining sessions using current visual clients. This protects
simultaneously connected older clients; claiming support cannot clear another
session's protection. It does not prohibit normal flight/mining with old clients.
The latch is ephemeral and clears on the last disconnect or server restart.

This is forward-client recovery, not save downgrade compatibility. Before a server
binary downgrade, evacuate all interior avatars with supported clients or use a
reviewed consistent backup/restore procedure. Old servers ignore the new fields
and cannot interpret interior coordinates/decks correctly. Do not blindly restore
a stale world backup over subsequent economic transactions. Validate Test first,
including old/new concurrent sessions and restart, before a Production upgrade.
