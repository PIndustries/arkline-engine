# Pioneer airlock room controls — first connected slice

This implements one deck-one room controller, not a complete ship PLC, atmosphere solver or electrical network. All ratings are gameplay tuning. The shared `PIONEER_CONNECTED_ROOM` definition places the breaker, overhead light, vent, emergency fixture and corridor door in the existing interior coordinates. No exterior airlock opening, oxygen consumption, player suffocation, heat flow or explosive decompression is simulated.

## Authority and lifecycle

The private `AssetsView.pioneer_room` reports the active Pioneer's vessel ID, commands and actual outputs. Existing vessels initially report an **uncommissioned** controller with no operational lights/vent; the first authorized command commissions and persists it. Older servers omit the field. Clients must not infer powered equipment from a missing state.

`SetPioneerRoom { action }` accepts only the typed breaker, light, vent and door actions. There are no arbitrary register writes, scripts or user-provided equipment identifiers. Authentication is required. The pilot must own and currently operate the Pioneer, either aboard using engineering controls or inside its first deck. Access from the hangar floor or another class/deck is denied. Remote-pilot delegation and corporation membership grant no access. Changing active ship preserves this Pioneer's controller rather than transferring it to the replacement hull.

State is saved in the same SQLite world transaction as other commands. It survives restart and advances bounded timers from the saved timestamp. Commands and actual state differ deliberately: an open request can remain interlocked while the actual door stays closed; a light request survives breaker loss while its actual output turns off.

## Power source and loads

The existing always-running ship reactor/capacitor model supplies the room. With the breaker closed, the controller draws 4 W, an enabled light 24 W, and an operating vent 96 W. Emergency reserve recharge adds 10 W when needed. These loads reduce the existing reactor's capacitor recharge rate for this vessel, including when the Pioneer is parked. They are not extra free power and do not change cargo or credit accounting. The first slice stays below the Pioneer's existing reactor output; it does not model wiring losses, overload protection, motor transients or a switched reactor.

The breaker disconnects room mains. Loss of mains turns off the actual normal light and vent. A separate initially charged emergency reserve provides ten minutes of light at an equivalent 10 W (6 kJ). The server decrements its remaining duration and stops its light at exhaustion; powered recharge replenishes it at the same rate. There is no player health consequence when it expires.

## Door and pressure interlocks

The initial room and fixed corridor reference are 101 kPa. With the door closed and mains available, Equalize raises pressure toward 101 kPa and Exhaust lowers it toward zero, both at a bounded 2 kPa/s. Hold stops the vent. This is a single scalar test model: it does not conserve gas against a reservoir, exchange heat, or represent a whole vessel atmosphere. An open door normalizes pressure to the corridor reference.

Opening requires a pressure difference of at most 5 kPa. Manual actuation bypasses motor power only; it never bypasses this interlock. An accepted but blocked open request remains commanded and opens once its conditions become safe. Closing is rejected while the avatar occupies the doorway. The server also checks occupancy before executing a previously blocked closing command.

The actual closed door adds its shared 1.4 m doorway obstacle to authoritative walking collision on deck one. Deck two is unaffected. The client must render the same door leaf and use actual outputs for illumination. The door does not imply a modeled route to vacuum or the ship exterior.

## Verification and remaining work

`apps/server/tests/room_controls.rs` covers breaker loss, emergency depletion/recharge, manual actuation, pressure interlocks, real walking collision, occupied-door rejection, unauthenticated/wrong-context rejection, isolation between pilots, per-vessel persistence across class changes and restart, and the exact reduced capacitor recharge rate. Native/browser fixture rendering and usability are separate acceptance gates.

Future work includes explicit generators/buses/breakers, motor and thermal loads, gas reservoirs, many-room topology, fault propagation, crew safety, per-device maintenance, and constrained programmable logic. None is implied by this first controller. Legacy interior clients do not render the moving leaf; while they still receive authoritative collision, updated clients are required to operate and see these room devices.

## Mixed-version safety

Updated clients opt in with `Connect.supports_room_controls`; omission means unsupported. After successful token authentication, an older connection puts this controller into compatibility recovery: pressure resets to the fixed corridor reference, ventilation stops and the door is held open. This is an explicit recovery of the simplified scalar model, not a claim of physical instantaneous equalization. Incorrect tokens cannot trigger recovery.

Any incompatible connection latches room operations off until every session for that pilot disconnects. A newer overlapping session cannot reclose an invisible door or start exhaust; reconnect all clients after closing older versions to enable controls again. The safer open/hold commands remain after recovery rather than secretly resuming a previous exhaust request. Missing room fields in older saves initialize uncommissioned, preserving existing gameplay loads until the first authorized room command.
