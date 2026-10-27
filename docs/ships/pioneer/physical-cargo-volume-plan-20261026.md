# Fresh physical cargo volume contract (proposal, not admission)

The current rules 1–3 `ore` quantities are prototype integer units. Pioneer
`cargo_capacity() == 1000`, Mining-drone capacity `100`, extraction `10 / 8 s`,
and manifold `25 / 500 ms` are **not** measured cubic metres. The recorded 40-unit
journeys are conservation/mechanism tests, not physical hold-capacity evidence.
Do not relabel or reinterpret those saved balances or historical receipts.

## Source requirements

A fresh storage manifest must bind the exact evaluated model hash and named
internal cargo chamber(s), with their internal boundaries and explicit excluded
hardware/service spaces. Exterior hull boxes and the docking insertion socket
are not cargo holds. Capacity is the union of disjoint usable internal cells;
overlap must be rejected or subtracted, never counted twice. No unmeasured packing
factor, density, structural payload limit, or invented material mass is implied.
A chamber still at design/proposal stage cannot enable the new runtime.

The geometry owner reports that the existing Mining model has no authored ore
chamber; its 0.075 m³ socket is an insertion void only. A screened candidate
chamber of 0.981504 m³ less an explicit 0.082944 m³ service exclusion leaves
0.89856 m³ nominal design volume. Its new exported candidate is conservatively
rounded inward to an integer-mm cell: 893,833 cm³ (0.893833 m³). This is an
**authored but unregistered candidate**, not the old model's installed capacity.
A separate ship candidate supplies34 disjoint guarded cells; the sum of each
cell's conservative floor is212,623,586 cm³, not the aggregate-floor212,623,600.
These source-candidate figures still require final independent source/receiver/
valve/routing acceptance; neither is a structural mass or operational rating.

## Minimal versioned representation

- Add a fresh physical-cargo manifest/ruleset; preserve rules 1–3 unchanged and
  reject implicit migration. An old balance has no defensible volume conversion.
- Use checked integer SI volume (proposed resolution: 1 cm³; 1 m³ = 1,000,000 cm³).
  Geometric usable capacity is rounded down at the declared resolution; consumed
  or reserved item volume is rounded up if a rational item definition requires it.
- Generic raw ore may be represented directly as bulk volume, not fictitious
  count units. If commodities are counted, each commodity must have explicit
  versioned volume per item. Mass/density may remain `unknown`; do not substitute
  zero mass or describe an unknown structural limit as a verified payload rating.
- Pin source/hold/commodity/rate-manifest identity in fresh ledger genesis.
  Extraction and transfer rates are explicit authored volume/time gameplay
  settings, not derived from historic unit counts or claimed physical pump tests.
- Represent the ship inventory, each immutable drone payload, pending extraction
  claim, destruction sink, and each transfer receipt in the same volume dimension.
  Reserve ship incoming volume as `sum(drone payload + active extraction claim)`.
  A transfer moves existing volume between owners and must not add a second
  reservation for the same payload. Turret extraction, manual loading, purchases,
  and service arrivals share checked headroom before any mutation.
- Track an independent mass constraint only when item mass and a source/engineering
  mass rating exist. Volume alone must not be advertised as validating thrust,
  stability, or hull strength. No fresh flight model may silently use volume as kg.
- Public data must distinguish `LegacyPrototypeUnits` from `PhysicalVolume`; fresh
  UI shows used/reserved/usable m³ and explicit unknown mass rather than the old
  integer ore bar with a new suffix. Unsupported mixed ledgers fail closed.

## Implementation seams (current source)

1. Protocol: `lib.rs::cargo_capacity`, `drone_cargo.rs::DroneOreHold` and constants;
   `carrier_inventory/cargo.rs` aggregate headroom, services and shape validation;
   `carrier_inventory/presentation.rs` public validation. Add a new representation,
   do not globally modify old constants or relax old validators.
2. Authority: `carrier_inventory/world/{cargo,service,history,persistence}.rs`;
   `mining.rs`, `drone_cargo::reserved_for_account`; all trade/unload/storage paths
   and `ship_acceleration` ore-dependent assumptions need explicit ruleset dispatch.
3. Replay: `apps/verifier/src/carrier.rs` genesis capacity and all balance/claim/
   receipt checks, plus general ship economy replay. Preserve old fixtures exactly.
4. Factory: opt-in fresh source only after usable-volume and source geometry proof;
   no unsupported-save writes or covert replacement of existing prototype saves.

## Required acceptance

Exact-fit and one-quantum-overflow; overlapping chamber rejection; mixed commodity
rounding; ten simultaneous service requests sharing one vessel budget; turret +
drone claim + incoming payload atomic headroom; partial transfer hold/restart;
loaded Recall All; destruction sink once; same-ID conservation; hash-valid malformed
manifest/owner/volume no-write reopen refusal; old rules 1–3 byte/replay regressions.
Until these gates pass, the current demonstration remains explicitly prototype-unit
cargo, even though its physical motion and ledger conservation are independently
verified.

## Explicit authored catalog (isolated foundation)

`physical_cargo::catalog` separates source measurement from gameplay selection:

- A named material explicitly uses `BulkVolumeCm3`; mass stays `Unknown` unless
  an authored mass assumption is supplied. No density is inferred from geometry.
- Named/revisioned ratings declare three distinct volume/time batches: drone
  extraction, ship-module extraction, and the vessel-global manifold. Explicit
  energy units per extraction batch are gameplay values, not measured joules.
  There are no default or registered live ratings.
- Optional rational credits per cm³ are an authored price. `None` means trading
  unavailable; it is not free material and does not fall back to old unit prices.
  Buy quotes round up and sell quotes down with checked integer arithmetic.
- Fresh genesis must pin BOTH the catalog SHA (rates, energy and optional price)
  and manifest SHA (exact ship/drone source, cells, material and selected rates).
  A plain shape-valid manifest is not an installed source certificate.
- Assembly1704 is reserved for the fresh physical-cargo ship; Mining2074 is a
  separate exact model/profile. Old1703 prototype recordings are immutable.

Fresh service must visibly and durably follow contact/latch → inlet-valve open →
transfer at the reached-open barrier → valve closed → unlatch. Both source-owned
split panels use a distinct per-drone device pose domain, never hull mechanism
roots or receipt-implied opening. Stroke timing is a declared animation/design
input; it is not a measured pump, force, pressure or actuator-performance claim.
