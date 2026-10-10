# Arkline v0.1 architecture

```text
Rust native cockpit / CLI
          |
     versioned commands
          v
Authoritative Rust server
  location + travel + mining + escrow + trade
          |
    one SQLite transaction
     /               \
durable world     economic event + previous hash
                         |
                  read-only JSON export
                         v
                Independent Rust verifier
                replay + conservation + checkpoint
```

The server owns gameplay decisions. A client may request a warp or trade; it cannot
set a balance, grant ore, or submit its own authoritative snapshot. Each successful
economic command appends one versioned event in the same transaction as its state
change. Rejected operations append nothing. Resume credentials are independent of
public pilot names and never appear in economic exports.

The event set includes pilot creation, mining, sell escrow, trade, cancellation,
boarding with cargo swaps, station storage transfers, and corporation membership
and shared storage operations.
Starter credits are explicit issuance. The ore conservation equation is:

`belt ore + active cargo + inactive ship cargo + personal storage + corporation storage + open sell escrow = initial belt ore`.

Authenticated responses contain the pilot's detailed assets; shared corporation
storage is shown only to members. Public snapshots expose the combined stored ore
total. The public ledger still reveals storage movements and membership, so this
response filtering is not economic confidentiality. See
[station asset audit boundaries](station-assets-audit.md).

Credits are integer quantities, and multiplication/addition are checked for overflow.
Total credits are `pilot count * starter issuance` in this ruleset. There are no
implicit NPC mints, fees, taxes, or operator rewards.

The server integrates flight at 10 Hz and persists position, velocity, heading and
a normalized quaternion orientation. Direct piloting submits bounded body-axis
translation/rotation inputs through a latest-value client mailbox. The server
expires input after 500 ms and applies assisted braking; it never trusts client
position or orientation writes. Warp and gate jumps use durable arrival timestamps. A gate transition
moves the ship to its paired gate in another system. All three systems currently
run in the same process and database; this is not distributed authority handoff.
The client polls snapshots, buffers normal flight and samples the same durable
warp trajectory as the server on every presentation frame. Warp effects represent
that trajectory; they do not replace travel with a timed location swap.

Asteroids have persistent IDs and individual remaining ore. Their reserves sum
to the legacy global belt reserve. Migration splits only the existing reserve,
without changing cargo, credits or ledger history. Mining checks system, target
lock, surface distance, cargo capacity and remaining resource before each module
cycle award. Slots 0 and 1 run independently with hull-defined cycle time and
yield. StartMining is idempotent for an already active target; StopMining cancels
an unfinished cycle without a partial award. Each completed cycle atomically
updates the selected rock, global reserve, active cargo and an OreMined event.

The same advancement runs in the server tick and request transactions. It awards
at most one cycle per module per advancement, then schedules the next full cycle
from the current server time. A cycle overdue by at least one additional whole
cycle stops without an award. Rapid polling cannot shorten cycles. Invalid
commands roll back their transaction, including any due cycle; the next successful
tick can commit that cycle once. Modules stop on invalid targeting/range, cargo
full, depletion, departure from normal space, last disconnect and restart.
Restart creates missing legacy module slots in an idle state; it never awards
offline cycles. Boarding a different hull resets its module phases. Legacy
Mine/MineAsteroid quantity requests are rejected.
The backward-compatible event records quantity and pilot; it does not attest to
the historical asteroid ID. The verifier checks the aggregate asteroid reserve,
not physical mining truth or per-rock historical allocation.

SQLite contains a serialized world record and ordered economic events. A single
server writer serializes access. Restart restores committed world state, while the
verifier separately reconstructs economic state from the exported events. A fixed
server ruleset and ordered JSON tuple hashing define the current event format.

## Community verification boundary

The verifier has no dependency on the server crate and no database credentials. It
has an independent rules implementation, sharing only protocol definitions. It checks
the export's reconstructed economy against the supplied snapshot and can retain a
checkpoint to reject later rollback or rewriting of its already observed history.

The first checkpoint is locally trusted. No operator signature, external witness,
consensus, independent simulation replay, privacy filtering, or node payout exists.
A malicious operator can fabricate a consistent first history. Different verifiers
must eventually exchange authenticated checkpoints to expose inconsistent views.

Network commands currently have no idempotency keys. If a response is lost, the
outcome is unknown: reconnect and inspect state before deciding another action.
Clients must not automatically retry economic commands. Losing the first credential
response can strand an identity; account recovery is not implemented.

## Growth path

After owner validation, extend one verified behavior at a time: persistent command
IDs and receipts; signed checkpoint identity; witness comparison; creator content
formats and editor; regional authority and handoff; operator admission and service
measurement. Reward policy and public governance need explicit owner/community
decisions. These are planned boundaries, not implemented features.
