# Vessel engineering contract — proposed game-core model

**Status: design contract, not implemented simulation or real engineering certification.** This document proposes a versioned, machine-readable contract for authoritative vessel systems. Existing ship JSON remains a design dossier; this document does not turn point anchors into fitted equipment. No reactor technology, structural material strength, fuel density, crew endurance or real hardware rating is invented here.

## What exists today

The source of current behavior is [protocol quantities](../../crates/arkline-protocol/src/lib.rs), [server power](../../apps/server/src/power.rs), [flight](../../apps/server/src/flight.rs), [mining](../../apps/server/src/mining.rs) and [drones](../../apps/server/src/drones.rs).

| Quantity/system | Implemented meaning | Missing engineering meaning |
|---|---|---|
| Hull length/width/height | Metres, seven class envelopes in `ShipClass::spec` | Structure, fitted material mass, load paths and pressure hull |
| Cargo | Integer ore game units, class capacity and conserved economic storage | Kilograms/unit, volume/unit, material composition, cargo centre of mass |
| Motion | Class/load acceleration multipliers, quaternion attitude, braking and speed limit | Physical inertial tensor, thrust force, torque, propellant or electrical propulsion debit |
| Power | Scalar joule capacitor, fixed watt regeneration, event debits | Fuel/source reserve, conversion efficiency, generator ramp/startup, buses, losses, heat and radiator rejection |
| Offline power | Running reactor replenishes active/parked hulls up to capacity | Fuel burn or thermal consequences; offline engineering simulation |
| Mining/drone actions | Completed cycles/shots debit charge and produce authoritative outcomes | Continuous electrical/thermal duty, machinery temperature and wear |
| Proposed rooms/services | Per-room placements and named topology references | Installed envelopes, pipe/wire routes, circuit capacities and working sensors |
| Damage/integrity | Drone hit points exist | Vessel element stress, fatigue, leaks and structural hull failure |

Pioneer currently has 240,000 J capacity, 1,200 W regeneration, 30,000 J warp admission cost and 4,000 J turret-cycle cost. These are **game balance values**, not sufficient energy to justify the illustrated ship. Regeneration truncates at capacity, retaining fractional millijoules when charging; it does not record rejected surplus. Motion's full ore load increases the tuned inertial multiplier by up to 50%, without defining empty mass in kilograms. Braking/damping removes kinetic energy without a modeled heat or exhaust destination. Warp is an explicit fictional travel subsystem: do not apply ordinary kinetic-energy equations to AU-scale apparent transit velocity.

The dossier's `source_commit` pins an earlier source snapshot. An implementation must compare it with the target ruleset; copied `implemented` fields are not a live query. Existing coordinate forms also differ: `bounds_xzy_lwh_m` orders X,Z,Y, while `position_xyz_m` orders X,Y,Z. Neither supplies a safe implicit renderer-origin conversion.

## Versioned data boundary

Use three separate documents: immutable `VesselSpec`, persisted authoritative `VesselState`, and read-only `VesselTelemetry`. A spec cannot contain live charge/damage; a client cannot upload authoritative state. Every document carries schema version and ruleset ID. State also pins a canonical spec digest. Loading an unknown version or a mismatched digest requires an explicit migration, never silently replacing ratings.

Proposed schema identifier: `arkline.vessel-spec/1-draft`. Until a validator and migration are implemented, this is a schema proposal, not a file the server accepts. JSON must reject duplicate keys, unknown fields, nonfinite numbers, unbounded arrays and dangling references. IDs are stable, unique, case-sensitive strings, at most 64 ASCII letters/digits/underscore/hyphen. JSON integer quantities use checked signed 64-bit storage and checked wider intermediates; never rely on JavaScript floating-point integers for large counters. API serialization of integers outside the exact safe range must use decimal strings.

| Field | Required type and constraint |
|---|---|
| `schema`, `spec_id`, `ruleset_id`, `revision` | Exact schema identifier, stable IDs, positive integer revision |
| `provenance` | Source commit/digest and explicit `draft`/`validated_gameplay` status; a real engineering claim is not an allowed inference |
| `frame` | Explicit right-handed body XYZ metres, +X forward,+Y up,+Z starboard; named origin and proper rigid transform from dossier bounding-box frame |
| `envelope` | Finite min/max XYZ metres, strictly positive extents; mesh/collision asset digests |
| `mass_model` | Dry mass, equipment/cargo/consumable inventory references, local mass centres, inertial tensors; unresolved values tagged, not zero |
| `materials` | Unit conversion definitions for each inventory commodity; kilograms and cubic metres per accounting unit, conservation category |
| `components` | Bounded list of typed system IDs with mounting transform, physical envelope, ports, lifecycle policy and parameter references |
| `connections` | Typed source/target port IDs, direction, capacity, loss model, isolation device and route reference; network types must match |
| `compartments` | Bounded volumes, boundary/door IDs, atmosphere/fluid inventories and occupancy policy; no automatic atmosphere for engine/cargo reserves |
| `structure` | Elements, joints, mount references, allowable gameplay loads, elastic/failure model IDs and declared load cases |
| `balances` | Accounting quantum, fixed-step policy, tolerance and deterministic allocation order for energy, heat and mass |
| `limits` | Maximum nodes/edges/compartments, iterations and substeps chosen through measured server budgets |

An unresolved rating uses a tagged quantity such as `{"status":"unresolved","unit":"kg","value":null}`. A selected game rating uses `{"status":"gameplay_assumption","unit":"kg","value":...}` with an assumption ID and justification; a measured/imported value also needs evidence. An enabled engineering system must reject unresolved required inputs. `null` never means infinite capacity or zero consumption.

Example minimal draft metadata, intentionally **not runnable**:

```json
{
  "schema": "arkline.vessel-spec/1-draft",
  "spec_id": "pioneer",
  "ruleset_id": "engineering-draft",
  "revision": 1,
  "provenance": {"status": "draft", "source_commit": null},
  "frame": {"axes": "X-forward_Y-up_Z-starboard", "handedness": "right", "origin": "declared_body_origin", "dossier_to_body": null},
  "mass_model": {"dry_mass": {"status": "unresolved", "unit": "kg", "value": null}},
  "components": [],
  "connections": [],
  "enabled": false
}
```

The example is only a metadata fragment; the table's full fields and validators are required for an enabled vessel. For a body origin explicitly defined at envelope centre, a dossier corner point maps to `(x-L/2, y-H/2, z-W/2)`. This formula is conditional, not a claim that every existing asset uses that origin. Convert the XZY arrays before applying transforms; keep renderer scale separate from metre-space authority. Require transform round-trip and known socket checks on every asset.

## Components, ports and lifecycle

Component kinds include source converter, energy store, bus, switch/protection, actuator, sensor/controller, heat exchanger, pump, radiator, material tank and passive structural member. A port declares network (`electrical`, `thermal`, `coolant`, `potable`, `waste`, `atmosphere`, `data`, `mechanical`), direction, units and capacity. Electrical joules cannot become coolant mass, and potable/waste networks cannot connect without an explicitly modeled treatment component and quality gate. A data connection conveys authorization/telemetry, not free power or actuator permission.

Persist operational state and condition separately. Operational states: `off`, `starting`, `standby`, `running`, `stopping`, `cooldown`, `isolated`. Condition: `healthy`, `degraded`, `faulted`, `destroyed`. Each transition includes server time, cause, authorized initiator, interlock results and remaining work. A fault cannot erase stored charge, fluid or heat.

- `off → starting`: explicit command, supply/thermal/fuel prerequisites and funded startup work.
- `starting → standby`: startup duration/work complete and diagnostics pass; failure consumes only work actually performed.
- `standby → running`: accepted demand and valid interlocks; generation/consumption ramp within declared limits.
- `running → stopping → cooldown → off`: unload, shut down and dissipate retained heat; pumps may require essential power during cooldown.
- Any operational state may be isolated by protection; isolation removes permitted transfers but does not set temperature or energy to zero.
- `faulted` requires diagnosis/reset policy; `destroyed` cannot reset through reconnect or power cycling. Repair consumes defined resources/time and records the authoritative change.

Vessel modes (`docked`, `free_flight`, `warp`, maintenance) are context, not substitutes for component lifecycle. Essential loads, propulsion and industrial loads have explicit priorities; deterministic shedding keeps protective/control functions ahead of optional loads. Reconnection/restart never restarts isolated equipment implicitly. Offline policy is explicit: either bounded authoritative progression with persisted resources or a documented paused engineering model. Do not combine free offline reactor refill with a claimed conserved fuel system.

## Conservation and production

For each interval Δt, accumulate exact transfers before committing state. Generation consumes a declared source reservoir or records a named external supply. A fictional reactor may use a defined gameplay energy reserve; it must not claim a real fuel's energy density without a selected model. Dock power is a metered external boundary, not an invisible second generator.

Electrical balance:

`ΔE_store = E_generated + E_external + E_recovered - E_useful_loads - E_conversion_losses - E_exported - E_dumped`

All terms are nonnegative transfer totals except signed storage change. Saturated storage forces generation curtailment or explicit export/dump heat. Do not clamp storage and silently discard the residual. Requested demand and delivered demand are distinct; cycle progress depends on delivered work, and failed admission creates neither reward nor debit. A stopped partial cycle retains/loses work only under a specified rule; disconnect cannot reset cost and retain output.

Thermal balance per node:

`ΔU_heat = Q_conversion + Q_equipment + Q_absorbed + Q_in - Q_out - Q_radiated - Q_exhausted`

Internal exchanger transfers cancel globally. Temperature derives from stored thermal energy and declared heat capacity, not a cosmetic timer. A radiator rejects energy to the environment subject to temperature/area/emissivity/view policy; an approximate gameplay rejection curve is acceptable if labeled and bounded. A pump transfers heat through coolant but does not destroy it. Its own electrical losses add heat. Rejected electrical energy is not double-counted as both useful work and waste heat.

Mass balance per commodity/species:

`ΔM = M_loaded + M_extracted + M_received - M_unloaded - M_expelled - M_transformed_out + M_transformed_in`

Chemical/process transformations use explicit stoichiometry or a declared aggregate gameplay conversion with balanced mass. Energy production cannot simply delete fuel mass without an exhaust/retained-product or explicit mass-energy model. Ore economic units remain authoritative until a versioned unit-to-mass conversion exists; migrating to mass cannot mint, round away or duplicate ore across active cargo, inactive hulls, station/corporation storage and escrow. Ejected cargo moves to an authoritative destination, not disappearance.

Source depletion, generated electrical work, heat and event rewards must commit atomically. Snapshot polling cannot generate additional energy. Fixed-point carries and stable component ordering make equal elapsed time independent of packet rate. Explicitly bound catch-up; never skip unprocessed energy while still advancing reward cycles.

## Static and dynamic structure

Static load cases include installed equipment, cargo restraint, dock supports and compartment pressure differential. Free fall does not impose an Earth-gravity weight load; dock/artificial-gravity assumptions must be named. Dynamic cases include actuator forces/torques, changing acceleration, cargo shifts, docking impulses and pressure transients. A cargo mass distribution contributes to centre of mass and inertia, not just a whole-ship speed modifier.

Proposed first implementation is a deterministic element/joint load graph, not finite-element certification. Mount loads travel through explicit structural paths. Track demand versus selected gameplay allowable load, cumulative fatigue/damage policy and failure consequence per element. Use `F = m*a`, moment `r × F`, and an explicitly defined inertia/rotation model only after the requisite mass and geometry exist. Whole-ship integrity is a derived summary, not a substitute for a broken mount or pressure boundary.

An overloaded joint may degrade, detach or isolate attached equipment according to its declared rule. Repair cannot restore missing material for free. Breach/leak flow requires compartment and boundary models; do not implement a red hull percentage and call it atmosphere physics. Warp remains outside normal inertial loading unless a separate fictional warp-load rule is deliberately specified.

## Measurable game acceptance

1. **Schema/geometry:** reject unknown units, duplicate IDs, missing mounts, incompatible ports, unresolved enabled ratings, cycles where prohibited, nonfinite transforms and out-of-envelope fitted hardware. Dossier→body→dossier round-trip error is within the declared coordinate tolerance.
2. **Energy:** over a scripted interval, accounting residual is at most one declared storage quantum per accumulator; full storage cannot create/discard unaccounted energy. Zero fuel/external supply eventually stops generation. Restart preserves energy and fractional carries.
3. **Heat:** a load without an available sink increases thermal energy; an isolated radiator stops rejection; cooldown consumes pump power where specified. Internal exchanges conserve total heat within declared quantum tolerance.
4. **Mass:** every load/unload/extraction/repair/ejection has equal source/destination accounting or an explicit conversion record. Existing ore audit remains exact through migration and interrupted transactions.
5. **Loads:** same thrust at greater actual mass produces lower acceleration; shifted cargo alters centre of mass/inertia; overload follows the selected damage curve and cannot be healed by reconnect. No numeric strength rating is accepted without a gameplay assumption/evidence record.
6. **Lifecycle:** low power stalls startup/cycles predictably; protection isolates the correct branch; essential supply cannot backfeed a failed main bus. A fault survives save/restart, and retry cannot duplicate material or rewards.
7. **Determinism/security:** identical commands and elapsed time produce identical balances across different poll rates. Unauthorized resets/repairs are denied without state change. Catch-up and graph solve costs have measured, enforced limits before deployment.
8. **Player proof:** undock, accelerate/brake, mine to capacity, return/dock/unload and repeat while displaying delivered load, stored energy, heat, mass and limiting fault from server telemetry. Include emergency shutdown, loss of cooling, interrupted save and recovery. Every UI number must trace to an authoritative quantity, not the dossier diagram.

Implement in small gates: schema/conservation harness first; electrical source/store/load lifecycle second; thermal rejection third; cargo mass and mounts fourth; structural and compartment effects afterward. Preserve current play and economic conservation behind explicit ruleset migration. This sequence is proposed engineering work, not a claim that the present game has these systems.

## Source register

- [Protocol quantities and class rules](../../crates/arkline-protocol/src/lib.rs)
- [Power accumulation/debit](../../apps/server/src/power.rs)
- [Authoritative flight](../../apps/server/src/flight.rs)
- [Pioneer dossier](pioneer/specification.json), [functional topology](pioneer/systems.json)
- [System layout coordinate limits](systems-reference.md), [room services](room-services.md), [subsystem behavior](subsystem-behavior.md)
