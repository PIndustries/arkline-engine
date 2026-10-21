# Functional Pioneer room program

Existing authority and missing dependencies must be distinguished. This is a room-by-room implementation queue, not a claim that every named room already functions.

| Room | Existing real state/actions | Missing requirement |
|---|---|---|
| D1-P1 Airlock/EVA | Prototype room pressure, breaker, lights, vent, door, emergency reserve and interlocks; existing physical engineering panel | Whole-ship atmosphere/boarding-room program; room controller is not the upper boarding actuator system |
| D1-S1 Power | Ship capacitor charge/regeneration and actual loads | Physical buses, switching, faults and repair authority |
| D1-P2 Drone workshop | Deployed drone assignment/position/hit points and orders; mined yield goes to owner hold | Bay stock/physical handling/service/repair; no independent drone cargo or batteries reported |
| D1-S2 Cargo access | Ship ore, docked station deposit/withdraw | Physical material/containment/payload model and loading mechanics |
| D1-P3 Stores/service | Account inventories | Installed room stock and consumed services |
| D1-S3 Workshop | Passive fitted room | Tools/fabrication/repair simulation and permissions |
| D2-P1 Bridge/nav | Fitted bow bridge piloting/navigation | Reconcile old room label with physical bridge without moving saved rooms |
| D2-S1 Crew/galley | Collidable furniture | Food/water/crew needs and consumption |
| D2-P2 Sensors/comms | Real contacts and telemetry | Room equipment/sensor/comms operation authority |
| D2-S2 Medical/rest | Passive room | Injury/treatment/rest simulation |
| D2-P3 Operations | Existing mining routines/fleet orders | Physical terminal access contract, not cloned generic panels |
| D2-S3 Life support | No whole-ship system | Conserved atmosphere/thermal/water networks and equipment |

First independent functional correction: existing room_controls panel now toggles door_commanded_open instead of achieved door_open, so an interlocked pending open/close can be cancelled. Requested and actual door/light/vent outputs remain distinct; UI never predicts success. Both manual/powered requests reuse the same typed server commands and interlocks. Focused pointer checks cover cancellation/disabled state/no mutation and actual labels.

Current access limits are explicit: client panel_access checks own docked Pioneer/compatible Deck 1 and three-metre breaker proximity. Server checks own-vessel room compatibility/Deck 1 or seated context, occupancy, power and pressure, but does not enforce that 3 m proximity or an expected binding on this command. No server-enforced range claim is made. New distributed room gameplay needs an authoritative context/range/binding contract before presenting more physical controls.

Next implementation should select an existing real subsystem and a room-specific useful interaction, then add any missing authority contract explicitly. Do not decorate passive rooms with fabricated repair/pressure/service controls or duplicate generic management panels.
