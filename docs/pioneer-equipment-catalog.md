# Inspectable Pioneer room equipment

The authoritative definition is `crates/arkline-protocol/src/room_equipment.rs`, exposed through `pioneer_room_catalog()`. The catalog has eight stable equipment IDs and thirteen directed, typed connections. It describes the existing first-room implementation; it does not introduce additional operating equipment or change saved state or wire requests.

`position` is a deck-one local coordinate in meters taken from `PIONEER_CONNECTED_ROOM`. A missing position identifies a logical endpoint: the ship supply, bounded controller and scalar pressure measurement are not claimed as separately installed physical hardware. The door point is the center of the same box used by authoritative collision.

Power links identify existing supply dependencies, not a wiring/circuit solver. Command links describe the fixed controller's outputs. Telemetry carries breaker availability or the scalar pressure measurement; the interlock link represents the existing pressure-safe opening check. There are no implied fluid pipes or gas reservoirs. These connections are inspectable metadata for the fixed implementation, not an arbitrary executable graph.

`active_load_w` is a conditional gameplay load. `null` means **not separately metered**, rather than a promise of zero consumption. Door motor transients and pressure-sensor loads are not independently modeled. Controller/light/vent values use the same constants as runtime. The emergency lamp's ten-minute reserve is 6,000 J at 10 W; mains recharge is separately conditional on reserve deficit. No value is a real-world engineering rating.

Generate the machine-readable catalog and connection diagram from the protocol crate:

```sh
cargo run -q -p arkline-protocol --example room_catalog > docs/pioneer-room-catalog.json
cargo run -q -p arkline-protocol --example room_catalog -- --mermaid > docs/pioneer-room-connections.mmd
```

The JSON is a generated inspection artifact; do not edit it independently. Protocol tests check its agreement with the shared catalog, unique IDs/ports/links, endpoint existence, direction and type compatibility, connected ports, valid fixture coordinates, and runtime rating references. The Mermaid diagram is generated from the same node/edge lists. The catalog schema is versioned independently of gameplay protocol version; stable equipment IDs are not vessel IDs. Pair them with `PioneerRoomState.vessel_id` when inspecting a specific owned vessel.

See [room behavior and limitations](pioneer-room-controls.md) for commands, actual outputs, power timing, pressure interlocks, persistence and mixed-client recovery.
