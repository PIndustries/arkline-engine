# Proposed physical cargo contract — not activated

The current game conserves abstract integer ore units. No source establishes kilograms or cubic metres per unit. The current Pioneer 1,000-unit cap is legacy gameplay, not a physical rating. No quantities, rules, ledger entries, geometry, migration or runtime capacity were changed by this design document.

## Authoritative physical model

A versioned CargoBaySpec should bind the exact canonical containment geometry revision, usable internal volume, authored structural payload rating, per-compartment limits, allowed materials/containers and loading/access constraints. Gross room volume excludes none of the structure, machinery, access lanes, loading/handling space or container walls, so it cannot be used as capacity. D1-S2 is about 154 m³ gross; the existing cabinet is only about 2.016 m³ externally and cannot establish whole-ship capacity. Neither figure proves structural payload.

For separately restrained cargo lots, compute loaded mass M=sum(m_i) and occupied volume V=sum(m_i/rho_bulk_i). Admit an additional mass only up to min(remaining_payload, rho_bulk_i*remaining_volume), with compartment/placement/centre-of-mass limits checked too. Summed lot volume is an explicit design assumption for separated lots, not a particle-mixture model. One authoritative calculation must supply mining admission, drone yield, withdrawal, trading/escrow restoration, UI remaining mass/volume/full reason and rendered occupied containment. Rendering must not invent an independent load.

Material profiles require identity, quantity basis, density basis and source/version. Distinguish grain/solid density, particle internal porosity and stored bulk packing. When intentionally modeled, bulk_density=solid_density*(1-internal_porosity)*packing_fraction. A supplied measured bulk density already includes packing: do not apply it twice. NASA distinguishes these density/void relationships in [TM20240003513 section4.1.3](https://ntrs.nasa.gov/api/citations/20240003513/downloads/TM-20240003513.pdf).

Illustrations, not game defaults: NASA notes undisturbed regolith can approach 1,920 kg/m³ versus constituent density around 3,100 kg/m³, and excavation swells its volume; stored material therefore differs from in-situ material ([NASA geotechnical guide](https://ntrs.nasa.gov/api/citations/20220014634/downloads/Final%20IEEE%20paper%20formatted%20footnote%20added.pdf)). The published 2015 IMO iron-ore schedule spans 1,250–3,500 kg/m³ bulk density, illustrating condition/material variability rather than choosing a generic asteroid-ore density ([IMO MSC.393(95), p17](https://wwwcdn.imo.org/localresources/en/KnowledgeCentre/IndexofIMOResolutions/MSCResolutions/MSC.393(95).pdf)).

Until structural data exist, payload ratings, compartment load limits, container dimensions and allowed mass distribution are declared ship engineering/design parameters, not measured facts. Geometry alone cannot derive them. NASA spacecraft-load guidance considers configuration mass properties/centre of gravity and consistency with actual geometry ([NASA-STD-5002B section4.9.3.1](https://standards.nasa.gov/system/files/tmp/Final%20Approved%20NASA-STD-5002_REV%20B%20-%2003-10-2026.pdf)).

Use explicitly named fixed-point mass/volume quantities with checked wide intermediates and specified rounding. UI should show tonnes or kilograms, usable/remaining m³, payload remaining, material profile and the actual binding constraint. Unknown legacy mass/volume is Unrated/Unknown, not zero, empty or physically full. Never present invented precision.

## Unit and migration boundary

Changing cargo_capacity() alone breaks saved-world validation, inactive hull checks and historical ledger replay. Existing verifier demands the current RULESET_VERSION and applies current capacity to historical events; merely bumping it would reject the existing 1,587-entry Pioneer history. Preserve all hashes/entries, not rewriting or rehashing the ledger.

Safest staging is an opt-in copy-validated versioned transition. Retain v1 rule dispatch for every historical event, append a rules-activation/migration manifest at a bound checkpoint, and apply physical admission only after that boundary. Preserve each original quantity and its legacy basis, account/escrow ownership, vessel, delivery goal/progress and station inventory. A legacy kg/unit/material interpretation needs explicit declared design approval; it cannot be inferred from a mineral-density website. Existing 10,000-unit goals/deliveries must not silently become kilograms.

If existing cargo exceeds a new physical rating, preserve it as explicit legacy overload, reject additional admission, and allow unloading; never truncate it or make the world unloadable. Unknown cargo properties require an honest compatibility state until resolved. Old clients require capability/version gating for physical commands. The migration must be rehearsed on a verified copy with conservation, load/save and old/new verifier checks before touching a real world.

## Affected source paths

- protocol/lib.rs cargo_capacity and ship_acceleration: integer limit/normalized load ratio; physical thrust acceleration also needs rated hull/equipment mass.
- server/mining.rs and drones.rs: bounded yield goes directly into owner hold; no independent drone cargo exists.
- server/assets.rs: inactive ships, boarding, withdrawals and transfers; deposits must remain possible when overloaded.
- server/lib.rs trading and validate_world: escrow restoration and conserved quantities.
- server/resource_jobs.rs: haul completion/goals preserve their original basis.
- verifier/lib.rs: historical ruleset dispatch and capacity validation.
- client services/resource_jobs_ui/ship_ai/exchange_readiness/target_controls/flight_workspace/sound_scene: shared authoritative cargo status for controls, estimates, full cues and presentation.

## Dependency

Cargo geometry/shared-adapter source edits were rejected before execution and remain unapplied. This newer physical requirement does not authorize retrying them. The documented 2.016 m³ cabinet is a proposed containment component, not an implemented ship-capacity model. No cargo images are labeled as shipped geometry.
