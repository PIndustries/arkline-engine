# Legacy systems: historical source review and Rust restoration plan

Historical snapshot, reviewed on 2026-10-09. Current Rust implementation has since
expanded substantially; statements about missing features below apply only to that
review date. See [component-map.md](component-map.md) and [api.md](api.md) for the
current source-based map and protocol.

Reviewed local source on 2026-10-09. This is an implementation inventory, not a
claim that the old deployments or test suites are currently healthy. Neither
legacy application was started or changed. The Rust features below are proposed
unless explicitly listed as present.

## Source locations

The primary reference is the sibling `arkline-core` Elixir repository. Paths in
its sections below are relative to that repository. The earlier cosmology app is
`arkline/arkline_app`; it was checked separately for clustering behavior.

The current Rust workspace implements one authoritative server, persistent pilots,
station/belt travel, finite ore, escrow trades and independent economic replay.
`crates/arkline-protocol/src/lib.rs` currently has no ship-computer, fitting,
construction, communications-coverage or regional-node commands. Cockpit visuals
and authenticated connection presence do not establish those systems.

## Ship computers and executable programs

| Source | Implemented behavior |
| --- | --- |
| `lib/arkline/lifecycle/computer.ex:71`, `default/1` | Patchwork defaults to PiOS Mk-I: 33 MHz, 64 KB RAM, 256 KB storage and a 9,600 bus field. Tinker maps to Mk-II: 66 MHz, 128 KB RAM, 512 KB storage and 38,400 bus. Other names fall back to Mk-I. |
| `lib/arkline/lifecycle/computer.ex:83`, `estimated_bytes/1` | Estimates saved program size as `String.length(text) + 32`; this counts characters, not UTF-8 bytes. |
| `lib/arkline/pide.ex:71`, `write/1` | Saves the program with a timestamp through the Store abstraction. This function does not enforce the ship's storage or RAM limits. |
| `lib/arkline/pide.ex:401`, `run/2` | Runs validated command text with loops, conditions, named subroutines and event handlers through existing ship syscalls. A shared five-minute deadline is checked between interpreter steps; repeat blocks are limited to 1,000 iterations. |
| `lib/arkline/pide/runner.ex`, `run_named/3`, `run_body/3`, `kill/1` | Starts supervised tasks, records pilot/account/ship metadata, lists running programs and terminates them. Concurrency admission uses account subscription benefits; lookup failures fall back to 999. |
| `lib/arkline/pide/rom.ex`, `packages_for/1` | ROM package availability is derived from a computer RAM requirement. Package metadata includes entry routines and RAM cost. |
| `lib/arkline/pi_os/service.ex:52`, `start/3` | Rejects missing packages, insufficient prerequisite RAM, missing entry routines and a running-service RAM total above capacity. Starts a recurring ROM-entry program through the runner. |
| `lib/arkline/pi_os/kernel.ex`, `uname/1`, `free/1`, `df/1` | Exposes hardware, process and storage status. RAM display estimates 1 KB per matching runner; storage display sums saved-program estimates. These are modeled values, not host process measurements. |
| `lib/arkline/lifecycle/computer.ex:96`, `crypto_throughput_msgs_per_sec/1` | One real consumer of CPU rating: modeled message processing capacity scales as `max(1, round(cpu_mhz * 0.5))`. |

The ship computer is real gameplay structure and automation code, but it is not a
full hardware emulator. The inspected PIDE interpreter does not derive execution
fuel or wall-time allowances from `cpu_mhz`. Its deadline is cooperative, not a
forced per-instruction preemption guarantee; a long wait can overshoot it.
`bus_kbps` is not proof of transport throughput enforcement.

There is also a scope mismatch to fix during the port: `Service.list_for_ship/1`
filters running ROM tasks by account and label, rather than `ship_ref`, despite
presenting its RAM total as per-ship. Capacity checks and task registration occur
separately. Preserve the useful behavior without copying those accounting races.

## Ship builds, fitted modules and construction

These are three related but separate systems.

**Loadouts:** `lib/arkline/lifecycle/ship/module.ex` defines module identity, kind,
slot, cycle duration, range, activation time, power, energy per cycle, efficiency,
CPU draw, powergrid requirement, overheating and disabled state. Active modules
are advanced by the ship tick; mining behavior consults the fitted mining laser.

`lib/arkline/drivers/hull_capacity.ex`, `budget/1`, provides the hull budget table.
For example, Patchwork has two high/mid/low slots, 80 CPU units, 50 MW powergrid
and a 200 MWh capacitor envelope. These fitting CPU units are separate from the
computer's MHz rating. `lib/arkline/drivers/fitting.ex:51`, `validate/2`, checks
high/mid/low slot counts, total CPU draw, powergrid and a one-hour power envelope.
`usage/1` provides the budget-bar totals. The same file implements ranked,
diminishing effect stacking.

`lib/arkline/lifecycle/ship.ex:877`, the `fit_module` handler, validates the whole
candidate fit before saving it. The legacy `install_module` handler is dock-only
but appends without this validator; `fit_module` itself has no dock-status guard.
`unfit_module` removes matching names. Rust needs stable module-instance IDs and
one authoritative inventory-to-fitting transaction instead of retaining both
admission paths.

**Manufactured hulls:** `lib/arkline/lifecycle/colony.ex`, the assembly flow around
line 410, spends 500 units from its in-memory refined-material bank and enqueues a
hull through `Shipyard.enqueue/1`. Processor workers withdraw raw hangar materials
and add refined output in `lib/arkline/lifecycle/colony/worker.ex`. The colony's
assembly bank is initialized in process state; this is not a fully atomic,
persistent production job model.

`lib/arkline/lifecycle/shipyard.ex:66`, `enqueue/1`, stores hull provenance and
availability. `allocate/2` at line 93 reads availability then writes allocation
as separate operations. `lib/arkline/facades/ships.ex`, `allocate_from_shipyard/3`,
uses that stock for pilot onboarding and records builder provenance. The source
also retains fallback starter creation. A Rust port must distinguish declared
starter issuance from manufactured supply and prevent two consumers allocating
one hull.

**Community structures:** `lib/arkline/lifecycle/schematic.ex` contains material
recipes, defaults and progress calculations. `Construction.start_build/1` at
`lib/arkline/lifecycle/construction.ex:123` creates a supervised build-site process;
`contribute/4` at line 192 caps accepted materials to the remaining requirement,
records contributors, and finalizes into `Infrastructure.add/1` when complete.
Relays are included: the default recipe requires iron, copper, silicon and
uranium. Range/cargo validation is delegated to the caller. Incomplete contribution
state is held in the process; this path does not itself persist a recoverable
build job or atomically debit contributor inventory.

A separate, persistent `lib/arkline/lifecycle/gate_construction.ex` models staged
gate projects with deposited materials and advancement thresholds. It is distinct
from the general live construction process and should not become a second Rust
source of truth for material ownership.

## Communications relays and inter-system travel

The source does not define a single service named “FTL relay.” It contains
communications infrastructure, network-dependent stargate travel, and account
connection-capacity accounting. Keep those concepts explicit in the port.

- `lib/arkline/lifecycle/infrastructure.ex` stores built stations, stargates,
  acceleration gates, communications relays and jammers, including ownership,
  position, operating state, band and antenna configuration.
- `lib/arkline/lifecycle/comms/coverage.ex`, `covering/3`, tests operational relays
  in the same system against band range, power multiplier and antenna beam.
  `signal_grade/2` returns dark/edge/core for zero/one/multiple covering relays.
  `reachable?/3` additionally compares signal with jammer noise against a 10 dB
  threshold. `in_network?/2` only checks coverage; it does not apply that SNR gate.
- `lib/arkline/lifecycle/comms/path.ex`, `plan/4`, picks an uplink and downlink and
  calculates three legs with propagation delay plus a switching allowance. It
  does not discover a connected multi-relay graph or route between systems.
- `lib/arkline/lifecycle/ship.ex:2533`, the `jump` handler, requires an in-space
  ship, suitable player standing and communications coverage. `do_handle_jump/2`
  requires a destination gate within 5 km, fuel and energy, then begins timed
  inter-system transit and records the energy debit. This is different from the
  current Rust preview's station-to-belt warp.
- `lib/arkline/lifecycle/relay.ex:185`, `register_session/3`, applies account
  channel/bandwidth policy for SSH, web and API sessions. `heartbeat/2` records
  supplied usage and activity; stale channels expire after two minutes. This is
  session accounting, not a distributed simulation authority or reward system.
  Although its comments call admission atomic, the inspected implementation does
  a separate gate read followed by `Store.put`. Do not reproduce that race.

## What auto-clustering actually did

`lib/arkline/cluster.ex:39`, `configured_peers/0`, reads explicitly configured
`ARKLINE_CLUSTER_NODES`. Boot attempts connections briefly; the supervised loop
retries absent peers every five seconds using `Node.connect`. Unconfigured nodes
idle. It does not perform open node admission or discover community operators.

There is real automatic synchronization after connection:

1. `lib/arkline/lifecycle/ship/index.ex:94`, `upsert/1`, writes a local ETS snapshot
   containing the hosting node, velocity and active mining/firing targets, then
   fans it out to connected peers through `:erpc.cast`.
2. `sync_from_peer/1` at line 168 pulls existing rows. The `nodeup` handler schedules
   that backfill; `nodedown` removes rows hosted by the departed peer.
3. `lib/arkline/facades/ships.ex:436`, `dispatch/3`, resolves `host_node` from the
   index and calls the remote ship process with a five-second RPC timeout.
4. Phoenix PubSub is supervised alongside local registries in
   `lib/arkline/application.ex` and carries cross-node event traffic.

This establishes configured reconnection, cross-node visibility and command
routing. It does not establish safe automatic authority failover. In the inspected
paths, remote index updates overwrite rows without ownership epochs, ship
registries are local, and `Persistence.restore_all/0` at
`lib/arkline/lifecycle/persistence.ex:165` iterates all saved ship states without a
regional ownership filter. A node-down handler removes visibility; it does not
transfer authoritative ownership. Multi-node partition recovery has not been
validated in this review.

The earlier `arkline/arkline_app/lib/arkline_app/application.ex:12` starts
`DNSCluster` using `DNS_CLUSTER_QUERY` from `config/runtime.exs:39`; absent a query,
it is ignored. That is configurable DNS discovery for the cosmology application,
not evidence that its simulation has region ownership or safe failover.

## Bounded Rust restoration sequence

These are proposed acceptance boundaries, not implemented controls.

| Slice | Concrete first implementation | Required acceptance |
| --- | --- | --- |
| Ship computer and fitting | Versioned hull/module/computer catalog; stable ship/module IDs; server-side fit/unfit; separate fitting CPU budget and script instruction budget | Legal fit survives restart; slot/CPU/power violations leave inventory and ledger unchanged; removing the mining module disables extraction |
| One useful ship program | Small typed instruction interpreter with inspect, wait, mine, warp and dock commands; stored program bytes; bounded instruction fuel, RAM, storage, action rate and concurrency; start/stop/status UI | A saved mining-return program uses the same authoritative command rules as a person; infinite loops yield; oversize code is rejected; stop releases resources; another pilot cannot run code on the ship |
| One production recipe | Persisted input escrow, recipe/version, due time and unique job ID; completion issues one identified module or hull with provenance | Restart mid-build; complete and claim once; duplicate submissions/replies cannot double debit or issue output; verifier accounts for all transformations |
| One buildable relay | Versioned material recipe and transactional contributions; durable site state; completed relay appears in the map and coverage queries | Two contributors finish a relay without overconsumption; restart preserves progress; finalization happens once; online/offline relay changes coverage visibly |
| One inter-system gate pair | Two real system IDs, selected gates, proximity/coverage/fuel/energy checks and durable departure/arrival | Uncovered jump fails without spending; admitted jump spends once and arrives once after restart; local warp remains distinct |
| Configured two-node connection | Authenticated approved-node identity, explicit peer configuration, health and read-only region summaries | New peer is visible only after admission; unknown peer is rejected; disconnect and reconnect do not create duplicate ships or economic authority |
| Regional authority handoff | Durable region leases/epochs, fencing on every mutation and explicit transfer records; central economy remains authoritative initially | Partition the old owner, transfer authority, and prove the old owner cannot commit; reconnect never duplicates inventory or replays an arrival |

Add economic event types and independent verifier rules in the same changes that
introduce fitting inventory, manufacturing, construction or fuel ownership.
Schema/ruleset migrations must preserve old saves and make unsupported versions
explicit. Do not infer node rewards, public hosting readiness or automatic
simulation failover from a discovered peer or a green heartbeat.

The first usable restoration should be one computer-equipped ship, one legal fit
and one bounded program completing the existing mine/return journey. Buildable
relays and genuine regional hosting follow their own verified journeys; they
should not be represented by decorative controls in the current preview.
