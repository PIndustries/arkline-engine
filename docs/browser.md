# Browser client and secure servers

Arkline's browser build runs the same Rust cockpit, tactical views, ship renderer, hangar, station services, fleet controls, inventory, and server-authoritative gameplay as the native client. It requires a desktop browser with WebAssembly and WebGL 2, a keyboard and mouse. Touch/mobile controls are not implemented. Browser right-mouse aiming remains visible and bounded by the screen; macOS native pointer capture is not advertised as a browser feature. The Fullscreen button requests browser fullscreen, which the browser may require a direct user gesture to allow.

The server picker offers **Production**, **Test**, and an editable custom address. Production defaults to `wss://relay.arkline.network/ws`; Test uses `wss://test-relay.arkline.network/ws`. These are separate worlds and pilot identities. `?network=test` selects Test on browser startup. A custom browser endpoint must use `wss://`; `ws://localhost:PORT/ws` and `ws://127.0.0.1:PORT/ws` are allowed for local development. An HTTPS page cannot connect to an insecure local WebSocket unless the browser permits it.

Native clients now default to Production too. `--local` explicitly starts the bundled local server; `--server HOST:PORT` retains the development TCP transport. `--server wss://.../ws` uses certificate-validated TLS without redirecting authentication to another endpoint. Native local preview saves remain available in their existing data directory.

## Identity and recovery

Browser resume keys are stored in that browser's origin-scoped local storage, indexed by server endpoint and pilot name. They are not sent to another selected endpoint. Clearing site data, switching browsers, or switching website origins does not carry the identity over. The browser and native client currently have no credential import/export interface; use separate pilot names unless you already manage matching resume credentials securely. Do not publish or paste resume keys into URLs.

The browser checks writable storage before creating a pilot. If the connection fails while an action is awaiting a response, its outcome is unknown: reconnect and inspect the authoritative state before retrying. Queued commands are not replayed. Browser recovery currently uses the visible Reconnect control; native clients retain bounded automatic resume attempts. Background tabs can be throttled by the browser; server flight-input leases still expire safely.

## Browser-specific limits

Procedural game audio uses the same Rust synthesizer through Web Audio and starts from the Enter action. Browser autoplay policy can require another explicit click on Settings → Retry audio output. Audio rendering is device/browser dependent.

Native macOS speech, the local SSH model adapter, local server hosting, and independent ledger replay auditing are unavailable in the browser. The UI identifies these limits. Deterministic ship-computer resource jobs, fleet delegation, and other normal server commands remain available. Use the native client or verifier CLI for an independent pinned audit.

## Build and serve

Use Rust 1.89 and the `wasm32-unknown-unknown` target. Install the wasm-bindgen CLI matching the version in Cargo.lock (currently 0.2.129):

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
apps/client/web/build.sh
python3 -m http.server 18080 --bind 127.0.0.1 --directory target/web
```

The output directory is `target/web` or the first argument to `build.sh`. Serve it over HTTP for local testing and HTTPS publicly; do not open the HTML through `file://`. The bootstrap script is external to work with a restrictive Content Security Policy. The server must explicitly allow the browser origin, for example `--ws-origin http://127.0.0.1:18080` for that local test. A WebSocket request is one JSON text message using the existing protocol; responses are single JSON text messages. Binary messages are unsupported.

The release's source download and license are linked in the client. Distribute the matching complete source, including dependency sources and notices, alongside browser and native releases. Building the web output alone does not publish it or certify every browser/GPU combination.
