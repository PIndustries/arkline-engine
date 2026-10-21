# Pioneer docking and finite cargo-transfer contract — 2026-10-21

Status: repository-backed architecture proposal, not shipped continuous docking or physical cargo transfer. This document changes no world, protocol, geometry or running session.

## Evidence boundaries

**Verified source behavior:** the function paths below were inspected locally. **Proposed game contracts:** operation phases, profile fields, coupling and rate behavior below require implementation and validation. **Parent research awaiting local drawing verification:** the parent supplied NASA's [IDSS IDD Revision G PDF](https://www.nasa.gov/wp-content/uploads/2026/01/m2m-idss-idd-rev-g-clean-1-23-2026.pdf). This audit did not browse that PDF or inspect its drawings. No dimension, load rating, capture envelope, service capability or compliance claim is verified here.

An IDSS reference is a candidate crew-port design basis only after the exact revision and drawings have been checked. Do not arbitrarily resize a referenced interface to fit a ship and still call it IDSS. Separate Arkline industrial cargo interfaces and load/service extensions explicitly from any verified crew-interface profile. Do not copy reference artwork/logos or infer legal clearance from availability of a document.

## Current implementation and gaps

| Source | Verified behavior |
|---|---|
| `apps/server/src/lib.rs`, `Request::Undock` | Immediately changes Docked to Station, assigns `flight::berth`, zeroes velocity/acceleration and selects Stop. There is no release/clearance journey. |
| Same file, `Request::Dock` | Requires normal space and hull-surface docking range; then immediately marks Docked, sets flight position to `STATION_POSITION` and zeroes linear/angular motion. There is no speed/alignment/capture or port-reservation gate. |
| `apps/server/src/flight.rs::berth` | Explicitly describes stable visual separation, not collision avoidance or physical docking. |
| `crates/arkline-protocol/src/fleet_layout.rs::fleet_berth_pose` | Defines class/catalog hangar layout positions. These are not a globally occupied docking-port registry and must not be repurposed as one implicitly. |
| `apps/server/src/docked_traversal.rs::can_undock` | Checks closed boarding hatches and a stationary boarding lift stowed at the ship landing. Preserve these real interlocks. |
| `apps/server/src/resource_jobs.rs`, ReturningToStation/Docking/Depositing | Requests Dock within range, expects immediate Docked, then requests one DepositOre for the entire current hold. |
| `apps/server/src/assets.rs::execute/transfer` | Personal/corporation deposit or withdrawal moves the requested integer quantity atomically. No finite transfer operation or physical coupler exists. |
| `apps/server/src/lib.rs::remove_presence/stop_disconnected` | Only the pilot's last-session disconnect invokes stop behavior. Controlled routines are cancelled; delegated activity with a connected controller has separate handling. No universal offline-continuation policy exists. |
| `apps/server/src/lib.rs::tick_at` | Advances the authoritative world transactionally using server/editor simulation time, then advances resource jobs. This is the proper clock boundary for future finite operations. |

## Authoritative docking operation

Persist one operation with stable operation/vessel/station/port IDs, authority owner, definition revision/hash, reservation lease, phase, timestamps, current target and explicit blocker. Acknowledging a request must not mean capture completed.

Proposed station sequence:

`Requested → Reserved → Approach → Align → Soft capture → Hard capture/Attached → Service ready`

Proposed departure sequence:

`Service isolation → Hatch/lift safe → Release → Clearance manoeuvre → Free flight`

Cancellation or failure must report its actual safe state: free approach can transition to bounded braking/holding; a captured vessel must remain attached until controlled release is safe. A reservation must not be released while its vessel still occupies the capture/clearance region. Repeated command IDs must not start duplicate operations.

Use actual server-owned `FlightState` translation/orientation and bounded motion integration during approach/alignment/clearance. Capture requires the matched profile's relative velocity, angular error, positional tolerance, clearance and load envelope. Validate swept hull/port clearance and occupancy; range alone is insufficient. Do not fake the journey by animating a client camera while authority teleports. A tiny accepted numerical mating tolerance is distinct from a long-distance snap.

During constrained phases reject conflicting manual/warp/hull-switch operations with a real blocker. Preserve existing hatch, boarding-lift, traversal and authorization gates. UI and physical displays must project the same operation and distinguish pending request, accepted operation, capture, attached and service-ready states.

## Port profiles, ship-to-ship docking and services

A versioned profile should describe interface geometry/coordinate convention, mating role, permitted partner profiles, approach corridor, capture/alignment tolerances, structural load envelope, latch/capture capabilities and optional services. Service connections need separate compatibility and state: crew passage, cargo handling, electrical/data or fluid connections are not implied by mechanical contact.

For ship-to-ship docking, bind both stable vessel/port IDs and each body-to-port transform. Compute relative pose and velocity from the same authoritative world frames; define an explicit capture/attachment owner and resulting constraint. Neither vessel may independently teleport or receive a second inconsistent attachment. Inertial/load and manoeuvre limits of the combined assembly require supported simulation definitions, not only matching visible rings.

A verified IDSS-derived crew profile and an Arkline industrial cargo profile are separate contracts. A large cargo port is not a scaled-up crew port. Compatibility requires matched geometry, capture mechanism, load envelope, approach capability and requested services. Unknown/unverified properties must remain unresolved or unavailable, never automatically compatible.

## Finite cargo-transfer operation

The first independently implementable slice is a server-owned finite transfer using the existing Docked service boundary. Label it **legacy docked service** until real coupling geometry/state is implemented; do not claim that a simulated hose or physical loading path already exists.

Persist operation ID, stable source/destination inventory IDs, authority/permission binding, requested/transferred quantity, quantity basis, configured rate, fractional time accumulator, connection identity, phase and blocker. Existing generic ore remains **legacy ore units**. The first rate is units/s, not kg/s or m³/s; physical-unit cargo migration is a separate explicit change.

Each simulation tick may transfer only the minimum of the finite rate budget, remaining request, available source stock and current destination admission. Atomically commit source subtraction, destination addition, operation progress, applicable delivery progress and ledger events in one SQLite transaction. A failed commit must change none of these. Preserve global inventory conservation, including active/inactive hulls, station/corporation storage, asteroid reserves and sell-order escrow.

Before every chunk recheck ownership/permissions, source/destination identity, valid attachment/service connection, isolation state and destination capacity. Loss of coupling or permission stops further transfer with a truthful reason. Completed chunks remain committed; cancellation stops the remainder and does not refund already delivered cargo. Do not reserve or consume the same source quantity twice across simultaneous transfer/trade actions.

Pause advances no transfer. Use simulation time and an explicit bounded-gap/lease policy; process suspension or reconnect must not cause an unlimited catch-up transfer. Do not silently change existing last-session disconnect/mining policy. Decide and implement whether an active transfer is cancelled, held or continued under a durable service owner, then expose that policy. Resume/reconnect must reconcile the saved operation ID and actual committed quantity, not issue a replacement full-hold deposit.

### Resource-job integration hazard

`resource_jobs::record_delivery` currently changes a nonterminal job to Undocking after a deposit event. Calling it unchanged for each finite chunk would depart after the **first chunk**. Separate cumulative delivered-quantity accounting from **haul-transfer completion**. Keep the routine in Depositing while its transfer is active; only completed haul transfer may select the next haul or terminal goal state. Existing target/mined/stored quantities and completed jobs must retain their meanings.

Existing deposit economic events could describe committed legacy-unit chunks without rewriting old history, but their producer, job phase handling and verifier acceptance must be checked together. No ledger rewrite, old-entry rehash or implicit quantity conversion is permitted.

## Save and client compatibility

Existing Docked saves have legacy anchors; they have no reserved physical port or attachment record. Preserve those saves and quantities as legacy docked state until an explicit, copy-validated binding migration exists. Report any required world/body/avatar frame transform before applying it. Do not arbitrarily relocate a saved avatar, reset a world or bypass collision admission.

Add negotiated docking/transfer capabilities and independently versioned definitions. Old clients must not bypass finite mechanics through old instantaneous commands in a migrated world; reject incompatible commands clearly or retain an explicitly separate legacy rules profile. Clients render operation progress from authority snapshots and cannot advance transfer or docking locally.

The physical cargo model also needs versioned containment volume, payload and material properties. The current arbitrary class-unit limit is not a physical rating. Preserve legacy units until a reviewed conversion and historical-verifier strategy is adopted; never relabel 1,000 ore units as kilograms by default.

## Prioritized implementation boundary

1. Add finite legacy-unit transfer authority, conservation checks, cancellation and truthful progress UI, with correct haul-completion handling. No docking geometry or saved-frame migration is needed for this first boundary.
2. Establish shared station port definitions/reservations, world/body/port frames and capability/load/service matching; validate approach/capture geometry and saved-state compatibility before continuous docking is enabled.
3. Add continuous server-owned approach/capture/release and shared presentation, then physical cargo couplers/loading paths using the same attachment and inventory authority.
4. Extend to ship-to-ship docking only with verified relative-frame, assembly-load and two-party authority semantics.

Previously denied cargo geometry/shared-adapter, walking, camera, renderer/light-selection, console and Session-preview edits remain out of scope. This document neither implements nor authorizes a retry of those operations. No build, live interaction, world command, publication or upload is part of this report.
