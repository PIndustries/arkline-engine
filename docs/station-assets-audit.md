# Station assets and independent economic replay

Each pilot has one active hull and three inactive prototype hulls. Active cargo appears once in `PilotView.ore`. Inactive cargo belongs to a particular hull parked at a particular station. Boarding exchanges these cargo balances; it does not copy them or empty them into personal storage. Starter hulls are empty and seeded at the home station once, including during migration from the earlier schema.

Personal ore storage is keyed by pilot and system. Corporation storage is keyed by corporation and system, and counted once regardless of membership size. Deposits and withdrawals require docking at that system's station. Owners invite existing pilots; invitees must accept. Members can deposit corporation ore; only the owner can withdraw or invite. The current model has no roles, ownership succession, leaving, kicking, ship trading, shared hulls, or paid ship acquisition.

The verifier independently replays `ShipBoarded`, personal transfers, corporation creation/invitation/acceptance, and corporation transfers alongside mining and escrow trading. `ShipBoarded` includes source and destination hull classes and exact stored/loaded cargo. `CorporationInvited` includes the inviter so owner authorization can be checked. Replay rejects invented inactive cargo, wrong-station loading, cargo overflow, insufficient station balances, membership without invitation, nonowner invitations, and nonowner corporation withdrawals.

Conservation is checked as:

```
belt reserve
+ active hull cargo
+ inactive hull cargo
+ personal station ore
+ corporation station ore (once per corporation)
+ sell-order escrow
= initial ore supply
```

Credits remain independently conserved. `Snapshot.stored_ore` must equal the replayed sum of inactive cargo and both storage types. Any failure prevents checkpoint advancement. Existing checkpoint rules still detect rollback and changes to previously pinned history.

## Acceptance evidence

`apps/server/tests/station_assets.rs` exercises real server command APIs with a deterministic clock: mining, returning to station, boarding/reboarding with cargo, personal storage, corporation invitation and acceptance, authorized owner withdrawal, rejected member/outsider withdrawals, rejected member invitations, failed-transfer rollback, per-system isolation after a gate jump, persistent restart, and independent export replay. A separate test removes the new account fields to reconstruct the legacy schema and verifies that repeated restarts do not replenish ships or mint cargo.

Verifier tests also modify economic events and recompute all hashes. Fabricated cargo, wrong-station hull loading, unauthorized invitations/withdrawals, missing invitations, insufficient personal balances, and a false aggregate stored balance are rejected even when the hash chain is internally consistent.

## Trust and scope limits

Session-filtered `Response.assets` is a convenience view, not confidential holdings. The public ledger reveals financial transfers, boarding cargo, invitations, and membership, allowing those balances and relationships to be reconstructed.

The public snapshot exposes aggregate stored ore rather than every private storage/hangar bucket. Replay validates the individual event-derived buckets, but compares only their combined total with the current snapshot. It cannot independently prove that every private database bucket or invitation view matches the replayed allocation.

Legacy hull selections were not economic events. Until the first new boarding event, the verifier cannot establish a legacy pilot's active hull class from history; it infers the source class at that event. Unseen starter hulls must be empty and at the home station. Zero-cargo asset ownership, physical docking, mining occurrence, travel, and operator honesty remain outside this economic proof. Events are operator statements, not independently signed player consent. A checkpoint anchors previously observed history; it does not make a dishonest operator trustworthy.
