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

The initial event set is pilot creation, mining, sell escrow, trade, and cancellation.
Starter credits are explicit issuance. The ore conservation equation is:

`belt ore + held pilot ore + open sell escrow = initial belt ore`.

Credits are integer quantities, and multiplication/addition are checked for overflow.
Total credits are `pilot count * starter issuance` in this ruleset. There are no
implicit NPC mints, fees, taxes, or operator rewards.

Travel is a persistent destination and wall-clock arrival time. GUI positions are a
visual interpolation of that state, not a physical trajectory. The client polls
snapshots; this is not the eventual high-frequency regional simulation transport.

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
