# Physical mining-drone ore drop-off contract

Status: approved fresh-preview defaults; implementation staged after this contract.
Frozen predecessor: d9fd266c18a4c1bebff9834d8468655de347042f. Its source, world
behavior and 630.2-second demonstration remain preserved. No existing save is
migrated, no public tracking/publication is authorized, and no unvalidated
receiver coordinates are accepted by authority.

## What exists at the frozen checkpoint

- `crates/arkline-protocol/src/lib.rs::DroneView` has identity, class, health,
  signal, motion and mining-cycle timestamps, **no ore or cargo capacity**.
- `apps/server/src/drones.rs::advance` completes a mining cycle by subtracting
  ore from the asteroid and global belt, adding it directly to `PilotView.ore`,
  and emitting `EconomicEvent::OreMined { pilot, units }`.
- Existing extraction is 10 generic ore units per 8-second cycle. These are
  not kilograms, and two visual cutters do not mean two economic yields.
- `cargo_capacity(Pioneer)` is 1,000 units. `PilotView.ore` is the active hull;
  `AssetsView.ships` lists inactive hull cargo; personal/corporation storage is
  per-system generic ore. There is no differentiated ore-item inventory.
- `drone_bay::capture` preserves ID/HP and moves Active to Stored only after
  cradle security. It does not transfer ore. `runtime::approach_points` targets
  the existing storage throat, not a quick-service coupler.
- Station `cargo_unloading::advance` is separate: confirmed Secured docking,
  no deployed drones, 25 units per 500ms, exact receipts bound to actual appended
  deposit ledger entries. It must not be relabelled as drone drop-off.
- `resource_jobs::plan` currently derives collected ore from ship cargo plus
  station deliveries, stops miners, fully recovers them, then docks/unloads.
- The verifier's `OreMined` rule debits belt and credits the pilot hold. Neither
  that event nor personal/corporation storage events can truthfully represent
  extraction into a drone or transfer from drone to ship.

## Approved new defaults and bounds

1. Only a fresh explicitly opted-in preview gets real drone cargo.
2. Each original Mining drone owns at most **100 generic ore units**. Recon and
   Combat drones have no ore hold. IDs/HP/home cradles remain unchanged.
3. Extraction remains **10 units / 8 seconds**, bounded by actual asteroid ore,
   drone free space and reserved mothership headroom. Existing energy policy is
   unchanged; this milestone invents no additional machinery power budget.
4. Confirmed coupled transfer commits at most **25 units every 500ms**. A full
   100-unit payload therefore needs 2 seconds of transfer; that excludes queue,
   approach, alignment, coupling, disconnect and egress. No total-trip speed is
   promised, and storage launch/recovery rates are not shortened.
5. One receiver serves one drone at a time. First implementation conservatively
   serializes receiver handling against full storage-bay handling; separate
   physical hardware is not evidence that concurrent swept paths are safe.
6. Quick drop-off never reports Stored. Full Recall All unloads before stowing;
   a loaded drone cannot silently disappear or move ore at cradle capture.

## Single authoritative cargo ownership and headroom

Persist cargo once per actual owned vessel and drone ID, separately from the
rendered Active body. Stored/active projections reference it; they are not
independently writable balances. Fresh initialization records the original
finite roster once, with zero ore. Missing initialization is corruption, never
permission to refill or recreate a balance.

The invariant for each mothership is:

`ship hold ore + drone-owned ore + accepted drone-cycle claims <= hull capacity`

Drone-cycle claims reserve possible incoming headroom when a real extraction
cycle starts; they are **not ore**, and never enter the global ore total. A cycle
completion atomically turns only its actual yield into drone-owned ore. Target
exhaustion, power failure, cancellation, loss of lock/signal, disconnect and
restart release unused cycle claims. Existing turret mining, withdrawals, market
purchases and every other incoming-hold mutation use the same available-headroom
calculation. They cannot spend space already promised to a loaded drone.

This avoids the otherwise real deadlock: a full ship cannot accept loaded miners,
while docking/station unloading requires the deployed miners to be recovered.
It also preserves the 1,000-unit combined carrying budget instead of silently
adding another 500 units of fleet capacity.

Public labels distinguish ship `used`, physical `free`, `incoming reserved`, and
`available for new ore`. Example: ship300, drone payload150, no cycle claims means
700 physically free but550 available. No unit is counted twice.

## Economic ledger and atomicity

Add explicit versioned events, rather than reinterpreting existing `OreMined`:

- finite drone-cargo initialization/ownership declaration (zero ore);
- extraction into a specific owned drone, with vessel, drone, cycle identity,
  target identity, units and optional job link;
- transfer from that drone to its immutable owning vessel, linked to drop-off
  operation and batch sequence;
- irreversible destroyed-drone ore loss into an auditable sink.

Ruleset selection is per persisted world feature, not a global constant bump:
unchanged worlds append only ruleset1 entries with unchanged meanings; fresh cargo
worlds append ruleset2 entries from genesis. The verifier has explicit version/event
combinations, so old exports remain independently verifiable and old readers fail
closed. It stays independent of server code and proves identity/capacity,
extraction amount bounds, transfer source balance and minimum receipt spacing,
unique cycle/operation-batch identities, snapshot balances and conservation.
The economic ledger alone does not prove physical contact, collision clearance,
claim admission timing or uninterrupted mining duration. Authority transaction,
clock and source-bound route tests prove those separate properties.

New total:

`belt + ship holds + drone holds + station/corporation storage + sell orders + destroyed-ore sink = initial ore`

Destruction never credits the mothership. Remaining payload becomes a durable
lost-ore receipt exactly once, and its headroom claim is released once. No free
replacement, recovery salvage or new commodity policy is implied.

All extraction, transfer/loss, reservations, job counters, receipts and world
persistence use the existing IMMEDIATE SQLite transaction. Bind receipts after
actual ledger append assigns sequence/hash, not a predicted sequence. Incoming
combat resolves before due extraction, transfer or capture: a zero-HP drone cannot deliver
ore or be resurrected. Faults after ledger append but before world update/commit
roll everything back. Confirmed earlier transfer batches remain committed.

## Physical operation and interruption semantics

The same drone follows `Queued -> Approaching -> Aligning -> Coupling ->
Transferring -> Decoupling -> Egress -> Complete`. Position, full attitude and
receiver machinery derive from one accepted source-bound sampler. Parent remains
stationary from accepted approach through confirmed safe egress; peer/obstacle checks include full envelopes and
same-tick reach. STOP and inspection remain available. Incoming targetability is
unchanged; constrained mining/attacks are disabled.

Commands use existing-session authorization and monotonic retained operation IDs,
not new credentials. Identical retry returns retained state before current
eligibility checks; changed intent under a used ID rejects. Queue membership is
immutable. Late join receives accepted pose, actual payload and confirmed batches.

Hold/disconnect/restart freezes last accepted drone/machinery poses and stops
transfer; there is no offline catch-up. Resume revalidates owner, source,
geometry, headroom, obstruction and the retained operation. Repeated Resume does
not reset deadlines or accelerate batches. Cancellation preserves committed ore
and remaining drone payload, safely disconnects/clears hardware, and never snaps.
A terminal logical cancellation/destruction cannot discard occupied hardware;
retain explicit resumable cleanup until the receiver is safe.

After ordinary quick drop-off, resume the same mining target only if still valid,
in range and owned by the same persisted manual/job intent generation. Every
replacement or cancellation invalidates that generation; target equality alone
is not permission to revive an old command. A cancelled/replaced job
must not be restarted. Otherwise leave the same empty drone safely Holding.

## Recall All and jobs

Loaded Recall All members first use the receiver, then the existing full storage
route. The batch remains active through this prefix and any held cleanup. Only
actual final cradle capture makes the member Stored. Cancel Remaining must not
start another pending member and must preserve an actionable current service.
Unloading happens before stow, never invisibly in `capture`. Each immutable
member retains typed `service_operation_ids` separately from its existing storage
`operation_id`; the batch has `current_service_operation_id` separately from
`current_operation_id`. A Servicing member is not Stored. Receiver cleanup keeps
its original service identity and actionable Resume; it cannot be hidden by a
terminal storage result or overwritten by a later storage operation.

Manual cargo/queue commands may not silently take over an active resource job.
Resource jobs distinguish historical extracted units, currently available
job cargo and committed station delivery. A payload loss reduces available goal
progress without undoing historical extraction or prematurely finishing the job.
They account for drone as well as ship ore without counting a transfer as new
extraction. Delivery counters still advance
only on committed station deposits. Goal/return planning must drain payloads,
fully recover bodies, dock and use the existing timed station unload. Initial
cargo baselines and per-job extraction identities prevent earlier cargo from
being counted as newly mined. Cancellation retains all ore and stops new claims.

Old schema1/2 stock contains no cargo fields and remains exactly that on old
worlds. Do not infer a loaded legacy drone from its mode or mine history. In a
new cargo world, a Stored drone with nonzero ore is an invalid unloaded-first
state: reject before mutation, do not silently credit or discard it.

## Compatibility and geometry ownership

Keep main hull source/registration12 and old bay layout unchanged. The new
receiver is a separately authored, independently hashed asset. Proposed
`DroneCargoRegistration` pins base `DroneBayRegistration`, receiver revision and
receiver source hash; its binding additionally pins the real vessel identity.
Geometry owner supplies exact gate/coupled poses, named mechanism roots, contact
and swept working envelopes. No placeholder route becomes supported.

Fresh-only world stamp plus client feature capability5 gates creation/admission.
An explicit new isolated cargo-preview profile must not reopen and rewrite the
existing `physical-flagship-v1` file. Old worlds retain absent cargo state and
individual/batch behavior. A serializer-preservation guard rejects older writers
that would discard the new stamp, including an empty fresh world. Immutable
DockingV1 layout schema2 and existing security/role policy remain unchanged.

## Acceptance and handoff stages

1. Protocol/state invariants, supported registration interface and independent
   verifier fixtures, including mixed old/new client/world/version cases.
2. Real extraction-to-drone and headroom tests: five miners plus turrets, small
   asteroid/final partial yield, full hold, overflow, interrupted cycle, no
   double credit and no fabricated payload.
3. Actual coupled batch transactions: retry/conflict, held/resume/cancel, source
   ownership/remote view, SQL-trigger rollback before/after ledger append,
   postcommit lost response, restart and destruction at transfer boundary.
4. Authored geometry clearance and continuous same-ID approach/contact/egress;
   no source acceptance before collision/art review. Storage routes unchanged.
5. Recall All with loaded miners, Cancel Remaining during service, full-budget
   no-deadlock, resource-job extraction/drop-off/full-stow/station-unload journey.
6. Exact-head workspace/strict/fmt plus actual100ms Responses and native normal-
   speed footage showing loaded approach, confirmed ore movement, disconnect,
   resumed mining and separately full recall. Durable source/evidence backup.

Ownership: authority worker owns cargo protocol/server/verifier and tests;
integration lead owns client/monitor and final native evidence; source-adoption
worker owns receiver asset and pure geometry adapter. No shared-target build
without the coordinator's explicit serial grant. Integration owns additive
receiver collision/depth/ray/camera-obstruction participation; rendering the new
asset alone is not physical acceptance.
