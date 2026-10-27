# Authoritative timed unloading checkpoint

Local continuation of the approved flagship work, based on `1c068a3aa360a568ab92ae5dc3ac390fb0b65b3a`.
Progress is tracked in this local checkpoint and private backups, not new public issues or pull requests.
This is a bounded DockingV1 preview, not engine publication, deployment, a save
migration, physical crane simulation, or a finished station logistics system.

## Contract and compatibility

Protocol 1 and economic ruleset 1 are deliberately retained. This is an additive
capability: authenticated `AssetsView.cargo_unloading` advertises independent
`schema_version = 1`. Legacy/Orbital worlds omit it and retain the existing
immediate storage commands. New clients send identified unload requests only for
the exact advertised capability; unknown capability versions disable service
commands. Older clients can still connect normally; an unversioned deposit in
DockingV1 is refused with guidance, never silently treated as an identified
operation. No authentication or membership policy changes are included.

`StartCargoUnload { operation_id, units, destination }` binds the accepted
operation to the authenticated pilot's immutable active vessel ID/class, system,
Secured assigned berth, and concrete personal/corporate storage owner. The ID must
be exactly the persisted high-water mark plus one. An exact retained retry returns
the existing operation; reuse with different units/destination is refused. Old
operation retries do not cancel a newer resource job or restart cancelled work.

The source hold and destination storage remain reserved while Unloading or Held.
One operation owns a source hold, and one operation owns a destination storage
account at a station. Other corporation members cannot mutate that reserved store.
Ordinary deposit/withdrawal, market cargo mutation, hull switching, resource-job
start and departure cannot spend or replace a reserved source. Deployed drones
must be recalled before reservation. Permissions, binding, remaining source
quantity and destination overflow are checked again before every batch.

## Authority, receipts and interruption

- At most **25 generic ore units per 500 simulation milliseconds**. No mass,
  container, conveyor or equipment-energy model is implied.
- Starting a transfer moves no ore. Progress is only committed batch quantities.
- A delayed tick commits at most one due batch and schedules the next from the
  current time. It never catches up many batches in one presentation frame.
- Existing personal/corporation economic events are reused. The independent
  verifier does not import server logic or accept a client animation as evidence.
- In both request-driven and tick-driven paths, one SQLite IMMEDIATE transaction
  moves inventory, appends the real economic event, records its exact sequence
  and hash in the batch receipt, updates any linked job counter, and saves the
  world. Failure before commit rolls all of these back together.
- Cancellation retains all committed ore and receipts; it releases only the
  uncommitted remainder. A cancelled/completed operation cannot resume.
- Disconnect, reconnect and restart hold unfinished work. An exact explicit
  resume revalidates binding and starts a fresh 500ms wait, with no offline award.
  Repeating resume while already running does not reset or accelerate its clock.
- Cancelling a resource job through any existing path holds its linked unload.
  Explicit job cancellation cancels the remaining unload. An explicit manual
  unload resume after a cancelled job detaches that job linkage: completed job
  accounting stays unchanged; later manual cargo is not attributed to the job.
- The journal currently retains all operation and batch receipts. There is no
  eviction/reuse window and no claim of production-bounded save size or scalable
  long-lived history. A future paged durable journal must preserve retry safety.

## Presentation

The actual cargo service panel shows phase, operation ID, confirmed/requested
units, reserved remainder and receipt count, with authority-gated Cancel/Resume.
The permanent onboard cargo glass shows the same confirmed operation only when
its pilot and vessel identity match the viewed ship; private owner receipts must
never appear on a remotely observed worker. Ordinary onboard ore remains the
actual pilot inventory. Stale, paused and disconnected labels remain explicit;
there is no client-side interpolation of awarded units or fake physical crane.
Loading from station storage remains immediate in this checkpoint.

## Acceptance plan and evidence boundaries

`apps/server/tests/timed_unloading.rs` uses genuine mined cargo and disposable
SQLite fixtures. Coverage includes 25+25+7 partial completion, exact retries,
conflicting IDs, reservations, ownership/capability/Secured checks, corporation
locking, cancellation, disconnect/reconnect, restart, abrupt process exit after
commit, delivery counters, and SQL-trigger failures after ledger append on both
tick and request paths. Synthetic overflow/revocation fixtures are explicitly
separate from ledger-valid economic journeys.

The ignored export fixture writes actual 100ms authority Responses and a verified
ledger for native playback. It includes full completion, disconnect/held/resume,
and partial cancellation/retry; it does not render a crane. Native rendering and
GIF review are separate acceptance steps owned by the integration/media workers.
A JSON export alone is not visual proof. Exact commands, counts, source commit
and capture provenance belong in the final validation receipt, not inferred here.

Remaining gates include source integration, combined native UI review, normal-world
adoption, transactional timed loading, physical cargo equipment and power/heat,
production receipt paging, wider network/multi-pilot soak and release checks.
