# Queued side-camera mechanisms and cargo overview

Read-only design plan following the privately preserved captain checkpoint b76ce0f.
The current drone-telemetry slice remains separate; none of the additional
mechanisms described here is implemented by this document.

## Two external side cameras on physical overhead screens

Requested order is D1, Port, D2, D3, D4, Starboard, D5, retaining the two lower
turret screens: nine moving monitors in total. The unchanged apparent gaps cannot
fit another readable 0.64m panel: midpoint insertion overlaps conservative moving
envelopes by about0.13–0.15m and fixed cassette pockets by0.19–0.21m. Tiny plates
are not the solution.

A quantified but not full-scene-validated candidate uses the same0.64×0.36m
panels on radius2.62m around canonical eye XZ[31.48,0], bearings−54,−36,−18,0,
+18,+36,+54degrees facing inward. Port-side centers: D1[33.020,−2.120],
Port[33.600,−1.540],D2[33.972,−.810],D3[34.100,0], mirrored starboard.
Current Y2.45→1.95 travel is retained. Adjacent conservative cassette envelopes
have about63mm separation; outer extent reaches |Z|2.46m. Canopy, crew capsules,
full remaining sweeps, seat/aisle paths and glass rays still require actual tests.
Keep original eye/FOV and readable glance views. Five fixed metric bands follow
the five associated drone cassettes, not the external-camera slots.

Preserve stable hardware IDs0–6; append port7/starboard8, independent of drone IDs.
Do not blindly expand persisted seven-element arrays: additive defaulted external
actuator/mode pairs and shared nine-slot accessors preserve deserialization.
Registration10 fresh matching fixtures must reject prior bound saves unchanged.
AUTO deploys in confirmed normal space, including initial load, and stows during
InTransit/Jumping. Manual STOWED persists after arrival; OPEN preference persists
but warp overrides actual travel. There is no first-warp latch or local arrival
timer. Owner checks and monitor-only captain controls extend to all nine.

Camera identity includes vessel and exterior side, separate from assigned drones.
Actual body-frame mounts/FOV should show the corresponding hull flank as a useful
reference, with honest own-hull occlusion rather than hidden hull or fabricated
video. Reserve permanent reactor surface15 and metric surfaces16–20; append new
surfaces explicitly. Worst-case nine plus four manual images requires13distinct
capture slots, with measured age/performance and conservative capture scheduling.
No camera/motor electrical simulation is implied. Geometry/coordinator,
protocol-server/authority and feeds-UI/runtime need explicit ownership before work.

## Dedicated permanent cargo screen

The existing fixed starboard screen7 can show CARGO independently of all moving
camera monitors. Authoritative onboard cargo is PilotView.ore in generic ore
units; Pioneer capacity is1000units. Show used/free/capacity and a clamped fillbar,
with invalid-state diagnostics rather than silently accepting impossible values.
Contents can truthfully say 'Ore N units', not fabricated minerals, kg or m³.
Snapshot.stored_ore is separate personal station storage and must not inflate the
ship's hold.

ResourceJob supplies phase,target_units,mined_units,stored_units,target_id,message
and update time. Show confirmed job progress separately from hold capacity.
Depositing is a real phase, but current DepositOre/WithdrawOre operations are
instant atomic transfers, not animated loaders. No per-transfer fraction or
throughput is exposed; a decrease in ore alone cannot prove unloading. Ledger
PersonalOreDeposited/Withdrawn events exist in export, not a live snapshot event
stream. Paused/stale/disconnected states show last-known data honestly.

Implementation mapping proposal: dynamic surfaces8–14 remain unchanged; exterior
port/starboard append as21–22 with explicit hardware-to-surface mapping rather
than `slot+8`. Cassette range checks must remain16–20. Runtime integration points
are `physical_cockpit::append_feeds`, `DisplayFeed` identity/capture_key and a body
camera helper alongside `flight3d::turret_camera_frame`, using `Space::position`
and `ship_orientation_at`. Existing walking/body registration remains shared.
`MAX_CAPTURE_SOURCES` would move11→13 with one capture per frame retained.
These are planned changes, not validated implementations.
