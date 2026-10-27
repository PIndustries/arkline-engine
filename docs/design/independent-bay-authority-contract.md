# Independent physical drone bays — authority contract

Status: authorized implementation, initially unregistered. Baseline is the clean
`fe1878abc5d392ce94f0215d641ccb13af297d48` checkpoint. This document is a contract
and acceptance plan, not evidence of working geometry or completed tests.

## Scope and compatibility

- One fully source-bound launch / return-to-unload / return-to-storage lane first;
  then the identical executor across fifteen independently reserved physical bays.
- Exactly fifteen original identities (five of each class), initialized once in an
  explicitly new preview. IDs, class, health and preferred home survive every cycle.
  Destroyed identities remain destroyed. No free replacements or capacity increase.
- A new immutable feature stamp binds the actual host and assembly source hashes.
  Capability 6 is required before session mutation in that new world. Existing
  registration 12, single-gantry stock/batches, ore receiver, layouts and saves keep
  their existing behavior. Never retrofit a saved world or substitute a new source.
- New assembly bounds must resolve from its registration for collision, berth,
  hangar and camera consumers. Do not change global ShipClass bounds for old ships.
- No new warp attachment, moving-carrier handling, invented energy/strength rating,
  player customization or combat-ready damaged-component claim in this slice.

## Ownership and physical states

The existing account's finite stock is the sole canonical ID/health/class roster.
New lane state references it; it is not another inventory. Preferred home is
immutable identity metadata. Actual captured berth and current physical operation
reservation are separate. Launch frees physical berth occupancy only through the
accepted same-body route; no duplicate stored/active rendering is allowed.

Each bay owns its door, support/cradle, clamp and ore mount independently. One
accepted physical operation or its empty-hardware cleanup owns that bay at a time.
Global resource ownership remains explicit where hardware is actually shared:
external approach conflict reservations and the vessel's ore manifold.

A return chooses its preferred home if healthy, compatible, empty and reachable;
otherwise it may atomically reserve another compatible empty, unreserved bay.
It never evicts stored stock. Two returns competing for one spare get one winner;
the other remains safely outside. A deployed drone's preferred home is not an
exclusive reservation in this new mode. No eligible bay means a truthful hold.
Changing an accepted destination requires safe release of the old physical route
and a fully checked replacement path; changing a bay number is not a teleport.

Stored means the same live entity is aligned, actually supported and clamped at
an accepted berth, required hardware is safe, and capture commits atomically.
A timer, model proximity or client interpolation never establishes Stored.
Return-to-unload instead couples at the service pose, transfers confirmed generic
ore, disengages safely and may resume the same still-valid mining intent. It must
not be labeled Stored. Loaded full recall unloads first, then safely stows.

## Geometry provider and motion authority

The client/source owner supplies `independent_bay_geometry.rs`: exact-registration
lookup, fifteen stable IDs and class compatibility, preferred-home/support/contact/
gate/clear poses, named mechanism roots, body and hardware swept envelopes, and
ordered source-bound keyframes. Semantic milestones identify door-clear, support,
clamp-secured, transfer-coupled, exterior-clear and hardware-safe states. No current
17-segment index convention is inherited. Unaccepted coordinates remain unsupported.

The authority owns time, sampled accepted motion, admission, holds and finalization.
All group roots are resolved body-space poses; renderers apply reference inverse
once. Full parent orientation is retained. Drone and hardware segments share an
accepted clock. Held state exposes confirmed poses without advancing segments.

The carrier remains stopped from accepted approach until all associated egress or
cleanup is safe. STOP and neutral input remain legal. No offline catch-up occurs
on reconnect/restart or an excessive simulation gap. Resumption validates actual
parent, source, bay ownership, all future route frames and dynamic clearance.

For parallelism, precompute candidate full-body and moving-group sweeps against
one tick's starting state and existing route reservations. Use each bay's real
geometry, not the legacy shared working box or an arbitrarily reduced sphere.
Nonintersecting lanes may advance together; intersecting reservations have stable
ordering. Free peers and other carriers remain obstacles, including same-tick
movement, docking traffic and other constrained operations. Do not ignore own drones.

## Cargo, manifold and atomicity

The user's approved initial policy preserves **vessel-global 25 ore units per
500 ms**, not 25 per bay. Drone capacity stays 100 generic units; extraction stays
10 units per 8 seconds. Units are not kilograms. There is no rate-only ruleset3
change. Physically coupled bays can wait for the shared manifold.

A fair finite grant owns one uninterrupted payload attempt (at most 100 units,
four confirmed batches). The pump cooldown persists across bay/drone handoff.
Held, dead or cancelled work cannot indefinitely own the manifold. Its committed
receipts remain; remaining ore remains with the drone. A later grant uses a new
monotonic economic service-attempt ID linked to the same physical operation.
This preserves existing v2's rule that an older transfer ID cannot reopen after a
newer one. An interruption does not replay committed ore or change the original
physical command identity. The UI shows physical progress separately from actual
manifold ownership and confirmed transferred units.

All incoming payload/cycle claims share the ship hold's checked headroom with
other miners, ship extraction, withdrawals and economic actions. Existing cargo
ledger events retain their economics and exactly-once sequence/hash receipts.
Any additive identity field must have an explicit compatibility test; do not
weaken legacy verifier rules. The economic verifier proves ledger ownership,
rates and conservation, not physical contact by itself.

Both request-driven and tick-driven advancement occur inside the existing
IMMEDIATE SQLite transaction. Persist world/stock, lane ownership, cargo balances,
job counters, operation/batch receipts and ledger entries together. Destruction
resolves before due extraction, transfer and capture; lost payload has the
existing auditable sink. Empty machinery cleanup retains its resource reservation
without reviving the destroyed identity.

## Commands, retries, jobs and cancellation

Requests use existing authenticated sessions and the actual controlled vessel;
no new credentials or security policy. Per-vessel monotonic command and batch IDs
retain original payloads and terminal results. Exact retries resolve before
current eligibility checks, generic manual-job cancellation or new batch admission.
A reused ID with different payload rejects. UI private counters never apply to a
foreign viewed vessel. No receipt eviction/replay-history compaction is authorized.

Deploy-class and recall-all retain immutable accepted member rosters. Each member
has typed physical and economic service links; multiple independently owned members
may run, but no member may have two pumps or two body routes. Cancel Remaining
cancels only not-yet-started members and keeps accepted routes/cleanup actionable.
Hold/Resume/Cancel target their owned operation only, not unrelated free drones.
A job does not silently take over manual handling. Restart and last-owner disconnect
hold active routes; repeated resume does not reset deadlines or accelerate transfer.
Post-service mining resumes only when the persisted intent generation, target and
job ownership still match; cancelled/replaced intent is never resurrected.

## Required staged acceptance

1. Exact old-source restore and unchanged legacy fixtures; old/new clients and
   old/new worlds; feature downgrade guard and no migration/reseeding.
2. Protocol/private/public ownership validation: duplicate/orphan IDs, bays,
   resource owners, malformed future routes, wrong source/parent/clock, invalid
   partial journal, hidden old/new state and foreign private counter use reject.
3. One real authored bay: continuous launch, mining, confirmed unload, resumed
   mining, loaded return and final secure capture of the same ID/health.
4. Restart/disconnect/hold/cancel/repeated retry at every semantic boundary,
   including destruction during cancellation, door crossing, transfer and capture.
5. Two then fifteen lanes: reverse iteration order; disjoint simultaneous progress;
   conflicting routes deterministic; intruding peers safely hold; compatible
   fallback contention; no spare and injected component-fault qualification.
6. Shared manifold: full capacity, concurrent claims, fair grants, no starvation,
   cooldown across handoffs/restart, partial attempt interruption, due-tick death,
   SQL-trigger rollback and post-commit lost-response retry. Independent verifier
   and global ore conservation must agree with every committed receipt.
7. Normal-speed actual Response exports and native before/after views. Show bay
   support/door/clamp/contact and honest queue/ore state. Do not claim artwork-only
   renders or test-injected faults establish production physical behavior.
8. Exact integrated workspace/check/strict/fmt gates, source-qualified evidence and
   offline-restorable private Library checkpoint. No runtime publication or public
   tracking is authorized by this milestone.
