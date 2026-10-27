# Component and deployment map

This describes the implemented Rust preview, not a distributed-universe promise. Source contracts are in [`arkline-protocol`](../crates/arkline-protocol/src/lib.rs); the [API reference](api.md) explains the wire format. Build identity comes from `BUILD_VERSION`, separately from Cargo package version and protocol version.

## Repository responsibilities

| Path | Responsibility | Trust boundary |
| --- | --- | --- |
| `crates/arkline-protocol` | Serializable requests, responses, snapshots, economic events; shared ship, universe, trajectory and station geometry definitions | Shared vocabulary, not permission to mutate the world |
| `crates/arkline-protocol/src/navigation.rs` | Navigation vocabulary and existing warp sampling | Stable root exports, serialized tags/defaults and exact numerical behavior |
| `apps/server/src/lib.rs` | Authenticated sessions, world loading/migration, validated command dispatch, SQLite transactions and ledger | Authoritative decisions and durable state |
| `apps/server/src/main.rs`, `websocket.rs` | TCP and optional WebSocket listeners, connection lifecycle, admission limits, health | Untrusted network input |
| `apps/server/src/flight.rs`, `universe.rs` | Six-axis flight, navigation, warp and connected systems | Client input requests acceleration; server owns position |
| `apps/server/src/mining.rs`, `power.rs` | Timed extraction and capacitor accounting | Awards and costs are computed by the server |
| `apps/server/src/drones.rs`, `drones/` | Stable drone facade; command, lifecycle, motion and ordered simulation responsibilities | Combat, cargo loss/extraction and physical capture keep their original execution order |
| `apps/server/src/drone_bay.rs`, `drone_bay/` | Finite stored identities, serialized physical handling, durable deployment/recovery batches | Stored means completed physical capture; presentation cannot complete a route |
| `apps/server/src/drone_cargo.rs`, `drone_cargo/` | Drone-owned ore, incoming hold reservations, receiver routing/transfer, loss and job integration | Extraction, transfer and loss are distinct atomic ledger-backed changes |
| `apps/server/src/physical_cargo.rs`, `physical_cargo/` | Source-bound volume inspection and gated owner-bound single-bay controls using existing world/ledger transactions | Fresh ruleset 4 only; old units unchanged; explicit factory and existing Request allowlist, no public listener or general movement admission |
| `crates/arkline-protocol/src/physical_cargo/` | Exact geometry receipts, integer volume, authored catalog and physical ledger vocabulary | Geometry capacity is not density, engineering rating or gameplay admission |
| `crates/arkline-protocol/src/carrier_inventory_geometry/physical_variant.rs` | Exact new-source inspection geometry and typed ship/drone valve poses | Only pinned Mining bay 0/cassette 0 evidence; normal gameplay, generic service, staging and travel stay closed |
| `apps/verifier/src/physical_cargo.rs` | Independent volume-event replay | No server or account transition implementation; inspection controller events do not prove docking |
| `apps/server/src/assets.rs` | Hull cargo, personal storage, corporations and shared storage | Session identity and membership checks |
| `apps/server/src/resource_jobs.rs`, `remote_control.rs` | Gathering jobs and explicit delegated control | Uses gameplay validation; delegation is not account ownership |
| `apps/server/src/station.rs`, `surface.rs`, `traffic.rs` | Private hangar/quarters movement, owned surface props, deterministic ambient traffic | Traffic is scenery, not an economic account |
| `apps/client/src/main.rs` and UI modules | Native and browser cockpit, inventory, jobs and station controls | Presents snapshots and sends requests |
| `apps/client/src/flight3d.rs`, `hangar.rs`, `world_visuals.rs`, `motion.rs` | Rendering, meter-scale geometry and buffered/analytic presentation | Interpolation is visual, never server state |
| `apps/client/src/flight3d/geometry_catalog.rs` | Single ordered CPU geometry catalog, immutable source and material resolution | Shared upload/depth/ray indices and intentional legacy CPU lookup remain unchanged |
| `apps/client/src/flight3d/model_recipes.rs` | Ordered ship/cockpit recipes for seven classes, Pioneer fallback and station construction | Existing cache identity, material/part order and geometry remain unchanged; shared builders stay at their caller boundary |
| `apps/client/src/cockpit_panels.rs`, `cockpit_panels/` | Existing status/readouts, mining, navigation/contact overview and ledger/market panel implementations | Presentation retains original command guards; shared state and frame/dispatch ordering stay in lib.rs |
| `apps/client/src/cockpit_shell.rs`, `cockpit_shell/` | Session/settings chrome, cockpit action banks and physical-computer shell | Preserves privacy prompts, command guards, focus and teardown order |
| `apps/client/src/session.rs`, `session/` | Existing world admission, credential namespace/persistence, native exchange/recovery and owned local-host lifetime | Preserves validation-before-Ready, credential privacy and uncertain-action discard; transport codec unchanged |
| `apps/client/src/transport.rs`, `browser.rs`, `pilot_input.rs` | Native TCP/WSS, browser WebSocket, latest control samples | Authentication and uncertain-response recovery |
| `apps/client/src/sound_engine.rs`, `sound_engine/` | Stable audio vocabulary/facade, device ownership/recovery/callback and deterministic synthesis | Device output does not establish gameplay success; offline PCM is not a speaker test |
| `apps/client/src/sound_scene.rs`, `sound_scene/` | Confirmed-state lifecycle transitions and spatial listener/emitter planning | Stale, paused or repeated state must not invent/replay effects |
| `apps/client/src/ship_ai.rs`, `browser_ship_ai.rs` | Ship-computer intent integration | Requests existing commands; cannot write balances |
| `apps/editor` | Official content/project workspaces, embedded preview sessions, imports and authoring tools | Saved-world, unsaved-document and parent-input ownership remain explicit; preview editing is not client authority |
| `apps/asset-tool` | Offline bounded GLB submission inspection | Does not install or execute assets; validation is not gameplay adoption |
| `apps/cli` | Scriptable protocol client | Same authoritative command checks |
| `apps/verifier` | Independent economic replay, conservation and pinned checkpoints | No server crate or database credentials |
| `tools`, `.github/workflows` | Packaging, validation, self-hosted CI and controlled publication | Release administration, separate from gameplay |
| `deploy` | Container, Kubernetes and operator documentation | One authority per persistent world |

The website is a separate Phoenix application, Bastion. It serves community pages and download links. Its account sessions are not presently game credentials. The Rust browser client is compiled to WASM and served as static assets; it is not a second server implementation.

## Incremental source organization

Organization follows domain and responsibility, not one file per function. This
map remains the entry point; the dated acceptance archives and ship dossiers are
not disposable clutter and are not silently moved or deleted.

The first bounded cleanup extracts the existing drone facade into command,
motion, lifecycle and simulation responsibilities. External facade signatures and
call sites stay stable. The entire simulation advance remains one ordered routine:
simultaneous combat resolves before cargo loss/extraction, receiver completion and
bay capture. A file move must not change transaction ownership or execution order.
Physical bay and ore service remain distinct domains rather than being folded into
a generic drone file.

### Completed organization pass

Eight bounded responsibility extractions retain their existing caller paths:
server drone commands/lifecycle/motion/simulation; client audio device/synthesis
and scene planning; client session infrastructure; cockpit panel implementations;
cockpit shell/chrome; the shared geometry catalog; protocol navigation/warp math;
and static ship/cockpit/station model recipes. These are organization changes,
not new simulation, UI, audio, network, permission or ship-design behavior.

Each slice has a preserved baseline, source-equivalence review, applicable
focused checks and a full integrated gate. Audio has deterministic PCM parity;
UI/render changes have matched native evidence; geometry and navigation retain
ordered bit-level and serialized-contract comparisons. Exact source revisions and
acceptance counts belong in their dated receipts rather than becoming permanent
claims about every later commit.

### Physical layout and discoverability

The workspace already separates six applications under `apps/`, shared protocol
under `crates/`, authored data under `assets/`, documentation under `docs/`, build
and validation tools under `tools/`, and operations under `deploy/`. The seven
Cargo members remain unchanged. Responsibility folders now sit behind stable
facades such as `drones.rs`, `sound_engine.rs`, `session.rs` and `cockpit_shell.rs`;
not every small domain module needs its own folder.

Many client/server/editor domain files still live directly in `src/`; protocol
modules remain directly in its `src/` behind root re-exports. This pass is not a
claim that the entire repository has been converted to nested folders. Existing
editor `git_workflow/` and `project_workspace/`, server `drone_bay/` and
`drone_cargo/`, and client render/audio/panel domains are preserved rather than
renamed for visual uniformity.

Start with the [local documentation index](README.md). The [editor workspace
map](editor-workspace.md), [asset guide](models.md), [Pioneer source record](../assets/ships/pioneer/README.md),
and [ship-design archive](ships/README.md) describe their own domains. Authored
assets, source manifests, rejected studies, ship dossiers and recovery archives
retain their names, hashes and references; none is disposable cleanup material.

### Deliberately retained boundaries and deferred review

| Boundary | Decision in this pass |
| --- | --- |
| Server root transaction/dispatch coordination | Keep authority, transaction ownership and execution ordering together. Domain implementations remain separate. |
| Client root state, send/poll/readiness and App update | Keep shared-state and frame/command ordering coherent; moving presentation does not move authority decisions. |
| Editor main update | Keep the ordered input-blocking, preview polling and document ownership orchestration; it already delegates to domain modules. |
| Renderer frame/GL/resource lifetime | Keep draw/state order intact. Catalog and recipes are separated; further scene/frame separation needs dedicated evidence. |
| Embedded client facade and worker/profile lifetime | Still combines public embedding/view controls with session/profile lifetime. A future responsibility review is warranted; no move or policy change is claimed here. |
| Remaining protocol ship/economic vocabulary and wire envelope | Still shares the root alongside stable domain re-exports. Further extraction must preserve field order, defaults, public paths and economic encoding. |
| Independent verifier and small CLI/asset tools | Keep their cohesive trust boundaries; do not split into one file per function. |

This closes the scoped organization pass, not all future architectural work.
Remaining opportunities are explicit, not a justification for automatic rewrites.
Any later slice needs exact ownership, a reversible baseline, behavior-preserving
review and appropriate tests. Public wire names, save schemas, asset hashes and
include paths are not renamed merely to match a folder preference. New features
and official ship redesigns require their own scope and acceptance.

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

## Maintaining these boundaries

The [repository organization standard](repository-organization.md) distinguishes
machine-checked Cargo membership/location/direct dependencies from review guidance
for cohesive domains, stable facades, thin entry seams and explicit ownership.
Update this map when responsibility moves. Native client → server/verifier,
server → verifier and editor → client are intentional current dependencies;
independent verifier → server is not permitted. Source identity, usable cavities,
exclusions and explicit physical units remain part of the domain contract.

### Selected physical bay inspection

The physical cargo domain now contains a versioned, source-bound selected-bay
inspection executor and an independent verifier replay module. These reuse the
existing World JSON/SQLite/ledger transaction boundary; normal geometry and
transport admission remain closed. [Scope and fixture limits](physical-bay-inspection.md)
distinguish source-timed mechanics and volume conservation from authored mining,
power and rate inputs, fleet concurrency and playable acceptance.

### Source-bound one-bay controls

`physical_cargo/control/` adapts the proved single-bay reducer to existing
`Server::handle_at`/`tick_at`, `PilotView`, `DroneView` and `PowerState`. A fresh
owner-bound factory alone admits this fixed-world mode. The legacy `Asteroid.ore`
and ship ore fields remain zero; the additive response names cubic centimetres
explicitly. Real Pioneer capacitor joules are independently replayed under a new
control binding, never aliased to older inspection energy units. Public network
launchers, arbitrary targets, ship movement, fleets and normal client UI admission
remain separate gates. Frozen-host zero auxiliary load and the finite target/rates
are explicit scenario configuration, not measured engineering ratings.

### Physical bay client consumer

`apps/client/src/physical_cargo_ui.rs` validates the additive physical response,
retains explicit volume/energy presentation and bounds the existing command UI.
`physical_cargo_session.rs` connects the existing Cockpit to the same local
controlled authority without a network listener or generated credentials.
It owns bounded periodic observations, reliable ordered command responses and
worker shutdown; the Cockpit owns source validation and input/state revocation.
`physical_live_tests.rs` is the opt-in actual-EGL event-loop/pointer measurement
harness, separate from documentary PNG/GIF capture and OS-window acceptance.
`physical_source_scene.rs` resolves exact source geometry and confirmed machinery
for shared renderer color/depth/ray paths. Existing client/editor modes are not
replaced. See [physical client boundaries](physical-bay-client.md).
