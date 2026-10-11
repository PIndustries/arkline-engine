# Rust terminal client

The terminal cockpit is a playable interface to the same authoritative world as
the graphical client. It uses Rust, Ratatui and Crossterm. It is source work for
the next client release; the published 26.10.11.6 downloads do not contain it.

## Start

Build with `cargo build --locked -p arkline-cli`, then run:

```sh
# Official Production relay (TUI default)
cargo run --locked -p arkline-cli -- --pilot YourPilot tui

# Separate Test world
cargo run --locked -p arkline-cli -- --server wss://test-relay.arkline.network/ws --pilot YourTestPilot tui

# Local development server
cargo run --locked -p arkline-cli -- --server 127.0.0.1:7777 --pilot LocalPilot tui
```

Keep your credential file. The TUI stores credentials under `data/`, with a hash
of the exact endpoint and the pilot name in the filename. `--credentials PATH`
selects an existing compatible credential file explicitly. Credentials are not
interchangeable between Production, Test or independent worlds. Creating a new
name registers a new pilot; existing names require their saved resume credential.
Credential storage is checked before registration. If saving a newly issued token fails, keep the TUI open: it retains the token in memory while you fix the destination conflict and press R to retry. Quitting before the save succeeds loses that in-memory recovery state. Unix credential files are created with mode 0600. Use a private user directory
on Windows; this client does not configure Windows ACLs.

Remote connections use verified WSS certificates. Plain TCP and WS are restricted
to loopback. Existing script commands keep their JSON output and local default;
they now also accept a WSS server URL. Remote plaintext TCP is no longer accepted.

## Cockpit

Tab cycles Overview, Inventory, Fleet, Market and Navigation. Up/Down selects an
asteroid in the local system. PageUp/PageDown scrolls a long panel. `?` opens help,
`:` opens the command line, Escape cancels entry, and `q` or Ctrl-C exits.

- `u`: undock; `b`: warp to belt; `s`: warp to station; `d`: dock.
- `a`: approach selected asteroid; `l`: acquire its lock; Space: stop navigation.
- `1` / `2`: start miner 1 / 2 on the selected asteroid; `!` / `@`: stop it.
- `R`: reconnect using saved credentials after a connection loss.

Mining yield, locking time, travel, storage and trading are determined by the
server. Select an asteroid, approach within range, wait for the confirmed lock,
then start a miner. Mining repeats until stopped or the server stops the module.
Use the Inventory page to see confirmed personal and corporation storage.

Commands entered after `:`:

| System | Commands |
| --- | --- |
| Travel | `belt`, `station`, `gate ID`, `jump ID`, `approach`, `orbit METERS`, `range METERS`, `fly X Y Z`, `stop` |
| Targeting | `lock [ID]`, `unlock` |
| Mining | `mine 0`, `mine 1`, `stop-mining 0`, `stop-mining 1` |
| Storage and ships | `deposit N`, `withdraw N`, `board HULL` |
| Drones | `launch recon`, `launch mining`, `launch attack`, `recall ID`, `hold ID`, `drone-mine ID`, `drone-orbit ID`, `drone-attack ID PILOT TARGET_DRONE` |
| Resource jobs | `gather N`, `cancel-job` |
| Market | `sell UNITS PRICE`, `buy ORDER_ID UNITS`, `cancel-order ID` |
| Corporation | `corp-create NAME`, `corp-invite PILOT`, `corp-accept NAME`, `corp-deposit N`, `corp-withdraw N` |
| Station | `leave-ship`, `enter-ship`, `jump-foot` |

A command is pending until the server replies. On connection loss, its outcome
can be unknown: refresh after reconnect rather than blindly repeating it. The
client never automatically replays an uncertain mutation. Network operations run
outside the input/render loop, with bounded messages and socket deadlines.

## Current limits and validation

This is the first terminal cockpit, not graphical-client feature parity. It has
no rendered drone cameras, voice interface, free-look cockpit, or continuous
on-foot movement. Pilot identity stays on your own ship; remote manual takeover
is not exposed yet. Mouse-driven panels and configurable keys remain follow-up
work. Account/OIDC sign-in is not implemented in the engine.

The isolated real-server acceptance test starts a 50-ore resource job, waits for
mining and station delivery, verifies credits/storage, and resumes the saved
identity after disconnect. Transport tests cover fragmented/truncated responses,
endpoint separation, plaintext remote rejection and handshake deadlines.

```sh
cargo test --locked -p arkline-cli
cargo clippy --locked -p arkline-cli -- -D warnings
# Run only against your isolated local QA world:
ARKLINE_TUI_QA_SERVER=127.0.0.1:17887 cargo test --locked -p arkline-cli live_mining_delivery_and_resume -- --ignored
```

Interactive terminal layout and input acceptance must still be checked on real
macOS, Linux and Windows terminals. A successful protocol journey alone does not
prove that visual acceptance.
