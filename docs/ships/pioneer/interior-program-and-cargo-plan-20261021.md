# Pioneer interior program and visible cargo plan

Status: **compatible design proposal; cargo mesh implementation blocked before execution**. This document is not evidence of a shipped cargo hold. The current source geometry and runtime room document remain authoritative. No cargo mesh, collision, registration or world changes were applied.

## One ship, one circulation model

Retain both real 32 × 14 m interior cores, their 2.4 m central spine, 1.4 m room doorways, 3.2 m clear room height, existing access core/deck connections, fitted upper boarding vestibule and bow bridge connector. Shared body-space parts must supply seated, walking, docked, space and exterior cutaways, with only rigid view transforms varying. Keep the rear chair route and forward bridge sightline open. No view-specific replacement rooms.

| Existing source room | Current program | Coherent first-pass purpose |
|---|---|---|
| D1-P1 | AIRLOCK / EVA | Retain existing authoritative room equipment/interlocks; reconcile signage with the actual upper boarding route in an explicit future layout revision. |
| D1-S1 | POWER CONTROL | Contained electrical cabinets and service access; never invent live power systems. |
| D1-P2 | DRONE WORKSHOP | Service/storage presentation over existing drone authority, without invented drone cargo inventories. |
| D1-S2 | CARGO ACCESS | First visible contained ore-utilization inspection bin; doorway/spine remain clear. |
| D1-P3 | STORES / SERVICE | Secured stores and maintained access. |
| D1-S3 | WORKSHOP | Existing workbench/service envelope, not loading spillover. |
| D2-P1 | BRIDGE / NAV | Existing source reservation is aft of the actual bow bridge. Preserve saved room identity; future program decision required rather than silent relocation. |
| D2-S1 | CREW / GALLEY | Existing crew support program. |
| D2-P2 | SENSORS / COMMS | Existing instruments; no fabricated capabilities. |
| D2-S2 | MEDICAL / REST | Existing rest/crew service program. |
| D2-P3 | OPERATIONS | Operational management distinct from flight sightline. |
| D2-S3 | LIFE SUPPORT | Existing utility/service program. |

The v05 illustration is conceptual. Its room placement is not fitted first-person or collision evidence. Actual upper-port boarding and bow bridge placement take precedence over visually relabeling the old AIRLOCK and BRIDGE source rooms. A larger industrial hold/loading route requires explicit room-program and compatibility work.

## First compatible cargo slice

Cargo Access (room4) is 8.9333 × 5.4 × 3.2 m at deck-local origin [13.5333, 8.4]. Its existing solid Storage envelope is min[14.5333,0,9.4], max[16.3333,1.4,10.2], or body-space min[2.5333,-4.12,2.4], max[4.3333,-2.72,3.2]. Replace only that visual cabinet with a framed inspection cassette, keeping every mesh vertex within its existing collision envelope. Preserve door, circulation and service clearance. No loose ore piles.

The game stores integer ore units. Pioneer capacity is 1000 units; no authoritative kg/m³ or physical per-unit volume exists. Use current confirmed PilotView.ore / cargo_capacity(ship_class), clamp only presentation at the capacity, and label **Representative hold utilization · ore / capacity units**. Empty means zero current ore, FULL only at actual capacity. A completed job's stored_units is delivered station inventory and must not fill the ship. Drones credit the owning ship directly and do not have independent cargo inventories.

Static frame/window geometry can share the existing room recipe. Dynamic fill must stay outside SceneCache/LayoutPartsCache and use the same per-frame helper in registered walking and seated/external adapters. Omit the bin when the live room/storage is removed or no matching confirmed cargo source exists. The legacy unregistered walking adapter remains separately blocked; do not hide missing coverage or edit that path under this feature name.

## Validation and local wiki images

Required before delivery: empty/partial/full from synthetic authoritative snapshots; all mesh bounds inside the existing obstacle; no fill in deleted/mismatched rooms; same load/recipe across registered views; exact source compile; saved binding and semantic avatar/job/ledger unchanged. Keep original authored assets and live world backups.

Capture implemented geometry through the existing native engine fixture paths and label images **native offscreen authority fixture**, not live user-world acceptance. Preserve a separate live-window acceptance note. Conceptual images, if made, must say conceptual. No new cargo images exist at this checkpoint because the source edit was rejected. Save future images under docs/ships/pioneer/images with accompanying source/checkpoint metadata; do not upload/publish.

## Source evidence

- crates/arkline-protocol/src/pioneer_interior.rs and room_layout.rs: current room program, storage obstacles and clearances.
- crates/arkline-protocol/src/pioneer_docked.rs: body/deck geometry and shared supports/portals.
- crates/arkline-protocol/src/lib.rs cargo_capacity; apps/server/src/mining.rs and drones.rs: integer cargo and delivery model.
- apps/client/src/pioneer_interior.rs and pioneer_docked_scene.rs: shared static recipes/caches; flight3d.rs: seated/external adapter.
- docs/ships/pioneer/images/v05-interior-review.jpg and pioneer-flagship-reference-assessment-20261020.md: reference interpretation versus current fit.

Automatic approval review rejected the proposed multi-file cargo source write because the worker received delegation rather than recognized direct feature authorization. Nothing executed; no alternate route was used. Requested direct approval: Implement the Pioneer visible cargo hold from authoritative ore/capacity, including the shared scene adapters.
