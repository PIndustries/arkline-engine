# Observer balances and local full audits

Gameplay protocol 2 uses observer schema 2. An admitted pilot receives exact own credits and active cargo. Other pilots' balances are `null`, including workers under an existing fleet grant or remote-piloting lease. `null` means unavailable; `0` means a known empty balance. Global stored ore is unavailable. Owner inventory remains in the existing actor-scoped assets view. A fleet grant preserves its existing power and job-control authority; it grants neither cargo visibility nor full-audit access.

Anonymous readers must request `Observe` with `observer_schema_version: 2`. Anonymous unversioned `Snapshot`, unsupported observation schemas, protocol-1 admission and every game `ExportLedger` request fail closed. Denials contain no world snapshot or full ledger. The same boundary covers TCP, WebSocket/WSS, account-authenticated sessions and embedded/shared editor requests. Shared-editor joins also require observer schema 2. These changes do not update a running older server or app automatically.

Ordinary responses cannot carry a full ledger. Jobs are visible only to their pilot or a currently granted controller; a stale recorded controller is not sufficient. Private capacitor/avatar filtering is retained. No captain, crew, auditor role, credential or persistent privilege is created.

## Operator-local audit

An operator who already has OS permission to read a saved database can run:

```text
arkline-audit-export --database EXISTING.sqlite --output NEW_AUDIT.json
arkline-verifier --file NEW_AUDIT.json --checkpoint PIN.json
```

The exporter does not listen on a socket or start the server. It opens the explicitly supplied existing database read-only and reads committed world state, complete ordered entries and the saved clock in one SQLite read transaction. It does not advance simulation, migrate or repair the world, grant permissions, or serialize account tokens and identity bindings. SQLite uses its normal locking and WAL sidecars; do not copy a bare live database or force immutable/nolock mode. Missing, inaccessible or invalid sources fail without creating a world. Output must be a new file and is created privately on Unix; existing files and source/sidecar aliases are never overwritten.

Full audit format remains **1**, independently of game protocol **2**. Canonical balances remain numeric. Existing economic events, ruleset 1, sequence numbers, hash calculation and pinned sequence/hash checkpoints are unchanged. Existing complete v1 audit files remain verifiable. The exporter reports committed saved state, not a fresh live simulation sample. A full audit verifies economic consistency and pinned continuity; it does not prove that mining occurred, player consent or operator honesty.

## Upgrade requirements

Native clients/editor, browser clients, shared-editor peers and the command CLI must upgrade together to read nullable balances and require schema 2. Old game clients receive an update/version refusal; there is no full-data legacy fallback. Existing pilot credentials and saved world data do not need rewriting.

`arkline export PATH`, `arkline-verifier --server`, and the cockpit's online audit action are unavailable. They do not connect to a fallback export or advance saved checkpoints. Use a trusted operator-provided full file with `arkline-verifier --file`. Network auditor access would require a separate real authorization design; it is not implemented here.

## Remaining information and inference

Public observations still include presence, coarse location, flight mode/targets and warp timing; mining targets, locks/cycles and drone state; public market orders/prices; and belt/asteroid reserves. Authorized job observers retain job goals, progress and target information. These fields, sparse activity and cargo-dependent motion can permit inference. Existing public exports and third-party copies cannot be retracted. This is direct balance/export access control, not a promise of perfect cargo secrecy. Range/sensor subscriptions and crew permissions remain deferred.
