# Stabilization audit — 26.10.10.22

This is a bounded source review and regression run, not public-release or native
visual acceptance. The follow-up fixes below change resource-job bookkeeping and loss handling. Final integration evidence is recorded in `validation.md`.

## Verified behavior

The following server suites passed locally on 2026-10-10:

- `mining_modules`: 7 tests. Independent repeating modules, exact completion,
  capacity/depletion clipping, range invalidation, restart cancellation, and a
  rejected deadline command rolling back before the next tick awards once.
- `remote_control`: 4 tests. Authentication/private assets remain the controller's;
  commands are pinned to the leased worker; switching rejects old-worker input;
  revocation, restart and final controller disconnect release control. An offline
  worker may remain controlled while its controller has a live connection.
- `resource_jobs`: 7 tests. Actual travel, locks, turrets, drones, storage and ledger
  verification; permissions/manual takeover/disconnect/restart; depleted resources;
  parallel delegated workers; existing cargo accounting; a 4,000-unit goal across
  multiple trips with cumulative delivered progress.

Source boundaries reviewed: `apps/server/src/{lib,resource_jobs,remote_control,
mining}.rs`, client transport and command routing in `main.rs`, the generation-based
input mailbox in `pilot_input.rs`, and drone camera freshness/cache gates in
`flight3d.rs`. Remote wrappers contain an explicit worker name, so queued input
cannot silently retarget a different lease. Recovery discards uncertain actions
rather than replaying them. These checks do not prove network security at public
scale or operator honesty.

## Findings and limitations

1. **Fixed: job delivery and progress commit together.** The audit found that
   `resource_jobs::advance` saved `stored_units` after the ordinary `DepositOre`
   transaction. `resource_jobs::record_delivery` now runs inside that transaction,
   before the same world/ledger commit. A SQLite trigger injecting a failure on
   nonzero delivery progress proves ore, ledger and progress all roll back; a
   successful deposit/reopen test proves they persist together. Active jobs still
   cancel on restart; resumable automation is not promised.
2. **Fixed: drone loss has an explicit outcome.** A mining job whose current fleet
   is smaller than its successful launch count now blocks with a lost-drone
   inspect/restart message, stops mining modules and recalls survivors. The
   regression removes one drone from an isolated fixture and checks this state.
   This is controlled failure, not automatic fleet repair or replacement.
3. **Timeouts and capacity are intentional limits.** Jobs have a five-minute
   no-progress timeout and two-hour total timeout. Large permitted targets are not
   a guarantee of completion before the time limit. Resource exhaustion can
   complete a partial haul; inspect status message and stored versus target units.
4. **Camera validation is separate from renderer throughput.** Cached drone views
   have bounded resolution, four-feed maximum, freshness checks and signal gates.
   Unit/native context tests cannot establish smooth presentation on the installed
   app. Test one and four feeds while mining, switching modes/IDs, losing signal,
   revoking worker access, disconnecting, and changing window size.

No new high-impact ownership or ore-duplication failure was reproduced in the
server checks above. The newly found recovery limitations were fixed as described. This is not a claim that all paths are defect-free.

## Station capability inventory

This inventory is vendor-neutral. It distinguishes existing protocol behavior from
future systems; a visible station or service tab does not implement a service.

| Capability | Current evidence | Remaining system |
|---|---|---|
| Docking and hangars | Range-checked dock/undock, class boarding and parked cargo | Validate current walk/board controls in the installed build; no assumption of full station life simulation |
| Inventory and organizations | Ore cargo, personal per-system storage, shared organization storage | General item stacks, equipment inventory, granular roles/permissions and transfer history UX |
| Market | Sell escrow, partial purchase and cancellation | Buy orders, regional discovery, order expiry, fee policy and robust market history |
| Ship fitting | Fixed class specifications and two mining modules | Item-backed fitting slots, CPU/power budgets, saved fits and server validation |
| Repair and insurance | Capacitor regeneration; drone damage exists | Persistent ship damage, repair quotations/transactions, insurance ownership and claims |
| Resource industry | Ore extraction and haul automation | Refining recipes, materials, manufacturing/research queues and escrowed job inputs |
| Contracts and logistics | Controller-authorized resource delivery | Public/private exchange and courier contracts, deadlines, collateral and settlement |
| Station operations | Fixed station endpoints and services | Service discovery/availability, access profiles, configurable fees, fuel/service upkeep and ownership lifecycle |

These are design gaps, not a request to implement all systems in this stabilization
release. First preserve a reliable gather → return → store → board → depart loop.

## Release evidence still required

- Local workspace tests and strict lint passed after integration. Native GL
  rendering and hangar input deadlock regression passed separately. See
  `validation.md` for the native walkthrough boundary.
- Exercise minimum-size cockpit undock/board controls, direct input/focus loss,
  delegated job completion, remote takeover/return, and live feed changes natively.
- Crash/restore an isolated database during economic actions and a multi-trip job;
  verify ledger conservation and honest job status after reopening.
- Keep public transport/authentication, migration/backup procedures, license and
  dependency/asset rights review, contributor/security documentation, and host
  trust limitations as explicit public-preview gates. This audit does not close
  those gates.
