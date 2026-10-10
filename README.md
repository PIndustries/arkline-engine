# Arkline

A Rust space-universe prototype by P.Industries. Pilots share a server-authoritative
universe of three connected test systems, mine finite ore, trade through escrow, and independently audit the economy.

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
2. Undock one pilot, warp to the belt, select an individual asteroid, lock it,
   then extract up to 100 ore.
3. Warp back to the station and dock.
4. List 60 ore at 7 credits each. From the other pilot, buy 35.
5. Observe the remaining 25 ore in escrow and the updated balances.
6. Close and restart the server, reconnect both pilots, and inspect the saved state.
7. Cancel the remaining order to return its ore to the seller's cargo.

Warp includes a two-second alignment/warm-up, visible local launch, acceleration, cruise, deceleration and final approach; travel time depends on distance. Routes over 300 km reserve six seconds each for the first and last 30 km. Mining is limited to 100 units per command and a
one-second cooldown. Cargo holds 1,000 units; escrow is separate. Cancelling an
order requires room to return its ore. The initial global reserve is 100,000 ore, divided across 108 rocks in three systems.

## Views and direct flight

Use **2D Tactical**, **3D External**, or **Cockpit** in the header to switch views.
They share selection, authoritative position, navigation and target lock. Cockpit
shows live ship instruments and follows the complete ship attitude, including roll.
The external view lets you orbit the ship to inspect its hull and turret mounts.
Direct flight moves the authoritative ship in all six degrees of freedom.

| Controls | Action |
|---|---|
| W / S | Forward / reverse thrust |
| A / D | Strafe left / right |
| Space / C | Translate up / down |
| Arrow keys | Pitch and yaw |
| Q / E | Roll left / right |
| Hold right mouse + move | Aim from unobstructed space |
| Ctrl+Space | Brake |

Release controls to stop rotation and apply assisted braking. Controls are disabled
while typing, unfocused, docked or traveling in warp. A short server input lease
prevents lost input from leaving thrust or rotation active. Left-click objects to
select them; overview actions still provide approach, orbit, warp, mining and docking.

Locking an asteroid starts a server-timed acquisition, lasting 2–8 seconds according
to the hull's sensors and target radius. The interface shows acquisition progress;
mining becomes available only after lock confirmation and within mining range.
Cancel/unlock releases the target. Retargeting starts a new acquisition, and leaving
range, warping, docking or jumping invalidates the target. A pending acquisition
is cancelled on server restart or loss of the pilot's last connection.

Hull-mounted mining turrets track the acquiring/locked asteroid. Confirmed
extraction produces a beam from the turret muzzle and impact feedback. Warp effects follow
the authoritative route. These effects do not issue extra mining commands or change
inventory. Procedural cockpit/world assets remain a prototype.

## Ships and physical scale

Open **Hangar** in the services strip while docked to board a preview hull. The
server saves your selection; all clients render the same hull at its physical size.

| Hull | Length | Width | Height |
|---|---:|---:|---:|
| Pioneer | 72 m | 38 m | 18 m |
| Prospector | 180 m | 92 m | 64 m |
| Hauler | 420 m | 180 m | 140 m |
| Vanguard | 1,100 m | 380 m | 260 m |

Arkline Station spans 20 km and stands 6 km high. Its navigation marker identifies
the docking port; docking range is measured to that port, not the station center.
Hull dimensions and world distances use meters, with one render unit representing
100 meters after camera-relative rebasing. Hulls have distinct procedural shapes.
The preview hangar does not charge credits; boarding preserves cargo and economic
history. All four hulls currently share flight tuning and cargo capacity. Ship
production, hull ownership markets and physical collisions are not implemented.

## Cockpit layout and services

Space fills the entire gameplay window in all three views. Session controls, overview,
ship controls and camera controls float above it instead of reserving fixed rows
or columns. Move or collapse panels to make room around your ship. Use **F11**
or **Fullscreen** to toggle fullscreen mode. Services open
in a floating window only when requested. Foreground panels consume pointer input
so manipulating a panel does not issue a navigation command behind it.


The right-hand overview lists local stations, belts, individual asteroids, gates
and ships, with category filters and distances. Its selected-object panel gives
the available navigation, targeting, mining and jump actions. Pilot records include live connection status. Undocked ships remain in space
after disconnect; docked ships are omitted from the space view. A floating ship HUD keeps cargo, balance and flight controls visible.

Use the service launcher to inspect **Cargo**, trade in **Exchange**, review
**Activity**, independently check the economic history in **Ledger**, or save an Emerald, Teal
or Amber appearance in **Settings**. Emerald uses the default black-and-green theme. Cargo
separates held ore from sell-order escrow. Exchange actions require docking.

The Ledger audit runs separately from the flight connection and pins a local
checkpoint after successful replay. Its result describes the exported history at
the time of that audit; later transactions require another audit. Keep checkpoint
files with your client data to retain rollback detection across restarts.

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

The [ecosystem milestones](docs/ecosystem.md) describe the next complete player,
creator, operator and community-observer journeys and their acceptance checks.

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

## Build identity and navigation

The header and window title show the same `year.month.day.build` identity as
`BUILD_VERSION` (for example, `26.10.9.1`). Increment the last component for each
new packaged build on a day; start at 1 on a new date. Packaging reads this file
and includes the original Arkline orbital icon.

Click a station, asteroid or visible ship through the cockpit to select it.
The overview shows contextual actions. Clicking an asteroid selects that individual
rock and its remaining ore. Use **Approach**, **Orbit**, or **Keep range** for
automatic navigation, or use W/S, A/D and Space/C for direct body-axis thrust.
Hold the right mouse button to steer; arrow keys pitch/yaw and Q/E roll the ship.
Asteroid extraction requires locking that rock first. The Travel overview contains
gates connecting Junction, Ember Reach and Glassway; warp to a gate and jump when
in range. **Ctrl+Space** brakes to a stop. Cockpit orientation follows the actual
ship attitude. See [flight controls](docs/flight-controls.md) for limits.

Online status counts live authenticated connections separately from durable ships.
Explicit disconnects are removed on socket closure; an unresponsive network may
remain online until the server's 120-second read timeout. Server restart clears
presence, while saved ships and cargo remain. The server remains one authority;
automatic clustering and ship-computer execution are not implemented in this build.
See [legacy system findings](docs/legacy-systems.md) for the source-grounded port plan.
