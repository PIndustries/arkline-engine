# Player and developer guide

This guide preserves the detailed preview, local-build and gameplay instructions moved from the repository README. Commands run from the repository root unless noted. Dated sections retain their original validation boundaries; consult the release notes for the exact downloadable build.

## Current preview

Docking opens a [station hangar](station-hangar.md) with a correctly scaled
ship and an animated on-foot pilot. Walking uses a continuous full gait cycle,
with alternating foot contact/lift and a smooth return to idle. Leave the ship, walk around its berth, then
return to the boarding marker before entering and undocking. A connected captain’s
room adds furniture, live operational displays and in-world station terminals. See the
[stability audit](stability-audit.md) for remaining foundation gaps.
On foot, WASD and arrow keys move the pilot, and Space jumps. Jump height and landing
are controlled by the server; boarding and seating wait until the pilot is grounded.

The ship computer retains short conversations and combines follow-up instructions
with current ship telemetry. Gathering starts from space as well as a station;
warp orders wait for arrival and existing cargo counts toward the haul. Goals
above hold capacity automatically repeat mining and delivery trips. Mining and attack drones orbit their targets; owned
drones also accept explicit orbit/hold orders. Flight has cargo-dependent
acceleration and gradual braking, with thrusters driven by measured acceleration.
Hold Shift and use arrow keys to look around the cockpit; release to recenter.
The 2D map supports cursor-centered wheel zoom and empty-space drag panning.

[Game sound](sound.md) follows confirmed machinery and lifecycle events,
with volume, mute, test and retry controls in Settings. Directional lighting,
material highlights and tone mapping improve the existing renderer; this is not
HDR-display output. Asteroids rotate slowly with matching selection and mining
impact geometry. The sound mix uses layered machinery harmonics, distinct mining start/stop
feedback and a connected warp charge, entry and arrival sequence. See [validation evidence](validation.md) for tested
behavior and remaining owner acceptance.

Drone screens provide Raised, Top down, Orbit and POV angles, plus up to four
individual feeds per page. Cached bounded-resolution feeds and smoothed camera
poses reduce repeated rendering and snapshot stepping.

## Models and community artwork

Current ships, stations, drones and scenery use lightweight geometry generated
in Rust. There is no GLB/glTF asset importer yet. GLB is the intended interchange
format for a future community model pipeline; see [model contributions](models.md)
for the current boundary and proposed asset requirements.

## Run the model

Open `Arkline.app` on macOS or run `arkline-client.exe` on Windows. The client
defaults to the official Production relay. Production and Test have separate pilots,
credentials, and economies; Test progress does not transfer to Production.
The browser client uses the same Rust game at `https://play.arkline.network`.
Browser capabilities and build instructions are in [the browser guide](browser.md).

To host a private local universe, use `arkline-client --local`. Closing its hosting
cockpit stops the local server; reopening it restores the world. Credentials must
be retained to resume a pilot. The macOS bundle is ad-hoc signed, not notarized;
Windows binaries are unsigned and need the x64 Microsoft Visual C++ v14 runtime.

For source development:

Install Rust through rustup. The repository pins Rust 1.89.0. On Ubuntu, install
the native window, graphics and audio development packages first:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxi-dev libxrandr-dev libxcursor-dev libgl1-mesa-dev libasound2-dev
```

Then, from the repository:

```sh
cargo build --workspace --locked
```

In one terminal:

```sh
cargo run -p arkline-server -- --bind 127.0.0.1:7777 --db data/arkline.sqlite
```

In a second terminal:

```sh
cargo run -p arkline-client -- --server 127.0.0.1:7777 --remote-only
```

Enter a new pilot name in the cockpit. Names use 1–24 letters, numbers, underscores,
or hyphens. Start another client with a different name to join the same world.
The client saves a private resume credential under `data/client/`; keep it to resume
that pilot. The command client has a separate credential store. Do not use the same
pilot name in both clients unless you deliberately transfer its credential.

## First flight

1. Create two pilots, each issued 1,000 prototype credits.
2. Undock one pilot, warp to the belt, select an individual asteroid, lock it,
   wait for confirmation, then activate a mining module. Let it collect at least 60 ore and stop it.
3. Warp back to the station and dock.
4. List 60 ore at 7 credits each. From the other pilot, buy 35.
5. Observe the remaining 25 ore in escrow and the updated balances.
6. Close and restart the server, reconnect both pilots, and inspect the saved state.
7. Cancel the remaining order to return its ore to the seller's cargo.

Warp includes a two-second alignment/warm-up, visible local launch, acceleration, cruise, deceleration and final approach; travel time depends on distance. Routes over 300 km reserve six seconds each for the first and last 30 km. Each ship has two independently activated mining modules. They repeat server-timed cycles with a fixed hull-defined yield; players do not enter extraction quantities. Cargo capacity depends on the hull (1,000 units on Pioneer); escrow is separate. Cancelling an
order requires room to return its ore. The initial global reserve is 100,000 ore, divided across 108 rocks in three systems.

## Views and direct flight

Use **2D Tactical**, **3D Tactical**, **3D External**, or **Cockpit** in the header or cockpit Settings to switch views.
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
Up to five pending/confirmed asteroid locks coexist. The target rack selects which
confirmed lock receives the next turret activation; each turret keeps its own assignment.
Unlocking one target affects only that target and its modules. Leaving range, warping,
docking or jumping invalidates affected targets. A pending acquisition
is cancelled on server restart or loss of the pilot's last connection.

Hull-mounted mining turrets independently track their assigned asteroids. Each active module
shows its cycle and a beam from its own turret muzzle; completed cycles produce
confirmed extraction feedback. Warp effects follow
the authoritative route. These effects do not issue extra mining commands or change
inventory. Procedural cockpit/world assets remain a prototype.

The five mining tiers yield 25/6s, 40/5s, 75/5s, 100/4s and 100/3s per module,
respectively; the hull table below gives names and capacities. Hauler yields 20/8s
and Vanguard 30/7s.
Only the terminal cycle can yield less, when the asteroid empties or cargo fills.
Stopping cancels the unfinished cycle without a partial award. Loss of lock or
range, warp, docking, jumping, depletion, full cargo, last disconnect and server
restart stop modules; they do not resume automatically. Stalled cycles never
produce a backlog of offline ore.

## Ships and physical scale

Open **Ship** in the cockpit console (or **Hangar** from the service rail) while docked to board an owned preview hull. The
server saves your selection; all clients render the same hull at its physical size.

| Hull | Mining tier | Length × width × height | Cargo ore | Ore / cycle per turret |
|---|---:|---|---:|---:|
| Pioneer | 1 | 72 × 38 × 18 m | 1,000 | 25 / 6 s |
| Prospector | 2 | 180 × 92 × 64 m | 2,500 | 40 / 5 s |
| Borealis | 3 | 640 × 260 × 180 m | 8,000 | 75 / 5 s |
| Stratum | 4 | 1,800 × 800 × 480 m | 24,000 | 100 / 4 s |
| Monolith | 5 | 5,200 × 2,200 × 1,400 m | 80,000 | 100 / 3 s |
| Hauler | Utility | 420 × 180 × 140 m | 16,000 | 20 / 8 s |
| Vanguard | Utility | 1,100 × 380 × 260 m | 4,000 | 30 / 7 s |

Both turrets use the listed per-turret rate while their targets, cargo room and
capacitor permit extraction. Hull names and rates are prototype specifications.

Arkline Station spans 20 km and stands 6 km high. Its navigation marker identifies
the docking port. Docking requires at most 1.8 km from the hull's conservative
enclosing sphere to that port, rather than from the ship center. The sphere radius
is half the hull diagonal; this is an orientation-independent interaction rule,
not collision geometry. Berths scale with hull size and keep the hull outside the
station front plane.
Hull dimensions and world distances use meters, with one render unit representing
100 meters after camera-relative rebasing. Hulls have distinct procedural shapes.
The preview grants one hull of each class once. Inactive hulls stay at their station;
boarding requires docking in the same system. Each hull keeps its own cargo, so
boarding an empty ship leaves the previous ship's ore aboard that stored hull.
Existing saves retain the active hull and cargo; the other preview hulls start at
Junction. The seven hulls currently share flight tuning, but have different cargo,
mining and reactor specifications, procedural hulls and cockpit layouts. Ship
production, multiple hulls of the same class, hull trading and general physical
collisions are not implemented.

## Planet approach and first surface construction

Verdant is the home system's landable prototype planet, with a 6,000 km radius and
60 km atmosphere boundary. Start near Junction station: choose Verdant in **Flight**
→ **Travel**, then **Approach planet**. This sends ordinary continuous flight to a
point about 5 km above the spherical surface. The station is about 120 km above
the ground, outside the atmosphere; approach takes roughly three minutes at the
preview ship speed. Return to the home station first if
you are at the distant belt or in another system; planet approach is not an AU-scale
warp route or an instant landing command. From the approach point, descend manually,
watch the server-reported altitude and hull ground clearance, then brake near the
surface. Translation axes follow your ship's attitude.

Open **More → Surface sandbox** at no more than 100 m ground clearance to place
crates, pylons or platforms. Choose an east/north offset; the server anchors the
prop to the spherical ground. Select an owned prop to move or delete it within
500 m of the hull envelope. Each pilot may own 32 props, with a world limit of 256.
Props and their ownership persist. The server constrains the ship against the
sphere using its oriented hull support; this is first construction support, not
walking, terrain deformation, a general collision/rigid-body system or a complete
planetary environment. The visual terrain is procedural, and no new economic yield
is created by props.

Shuttles, freight tenders and patrols provide server-timed background traffic near
stations. They are decorative service actors, not online players or economic NPCs:
they do not mine, trade, consume stock, create credits or change the ledger.

## Station assets and corporations

Docking opens **Station** services with the active vessel, **Undock**, and shortcuts
to **Hangar**, **Inventory**, **Corporation**, and **Exchange**. The station interior
contains a private berth and connected captain’s room, with service terminals
opening the same functional panels. It is not a shared station concourse.

**Inventory** is available in space as well as while docked. It shows active cargo,
stored hull cargo, personal station ore, and market escrow. You can inspect remote
holdings, but personal deposits and withdrawals require docking at the station
holding the ore. Boarding does not teleport ships or cargo between systems.

Create a corporation, invite an existing pilot, and have that pilot accept the
invitation. Members can inspect shared station storage and deposit ore while
docked; only the owner can withdraw. Transfers use the current station and obey
the active cargo capacity. Membership and storage survive server restart. This is
the first corporation implementation: custom roles, shared ships, corporate wallets,
member removal, ownership transfer, and corporation dissolution are not included.

Inventory currently contains ore and seven preview hull classes. General item
types, fitting, blueprints, manufacturing and repair services remain future work.

## 3D tactical camera

Choose **3D Tactical** in the view selector for a wide-angle, ship-centered 3D
range overlay. Drag empty space to orbit and scroll to zoom. At the default zoom,
rings mark 5, 10, 15 km and onward; close/far zoom uses 1/25 km spacing to keep the
plane readable. Ring dimensions use the same meter coordinates as ships and objects.
The selected object has a horizontal range guide and a vertical height line; its
label reports the full 3D center-to-center distance and signed height. Module range
checks continue to use the server's surface-distance rules.

**Flight controls** can hide the overlay or reset the tactical camera. The tactical
camera follows your ship through movement and warp and retains its own zoom/orbit
when switching to external or cockpit views. **2D Tactical** remains the local/system
map. This version anchors the tactical camera to your ship; detached panning and
linked-window groups are not implemented.

## Cockpit layout and services

Space fills the entire gameplay window in all four views. Cockpit operations live
on the physical console; overview and ship/camera overlays are hidden there. Other
views use movable, collapsible overview, ship and camera panels, and requested
services open in a floating window. Use **F11** or **Fullscreen** to toggle fullscreen. Foreground panels consume pointer input
so manipulating a panel does not issue a navigation command behind it.

Drag panel titles and release near a viewport edge or center to snap. Panels keep
their edge-relative placement when resizing or switching fullscreen, and save the
layout locally. **Settings** provides **Pin panels**, **Snap panels**, and
**Reset panel layout**. The header reports measured FPS and frame time; rendering
is throttled while the window is unfocused.

The overview, available in cockpit Flight or a floating panel, lists local stations,
belts, individual asteroids, gates, planets, props and ships, with category filters and distances. Its selected-object panel gives
the available navigation, targeting, mining and jump actions. Pilot records include live connection status. Undocked ships remain in space
after disconnect; docked ships are omitted from the space view. The compact HUD in other views and the physical cockpit instruments show cargo, balance and flight controls.

Use the service launcher to inspect **Inventory**, trade in **Exchange**, review
**Activity**, independently check the economic history in **Ledger**, or save an Emerald, Teal
or Amber appearance in **Settings**. Emerald uses the default black-and-green theme.
Inventory separates held ore from station storage and sell-order escrow. Exchange
actions require docking.

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
cargo run -p arkline-cli -- --pilot TerminalPilot lock 1
cargo run -p arkline-cli -- --pilot TerminalPilot mine 30  # activation seconds, fixed yield
# Once back at the station and docked:
cargo run -p arkline-cli -- --pilot TerminalPilot deposit 50
cargo run -p arkline-cli -- --pilot TerminalPilot board prospector
cargo run -p arkline-cli -- --pilot TerminalPilot inventory
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

The [ecosystem milestones](ecosystem.md) describe the next complete player,
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
The detailed assets response requires login and is scoped to the pilot and their
corporation. The public audit history still reveals economic storage movements;
this is not a private economy.
One serialized server and one SQLite world are intentional limitations. Starter
issuance per pilot is not resistant to multiple identities. Credits have no monetary
value. Operator rewards and hosting governance require separate reviewed designs.

If a connection drops during a command, its outcome may be unknown. Check the
reconnected state before repeating economic actions; command deduplication is not
implemented yet.

All first-party application logic is Rust. Dependencies, operating-system APIs,
SQLite's implementation, and build/packaging configuration are not themselves all Rust.

## Build identity and navigation

The header and window title show the `year.month.day.build` identity stored in
`BUILD_VERSION` (for example, `26.10.9.1`). Allocate it with
`python3 tools/build.py -- <cargo command>`. The wrapper uses the America/Chicago
calendar date, increments the build number for each invocation that can produce
binaries, and holds a local file lock until Cargo exits. The local macOS package
scripts also stage the exact binaries and version while that lock is held.
Release archives use the committed version to keep their tag, source commit and
package metadata aligned. Direct `cargo` commands bypass allocation; separate
checkouts and hosts do not coordinate. The package includes the original Arkline
orbital icon.

Click a station, asteroid or visible ship through the cockpit to select it.
The overview shows contextual actions. Clicking an asteroid selects that individual
rock and its remaining ore. Use **Approach**, **Orbit**, or **Keep range** for
automatic navigation, or use W/S, A/D and Space/C for direct body-axis thrust.
Hold the right mouse button to steer; arrow keys pitch/yaw and Q/E roll the ship.
Asteroid extraction requires locking that rock first. The Travel overview contains
gates connecting Junction, Ember Reach and Glassway; warp to a gate and jump when
in range. **Ctrl+Space** brakes to a stop. Cockpit orientation follows the actual
ship attitude. See [flight controls](flight-controls.md) for limits.

Online status counts live authenticated connections separately from durable ships.
Explicit disconnects are removed on socket closure; an unresponsive network may
remain online until the server's 120-second read timeout. Server restart clears
presence, while saved ships and cargo remain. The server remains one authority;
automatic clustering and ship-computer execution are not implemented in this build.
See [legacy system findings](legacy-systems.md) for the source-grounded port plan.


### Cockpit instruments and connection recovery

Cockpit instrument text uses the physical display faces for positioning, clipping,
and sizing. The center display reports speed, body-axis velocity, heading, pitch
and navigation mode. Side displays show server-reported local contacts, selected
center range, lock state, mining cycles, cargo capacity and credits. These are live
simulation values. Capacitor power is modeled; heat and ship damage sensor models
are not implemented. Readings are marked stale when disconnected or snapshots stop
arriving. Cockpit mode hides the floating Ship controls panel.

The client preserves partial TCP responses across short read timeouts, and attempts
up to three saved-identity reconnects per session after a transport failure. Recovery
discards queued actions and flight input, refreshes authoritative state, and never
replays an action whose response was lost. If recovery fails, **Session → Reconnect**
starts a new attempt. An uncertain action remains flagged for inspection.


### Drone fleets and cockpit screens

The **Drones** console supports five recon, five mining and five attack drones per
pilot. Select a drone by its ID to navigate, hold, recall or assign it. Each has
server-owned position, signal strength and hit points. Mining drones approach a
confirmed asteroid lock and repeat an eight-second, ten-ore cycle into the owner's
cargo. Attack drones damage other pilots' deployed drones; ships and cargo are not
combat targets in this preview. Deployments are free test equipment, not a finished
drone manufacturing or replacement economy.

The 10 km control link weakens with distance. At signal loss, work stops and the
drone returns autonomously; recall remains available. Warp, docking, jump, final
disconnect and restart recover the fleet immediately under the current preview
policy. Individual recall normally travels home before recovery.

The left physical screen shows the selected drone's **POV** or **Orbit camera**.
Every drone can be selected; one live world-camera pass is rendered at a time.
Signal degradation adds noise and darkens the picture; a lost/stale link is labeled.
The cockpit includes a selectable 2D local map and a ship-relative 3D holographic
map with real-distance rings. Scroll over a map to change range, and click a contact
to select it. Ship orientation drives the map axes. The center console provides
Flight, Ship, Modules, Drones, Inventory, Station and AI, with remaining services and
view-mode Settings under More.
Cockpit field of view is 70 degrees. The slim service rail uses hover labels;
other view modes retain compact floating windows.

See [public preview readiness](public-preview-readiness.md) for remaining
release work and historical validation evidence. Public source availability does not imply production readiness.

### Ship power and clear cockpit actions

The cockpit console keeps **UNDOCK / DOCK / STOP** visible above every service tab.
Flight, Ship, Modules, Drones, Inventory, Station and AI are the main tabs; **More**
contains the remaining services. The larger screen uses current-window geometry,
including at the minimum supported window size.

Each hull has a server-owned capacitor (joules) and reactor output (watts). Warp
reserves its full energy cost at admission. Successful turret/drone mining cycles
and actual attack-drone shots consume energy; unfunded actions cannot create
ore or damage when the capacitor is empty. Mining modules stop with a low-power
reason, drones stop their assignment, and warp is rejected until sufficient charge
returns. The HUD shows authoritative charge, capacity, regeneration and action
costs; missing or disconnected power readings are marked accordingly.

Prototype rates are defined by `power_spec` in the protocol crate. Reactors remain
running while ships are parked/offline, bounded by capacitor capacity. Stored hulls
keep their own energy when boarding another ship. These are tunable gameplay
values, not a complete electrical/thermal simulation. Combat remains drone-only;
ship-mounted turrets currently mine rather than fire combat weapons.

### Ship computer and delegated resource jobs

The cockpit **AI** tab can perform one server-controlled mining haul on your own
ship or an existing worker that explicitly granted fleet control. It uses both
turrets and up to five mining drones, recalls them before return, then deposits
confirmed ore into that worker's station storage. Watch a permitted worker on the
left cockpit screen while retaining control of your own ship. Explicit **Take manual
control** switches flight to that worker; the persistent **Return to my ship**
control restores your own vessel without changing your player login.

Deterministic Start/Cancel controls work without a model. Optional typed Spark
instructions use SSH and a bounded intent adapter. The macOS package adds explicit
Talk/Finish/Cancel recording, local speech recognition and optional spoken replies. See
[resource jobs, delegation and remote viewing](resource-jobs.md) for setup,
readiness requirements and cancellation behavior.
