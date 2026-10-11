# Implemented game API

Arkline currently exposes a JSON command protocol, not a REST gameplay API. The authoritative schema is [`crates/arkline-protocol/src/lib.rs`](../crates/arkline-protocol/src/lib.rs). `PROTOCOL_VERSION` is currently `1`; the dated application build version is separate. New fields often have Serde defaults for old saves, but clients must still negotiate the protocol version and tolerate unavailable optional state. This is a preview contract, not a stable third-party SDK guarantee.

## Endpoints and framing

| Endpoint | Function |
| --- | --- |
| `wss://relay.arkline.network/ws` | Official production world |
| `wss://test-relay.arkline.network/ws` | Independent test world |
| `GET https://relay.arkline.network/health` | Production tick freshness |
| `GET https://test-relay.arkline.network/health` | Test tick freshness |
| Local TCP `127.0.0.1:7777` by default | One JSON request per newline; one JSON response per newline |
| Optional local WebSocket listener, `/ws` | One request per text frame/message; one response per text message |

`GET /health` returns 200 when a successful simulation tick is less than three seconds old, otherwise 503. It does not establish gameplay, backup or account health. Use GET, not HEAD. The public proxy can present rejected upgrades as an edge error rather than preserving the origin's status.

WebSocket is request/response, not a push subscription. Poll `snapshot` for current state. Keep request/response ordering; there are no request IDs or durable idempotency receipts. Browser Origin must match the configured exact allowlist; native clients may omit Origin. An Origin is not an identity. No tokens belong in URLs. See [public limits and transport lifecycle](../apps/server/PUBLIC_TRANSPORT.md) for the complete admission, size, timeout and rate policy.

## Establishing a pilot session

These examples use an invented name and placeholder credential. On a new test world, creating a pilot changes durable state and issues starter resources; use a read-only snapshot when no creation is intended.

```json
{"command":"snapshot"}
{"command":"connect","protocol_version":1,"pilot":"ExamplePilot","resume_token":null}
```

Save the returned `resume_token` securely, scoped to this server/world. For an existing pilot:

```json
{"command":"connect","protocol_version":1,"pilot":"ExamplePilot","resume_token":"REPLACE_WITH_SAVED_CREDENTIAL"}
```

A pilot name is public, not proof of ownership. Existing names require their credential. New names are subject to server validation and registration limits. The server persists a token hash; public snapshots and ledger exports contain no resume tokens. Current game credentials are independent of website sign-in. There is no account recovery API or shared OAuth login yet.

A response contains `ok` and `message`, and may contain `snapshot`, `assets`, `resume_token`, or `ledger`. Absent optional keys can be omitted entirely. Error messages are human-readable, not stable machine error codes.

```json
{"ok":false,"message":"Human-readable rejection reason"}
```

Read-only `snapshot` and bounded `export_ledger` do not require login. Mutating commands require a bound pilot session. Presence is reference-counted across simultaneous authenticated transports. Closing one connection does not necessarily make a pilot offline. Last-disconnect safety neutralizes transient controls; persisted ships and resources remain.

**Unknown outcome:** a timeout after sending a command does not prove failure. Reconnect, inspect current state and reconcile the action. Never automatically replay trades, transfers, job changes or other mutations. A lost first registration response can leave the newly created pilot's credential unavailable.

## Commands and fields

Each row lists the `command` discriminator and its additional JSON fields. Numeric IDs are server-assigned unsigned integers. Distances/positions are meters, time fields are server milliseconds, and angles are radians. Do not derive world positions from rendered pixels or send client-computed awards.

| Area | Commands and fields | Important conditions |
| --- | --- | --- |
| Observation | `snapshot`; `export_ledger` | Public, bounded; private assets only on authenticated responses |
| Flight | `pilot_input`: `translation:[x,y,z]`, `rotation:[roll,yaw,pitch]`; `navigate`: `mode` | Finite normalized controls; short input lease, server acceleration/rotation limits |
| Travel | `undock`; `dock`; `warp`: `destination`; `jump`: `gate_id` | Server location, proximity, power and transit restrictions |
| Targeting | `lock_asteroid`: `id`; `unlock_asteroid`: `id`; `unlock_target` | At most five pending/completed locks; timed, same-system/range checks |
| Ship mining | `start_mining`: `module_id`, `target_id`; `stop_mining`: `module_id` | Slots 0 and 1; confirmed target; fixed server cycle/yield; no partial-cycle award |
| Hulls/storage | `select_ship`: `ship_class`; `deposit_ore`, `withdraw_ore`: `units` | Docked and aboard as applicable; hull/storage location and cargo limits |
| Market | `place_sell`: `units`, `unit_price`; `buy`: `order_id`, `units`; `cancel_sell`: `order_id` | Escrow, checked arithmetic, valid owner/buyer, docking and capacity checks |
| Corporations | `create_corporation`: `name`; `invite_corporation`: `pilot`; `accept_corporation`: `corporation`; `deposit_corporation_ore`, `withdraw_corporation_ore`: `units` | Explicit invitation; member deposits, owner-authorized withdrawals |
| Drone fleet | `launch_drone_class`: `class`; `recall_drone_by_id`, `drone_hold_by_id`: `id`; `drone_navigate_by_id`: `id`, `position`; `orbit_drone`: `id`, `target` | Owned IDs; class caps; signal/range and lifecycle validation |
| Drone work | `assign_mining_drone`: `id`, `target_id`; `assign_attack_drone`: `id`, `target_pilot`, `target_drone_id` | Mining requires confirmed target; combat currently drone-vs-drone, not ship guns |
| Jobs | `start_resource_job`: `target_units`, optional `pilot`; `cancel_resource_job`: optional `pilot` | Total goal may require multiple trips; own pilot or explicit delegation |
| Delegation | `set_fleet_controller`: `controller` name or null | Worker grants/revokes permission; no account or inventory delegation |
| Manual takeover | `begin_remote_control`: `pilot`; `end_remote_control`; `remote_control`: `pilot`, nested `request` | Active matching lease plus shared gameplay whitelist; nested/account/economic/job actions denied |
| Surface props | `spawn_surface_prop`: `kind`, `position`; `move_surface_prop`: `id`, `position`; `remove_surface_prop`: `id` | Surface proximity, bounded placement and ownership |
| Station avatar | `disembark_ship`; `enter_ship`; `station_walk`: `forward`, `strafe`, `turn`; `station_sit`: `seated` | Private docked hangar; collision/range/input lease; boarding required for ship departure |
| Station jump (current source) | `station_jump` | Authoritative bounded vertical jump; may await the next deployed build |

Legacy `mine` and `mine_asteroid` quantity requests are rejected: clients cannot choose mining yield. Legacy singleton `launch_drone`, `recall_drone`, `drone_hold`, `drone_navigate` remain compatibility operations; new fleet clients should use explicit IDs. `select_ship` boards an owned hull; it is not unrestricted remote hull spawning.

## Nested JSON types and examples

`NavigationTarget` uses `kind`: `station`, `belt`, `pilot` with `pilot`, `asteroid` with `id`, or `gate` with `id`. `NavigationMode` uses `mode`: `stop`, `manual`, `approach` with `target`, `orbit` with `target` and `radius`, `keep_range` with `target` and `distance`, or `fly_to` with `position`. Direct manual controls use `pilot_input`; the server may reject modes inappropriate for a particular request.

```json
{"command":"navigate","mode":{"mode":"approach","target":{"kind":"asteroid","id":1}}}
{"command":"navigate","mode":{"mode":"orbit","target":{"kind":"pilot","pilot":"ExamplePilot"},"radius":1000.0}}
{"command":"pilot_input","translation":[1.0,0.0,0.0],"rotation":[0.0,0.0,0.0]}
{"command":"warp","destination":"belt"}
{"command":"warp","destination":{"gate":{"id":1}}}
{"command":"lock_asteroid","id":1}
{"command":"start_mining","module_id":0,"target_id":1}
{"command":"start_resource_job","target_units":4000,"pilot":null}
{"command":"remote_control","pilot":"WorkerExample","request":{"command":"navigate","mode":{"mode":"stop"}}}
```

`Destination` deliberately has a different representation: `"station"`, `"belt"`, or `{"gate":{"id":N}}`. Ship classes are `pioneer`, `prospector`, `borealis`, `stratum`, `monolith`, `hauler`, `vanguard`; drone classes are `recon`, `mining`, `attack`; prop kinds are `crate`, `pylon`, `platform`. Read current target IDs from snapshots instead of assuming sample ID 1 exists or is reachable.

## Snapshot, private assets and ledger

`Snapshot` contains `server_time_ms`, `pilots`, `online_pilots`, `asteroids`, `orders`, `belt_ore`, `stored_ore`, `ledger_sequence`, `ledger_hash`, `remote_controls`, `fleet_links`, `resource_jobs`, `surface_props`, and `traffic`. It is a world view, not proof every listed ship is connected or visible in the local system. Filter by `system_id` for local rendering.

`PilotView` contains public name, credits, active cargo `ore`, hull class, system/location, optional flight/power/surface/station avatar, mining modules, drone fleet and pending/completed lock lists. Old singleton lock/drone fields mirror compatibility state. `FlightState` uses f64 position, velocity, heading, actual acceleration, quaternion `[x,y,z,w]`, angular velocity, navigation mode, update timestamp and optional durable warp trajectory. Body axes are +X forward, +Y up, +Z right. Shared trajectory helpers exist so clients can present continuous travel without inventing server positions.

`assets` contains authenticated inactive hulls and their cargo/power, personal storage by system, membership-visible corporation storage, and invitations. Active hull cargo remains `PilotView.ore`. Remote control does not impersonate the worker's account or grant this asset view.

`ledger` is `LedgerExport { protocol_version, entries, snapshot }`. Each entry contains sequence, ruleset version, timestamp, previous hash, economic event and hash. Hash encoding is the ordered tuple implemented by `LedgerEntry::calculate_hash`, not arbitrary JSON object text. Events cover creation, extraction, escrow/trades, cargo swaps, storage and corporations. Use `arkline-verifier` rather than assuming a matching final balance validates history. An unsigned hash chain plus a locally pinned checkpoint cannot establish the honesty of the first history or physical mining truth.

## Publishing and extending the contract

This reference and the Rust protocol are the current API publication. There is no generated OpenAPI service, hosted developer key portal, stable SDK or OAuth token endpoint in the game server. Adding those labels to a website would not create those capabilities. Changes should include Serde compatibility tests, transport tests and independent economic replay where relevant. Account integration is tracked separately in the [identity plan](identity-plan.md).
