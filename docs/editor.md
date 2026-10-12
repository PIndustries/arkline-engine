# Arkline Editor: local launcher

The first native Rust Editor is a local development launcher and process inspector. It starts a real isolated world and opens the existing game client. It does not yet edit ships, scenes, materials, room catalogs or server permissions.

Build the three native executables together:

```sh
cargo build --locked -p arkline-editor -p arkline-server -p arkline-client
cargo run --locked -p arkline-editor -- --project work/my-world --bin-dir target/debug
```

The default binary directory is the directory containing the Editor executable, suitable for a future bundle with all three binaries. `--bin-dir` selects a development build directory. These binaries must come from a compatible source revision supporting `--local-development`.

1. Choose a project folder and the binary directory.
2. Click **Start local world**. The Editor invokes `arkline-server --bind 127.0.0.1:0 --local-development <project>/.arkline-editor`. It waits for the server's actual loopback readiness message; the OS chooses an unused port.
3. Enter a test pilot name and click **Launch real client**. Repeat with another name for multiplayer testing (up to eight client processes).
4. Stop the session when finished. Closing the Editor stops and reaps only the clients and server it started. It does not close unrelated game windows or attach to existing servers.

The project retains `.arkline-editor/world.sqlite` and client credentials in `.arkline-editor/client`. The canonical project path supplies a stable explicit credential scope, so changing the ephemeral TCP port does not lose the pilot's resume credentials. Moving a project to another path changes that scope; keep the original project path for seamless resume. Existing pilot registration and resume-token validation remain enforced. This launcher does not recover or bypass missing credentials.

The UI has no public endpoint control. It never supplies official-world credentials or enables a WebSocket/public listener. Process output is bounded, with the last 200 lines visible. Start, readiness checks and child management run off the rendering thread; a server that fails to become ready within 15 seconds is stopped. Errors remain visible. Stopping terminates owned processes rather than simulating an in-game save/quit; finish any client action first.

Tests cover strict loopback readiness parsing, bounded output, pilot names, canonical project paths, explicit local-mode/client arguments and owned-child cleanup using disposable helper executables. An explicit ignored test also starts the actual server, connects a local pilot over the real loopback socket and verifies world creation, private directory permissions and child cleanup. Invoke it with `ARKLINE_EDITOR_BIN_DIR=/path/to/binaries cargo test -p arkline-editor actual_server_readiness_authentication_and_cleanup -- --ignored`. It never launches a GUI or connects publicly. These tests are not native visual acceptance or a completed server/client gameplay journey. Production deployment, remote project editing, account management, asset authoring and packaged Editor downloads are outside this first shell.
