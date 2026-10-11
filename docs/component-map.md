# Component and deployment map

This describes the implemented Rust preview, not a distributed-universe promise. Source contracts are in [`arkline-protocol`](../crates/arkline-protocol/src/lib.rs); the [API reference](api.md) explains the wire format. Build identity comes from `BUILD_VERSION`, separately from Cargo package version and protocol version.

## Repository responsibilities

| Path | Responsibility | Trust boundary |
| --- | --- | --- |
| `crates/arkline-protocol` | Serializable requests, responses, snapshots, economic events; shared ship, universe, trajectory and station geometry definitions | Shared vocabulary, not permission to mutate the world |
| `apps/server/src/lib.rs` | Authenticated sessions, world loading/migration, validated command dispatch, SQLite transactions and ledger | Authoritative decisions and durable state |
| `apps/server/src/main.rs`, `websocket.rs` | TCP and optional WebSocket listeners, connection lifecycle, admission limits, health | Untrusted network input |
| `apps/server/src/flight.rs`, `universe.rs` | Six-axis flight, navigation, warp and connected systems | Client input requests acceleration; server owns position |
| `apps/server/src/mining.rs`, `drones.rs`, `power.rs` | Timed extraction, drone movement/mining/combat, capacitor accounting | Awards and costs are computed by the server |
| `apps/server/src/assets.rs` | Hull cargo, personal storage, corporations and shared storage | Session identity and membership checks |
| `apps/server/src/resource_jobs.rs`, `remote_control.rs` | Gathering jobs and explicit delegated control | Uses gameplay validation; delegation is not account ownership |
| `apps/server/src/station.rs`, `surface.rs`, `traffic.rs` | Private hangar/quarters movement, owned surface props, deterministic ambient traffic | Traffic is scenery, not an economic account |
| `apps/client/src/main.rs` and UI modules | Native and browser cockpit, inventory, jobs and station controls | Presents snapshots and sends requests |
| `apps/client/src/flight3d.rs`, `hangar.rs`, `world_visuals.rs`, `motion.rs` | Rendering, meter-scale geometry and buffered/analytic presentation | Interpolation is visual, never server state |
| `apps/client/src/transport.rs`, `browser.rs`, `pilot_input.rs` | Native TCP/WSS, browser WebSocket, latest control samples | Authentication and uncertain-response recovery |
| `apps/client/src/sound_engine.rs`, `sound_scene.rs` | Synthesized audio and authoritative event/loop planning | Effects do not award resources or confirm failed actions |
| `apps/client/src/ship_ai.rs`, `browser_ship_ai.rs` | Ship-computer intent integration | Requests existing commands; cannot write balances |
| `apps/cli` | Scriptable protocol client | Same authoritative command checks |
| `apps/verifier` | Independent economic replay, conservation and pinned checkpoints | No server crate or database credentials |
| `tools`, `.github/workflows` | Packaging, validation, self-hosted CI and controlled publication | Release administration, separate from gameplay |
| `deploy` | Container, Kubernetes and operator documentation | One authority per persistent world |

The website is a separate Phoenix application, Bastion. It serves community pages and download links. Its account sessions are not presently game credentials. The Rust browser client is compiled to WASM and served as static assets; it is not a second server implementation.

## A command through the system

```text
Native cockpit / browser WASM / CLI
       | JSON request, authenticated connection
       v
TCP or WebSocket transport -- bounds / deadlines / session cleanup
       |
       v
One serialized authoritative world
       | validate permission, location, range, timing, capacity, power
       v
SQLite transaction: world change + economic event when applicable
       |
       +--> response snapshot + authenticated asset view
       +--> ordered hash-chained economic export --> independent verifier
```

The simulation advances at 10 Hz and also advances through request handling. Movement, lock deadlines, mining cycles, drone activity and jobs share server time. Economic changes commit with their ledger events. The ledger is an economic audit history, not a complete replay of flight, combat or every command. See [architecture](architecture.md) for the conservation equation and verifier limitations.

A snapshot includes registered pilots; `online_pilots` is separate transport presence. An offline ship can still exist in space. Private asset detail is scoped to the authenticated pilot, but public economic events reveal transfers and membership: this is not a confidential economy.

## Local and hosted layouts

```text
Local computer
  native client / CLI -- 127.0.0.1 TCP --> server -- SQLite + ownership lock
  optional browser -------- WebSocket --> same server

Hosted platform
  Internet HTTPS/WSS
        |
  Cloudflare edge + dedicated tunnel
        |
        +-- website domains ------> Bastion Service -> Phoenix + its account data
        +-- play / downloads -----> static web Service -> WASM + release files
        +-- relay /ws ------------> production Service -> one server -> prod PVC
        +-- test-relay /ws -------> test Service ------> one server -> test PVC
```

The current hosting platform is the dedicated Talos Kubernetes cluster reachable through Fleet administration, not the retired managed-cluster context. Production and test have separate StatefulSets, Services and SQLite volumes. Their accounts and economies are independent. Raw TCP stays private; the public edge reaches WebSocket origins. Exact rollout revisions and backup evidence belong in deployment reports, not hard-coded architecture claims.

Kubernetes schedules and restarts the authority; it does not distribute the simulation. All systems in one world run in one process and database. Do not scale a world StatefulSet above one replica, share its SQLite file among authorities, or treat a second server as a synchronized replica. A lifetime database ownership lock prevents accidental local double hosting. Persistence is not high availability: recovery still depends on volume availability and validated backups. See [deployment instructions](../deploy/README.md), [runner trust and scoped deployment](../deploy/CI_RUNNERS.md), and [publication procedure](../tools/ci/PUBLISH.md).

## Implemented boundaries and future work

Implemented: authoritative flight, durable travel, timed locks, fixed-yield mining, finite ore, escrow market, storage, corporations, drones, constrained surface props, private station walking, delegated jobs/control, browser/native clients and independent economic verification.

Not implemented: multi-authority system handoff, permissionless validator consensus, node uptime payments, signed public witness network, globally confidential economy, unrestricted creator scripting, production account recovery or shared website/game OAuth login. Community contributions can improve the code and inspect exports today; operating an arbitrary node does not grant official authority or rewards. [Identity integration](identity-plan.md) separates existing account capabilities from the proposed sign-in work.
