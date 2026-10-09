# Arkline

A Rust space-universe prototype by P.Industries. Pilots share a server-authoritative
region, mine finite ore, trade through escrow, and independently audit the economy.

**Private validation preview, v0.1.0.** Keep this repository private until the owner
has played the model, validated the direction, and explicitly approved publication.
An open-source license has not been selected. This preview is not a public MMO or a
financial network.

## Run the model

For the packaged macOS preview, open `Arkline.app`. It starts a private local
server automatically and stores its world and credentials in
`~/Library/Application Support/Arkline/Preview`. Use **Open second cockpit** to
join that same world as another pilot. Closing the hosting cockpit stops its
server; reopening it restores the saved world. The preview bundle is ad-hoc signed,
not Developer ID signed or notarized.

For source development:

Install Rust through rustup. The repository pins Rust 1.89.0. From the repository:

```sh
cargo build --workspace --locked
```

In one terminal:

```sh
cargo run -p arkline-server -- --bind 127.0.0.1:7777 --db data/arkline.sqlite
```

In a second terminal:

```sh
cargo run -p arkline-client
```

Enter a new pilot name in the cockpit. Names use 1–24 letters, numbers, underscores,
or hyphens. Start another client with a different name to join the same world.
The client saves a private resume credential under `data/client/`; keep it to resume
that pilot. The command client has a separate credential store. Do not use the same
pilot name in both clients unless you deliberately transfer its credential.

## First flight

1. Create two pilots, each issued 1,000 prototype credits.
2. Undock one pilot, warp to the belt, then mine 100 ore.
3. Warp back to the station and dock.
4. List 60 ore at 7 credits each. From the other pilot, buy 35.
5. Observe the remaining 25 ore in escrow and the updated balances.
6. Close and restart the server, reconnect both pilots, and inspect the saved state.
7. Cancel the remaining order to return its ore to the seller's cargo.

Warp takes three seconds. Mining is limited to 100 units per command and a
one-second cooldown. Cargo holds 1,000 units; escrow is separate. Cancelling an
order requires room to return its ore. The initial belt holds 100,000 ore.

## Tactical and flight views

Use **2D Tactical / 3D Flight** at the top of the cockpit. Both display the same
live ships and authoritative warp timeline; switching views changes no game state.
The 3D mode uses native GPU geometry and perspective lighting with procedural ship,
station, asteroid and planet assets.

- Drag inside the 3D viewport to orbit the camera.
- Scroll inside it to zoom.
- **Focus my ship** tracks your pilot; **Sector overview** frames the region.
- **Reset camera** restores the default viewpoint.

Flight buttons still control travel. The camera does not move your ship or affect
the economy. This is an early 3D rendering foundation with procedural assets, not
the final art direction or a full free-flight/combat simulation.

## Independently audit the economy

```sh
cargo run -p arkline-verifier -- --server 127.0.0.1:7777 --save data/audit.json --checkpoint data/checkpoint.json
cargo run -p arkline-verifier -- --file data/audit.json --checkpoint data/checkpoint.json
```

The verifier replays the exported history without database access or server code.
It checks hash linkage, quantities, escrow, balances, resource conservation, and
agreement with the exported state. A saved checkpoint detects a later rollback or
rewrite of history already observed by that verifier. Preserve it independently.

This proves consistency of the supplied economic records. It does not prove mining
actually happened, validate combat, authenticate a server operator, or prevent an
operator presenting separately consistent histories to different first-time observers.
There is no signature, consensus system, reward token, or permissionless hosting yet.

## Command client

```sh
cargo run -p arkline-cli -- --help
cargo run -p arkline-cli -- --pilot TerminalPilot connect
cargo run -p arkline-cli -- --pilot TerminalPilot undock
cargo run -p arkline-cli -- --pilot TerminalPilot warp belt
# After arrival:
cargo run -p arkline-cli -- --pilot TerminalPilot mine 100
```

## Repository

```text
apps/client/             Native Rust cockpit and live space view
apps/server/             Authoritative simulation, SQLite transactions, TCP
apps/verifier/           Independent economic replay and checkpoints
apps/cli/                Scriptable Rust command client
crates/arkline-protocol/ Versioned commands, snapshots, economic events
docs/                    Architecture, migration rationale, validation
```

This first workspace implements a small complete journey. Editor, advanced engine,
content, and community-node crates will be introduced when they have functioning
behavior. Rendering currently uses eframe/egui; the game rules and server are ours.

## Check the build

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The server's black-box journey test starts two actual TCP clients, mines and trades,
kills the server, restarts it, checks durable state and credentials, and runs the
independent verifier. Other tests exercise concurrent fills, rejected operations,
tampered exports, and checkpoint rollback. CI runs on macOS and Linux.

## Operating boundary

Use loopback for the preview. TCP is not encrypted. Remote experiments require an
authenticated encrypted tunnel; direct Internet exposure is not supported. Exports
and snapshots are readable without login and contain all prototype pilot balances.
One serialized server and one SQLite world are intentional limitations. Starter
issuance per pilot is not resistant to multiple identities. Credits have no monetary
value. Operator rewards and hosting governance require separate reviewed designs.

If a connection drops during a command, its outcome may be unknown. Check the
reconnected state before repeating economic actions; command deduplication is not
implemented yet.

All first-party application logic is Rust. Dependencies, operating-system APIs,
SQLite's implementation, and build/packaging configuration are not themselves all Rust.
