# Arkline — proposed ship system layouts

These seven editable SVG sheets and seven JSON specifications expand the fleet's existing room and zone reservations into **proposed functional topology**. They are not certified engineering, fitted equipment, construction instructions or completed gameplay. The current game prevents station avatars from walking through the ship hull; no systems in this directory implement ship interiors.

## Authority and files

The numerical input is `fleet-specifications.json`, source snapshot `cb76d975421937d4cb4ae5da57cb5488843b108b`. Read the [design reference](design-reference.md) and [gap register](gaps.md) alongside these sheets. Original concept images are visual references, not dimensional authority.

| Ship | SVG schematic | JSON endpoints and room services | Occupied decks | Room branches |
|---|---|---|---:|---:|
| Pioneer | [Pioneer.svg](pioneer/systems.svg) | [Pioneer.json](pioneer/systems.json) | 2 | 12 |
| Prospector | [Prospector.svg](prospector/systems.svg) | [Prospector.json](prospector/systems.json) | 3 | 18 |
| Hauler | [Hauler.svg](hauler/systems.svg) | [Hauler.json](hauler/systems.json) | 4 | 24 |
| Vanguard | [Vanguard.svg](vanguard/systems.svg) | [Vanguard.json](vanguard/systems.json) | 6 | 36 |
| Borealis | [Borealis.svg](borealis/systems.svg) | [Borealis.json](borealis/systems.json) | 5 | 30 |
| Stratum | [Stratum.svg](stratum/systems.svg) | [Stratum.json](stratum/systems.json) | 8 | 48 |
| Monolith | [Monolith.svg](monolith/systems.svg) | [Monolith.json](monolith/systems.json) | 12 | 72 |

Each sheet contains six color-separated networks, 54 numbered equipment/interface anchors, 66 named inter-equipment connections, exact current gameplay energy values, a coordinate register, deck service waypoints and acceptance gates. JSON expands every room's normal power, essential power, data, air supply/return and applicable wet services into explicit source/target endpoints and separately named branch isolators. Arrows indicate functional flow; data connections require bidirectional communications even where a control-flow arrow is shown. Tables enumerate all connections; short arrows in the block grid show adjacent links without implying the other listed links are absent.

Open SVGs in a browser/vector editor and zoom. The 1800 × 3400 canvas is a long schematic sheet, not a printable single-page scale drawing. SVG coordinates are display coordinates, not vessel metres. JSON coordinates are authoritative within this proposal.

## Coordinate and placement contract

`position_xyz_m` is **X, Y, Z**: X forward from the aft bounding-box face, Y up, Z starboard from the port face. This differs from the parent zone array ordering `bounds_xzy_lwh_m` (**X, Z, Y, length, width, height**) and from the current game mesh origin. Conversion must be explicit before implementation.

Equipment nodes are distributed **point anchors**, inside nominated inherited rooms or the engine/cargo reserve. They do not reserve equipment dimensions, working clearance, structural foundations or pipe/cable bend space. Room service anchors are inside their room volume; deck service waypoints are within the existing corridor and human-core envelope. They do not certify overhead headroom after installation. JSON sets equipment envelopes and physical routes to `null` rather than implying an engineered route.

The same logical architecture is mapped separately to each ship's real proposed coordinates/deck count. Large ships retain the compact human core from the parent plans. They still need additional distributed service islands, remote maintenance access and industrial transport paths; this package does **not** invent a fully habitable 5.2 km Monolith interior or extrapolate thousands of crew.

## Power delivery and wiring

**P01 → P02 → P03 → P04** represents source equipment, main disconnect/breaker, charge storage and controlled distribution. Separate industrial and hotel feeders supply propulsion/warp, turret circuits 0 and 1, drone service and occupied-room systems. P08 is a distribution interface for two separately isolated turret circuits, not a third mining module. Every physical turret/actuator socket still needs model-level placement.

Hotel circuits explicitly feed communications, air handling, water pumping and secondary cooling. The independent E01 reserve and E03 essential bus provide a proposed alternate path to critical air/control functions and essential room services. They must be diode/contactor-isolated or otherwise prevented from backfeeding failed main equipment; the actual scheme remains unresolved. Load priority, selective shedding and restart sequencing require a formal design. Current gameplay only implements one scalar charge account and event costs; it does not model these branches, hotel loads or power faults.

Power harnesses require defined supply/return conductors, protective bonding, connector keying, local lockout and overcurrent protection. Data harnesses need separate trays or approved separation, shield/ground termination rules and replaceable labeled endpoints. No conductor gauge, voltage, current, breaker rating, insulation spacing or protective-ground architecture is specified as fact. The exact gameplay watt values must not be used to size a physical reactor.

## RF, data and control

N01 bridge control reaches the core router and local control switch. Room networks and industrial field controllers are separated; the RF gateway/transceiver connects through an interior antenna bulkhead interface. External antenna geometry, hull shielding, interference, spectrum, protocol, throughput, latency and link budget remain unresolved. The drone telemetry link is a logical interface, not a claim about current radio physics.

Safety shutdown must remain available locally when the router or RF link fails. Commands require authorization and bounded fail-safe state transitions; ordinary telemetry cannot silently become actuator authority. Independent emergency beacon and local controls have essential power paths. A physically diverse second data route is an acceptance requirement, not already proved by the single reserved vertical access core.

## Air and human support

A01 makeup supply, A02 air handler, supply manifold and room branches form a proposed treated circulation loop through filtration/CO2 removal. Airlock gas recovery and dirty workshop extraction stay isolated from normal cabin return. Local dampers, gas detection and manual closure are required at boundaries. Emergency makeup reserve is separate; no crew count, atmosphere composition, pressure, metabolic load or reserve duration is asserted.

The human core is the proposed occupied pressure/comfort volume. Engine and cargo reserves are normally unmanned equipment volumes, not automatically conditioned or safe to enter. Maintenance entry needs shutdown, isolation and atmosphere verification. Cabin pressure boundaries, fire compartments, airlock pumping hardware, refuge and independent escape routes require later geometry and simulation decisions.

## Water and sanitation

Potable storage feeds a pump/check-valve assembly and individually isolated fixtures in proposed galley, hygiene and medical rooms. Graywater returns through treatment and a **quarantine/test buffer**; a quality gate must approve any return to potable storage. Blackwater has separate retained waste plumbing and never shares a potable connection. Service fill/drain needs contamination control, keyed couplings and backflow protection. Tank sizes, treatment process, gravity dependence, flow/pressure and consumption remain unresolved. This is not a claim that an unspecified treatment process produces safe drinking water.

## Cooling and heat rejection

The machinery loop, hotel loop and industrial branches use separated interfaces at heat exchangers. Supply and return paths are explicit; isolation and expansion management are logical functions, not sized components. The radiator node is an **interior heat-rejection interface**, not an invented external radiator location. Fluid selection, heat load, operating temperature, leak detection, pump redundancy, radiator area and deployable mechanisms require engineering. Never join coolant and potable water; a shared diagram sheet does not imply a shared fluid.

## Emergency behavior and acceptance

| Gap | Required acceptance before implementation claims |
|---|---|
| Physical fit and service access | Fit real meshes and equipment envelopes, trays, supports, penetration seals and maintainable valve/breaker access; validate no room/door collision. |
| Electrical sizing and protection | Define complete load cases and fault energy; size supplies, wiring, return/bonding and protection coordination; test main loss and essential-bus isolation. |
| Human-support budgets | Set occupancy, mission duration and atmosphere requirements; validate normal/failed air supply and refuge endurance. |
| Water quality and waste | Close water mass balance, size tanks, isolate contamination and prove safe quarantine/reuse and waste handling. |
| Thermal rejection | Close primary/secondary heat balances and prove loss-of-pump, leak and overheating behavior. |
| Data and RF | Define spectrum/protocol/security; test link loss, unauthorized commands and local independent safe shutdown. |
| Egress and hazards | Demonstrate distinct escape/refuge paths, fire/smoke zoning and airlock isolation; one vertical core is not proof of redundancy. |
| Game implementation | Add authoritative interior coordinates, movement, permissions, circuit/fluid states, repair, persistence and native/browser tests only after scope decisions. |
| Large industrial hulls | Design distributed maintenance and equipment-support networks outside the compact occupied core before claiming complete vessel coverage. |

## Validation performed

`systems-validation.json` records all seven ships. Checks cover exact inherited gameplay values, node-ID and endpoint references, coordinate containment within inherited zones and rooms, deck waypoints, room service branch references, and SVG XML/layout bounds. Representative Pioneer and Monolith sheets were rendered for visual inspection, including the lower register/acceptance region; the remaining sheets share the same deterministic layout with validated text bounds.

These checks do not prove electrical safety, pressure integrity, safe water treatment, thermal adequacy, egress, hull fit, installed clearances, crew endurance or runtime gameplay. No repository code, Git history, wiki content or production state was changed by creating these files.
