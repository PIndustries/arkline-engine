# Public WebSocket transport

Run the optional listener with `--ws-bind 0.0.0.0:7778` behind a trusted TLS edge. Without TLS flags the origin is plaintext: expose only the edge WSS endpoint, keep raw TCP private, and restrict origin network access to the proxy. Add `--tls-cert PEM --tls-key PEM` to terminate native Rust TLS on the WebSocket listener. See [transport security](../../docs/transport-security.md) for verified origin setup, rotation and deployment limitations. Both transports share one authoritative world, database and session cleanup.

`/ws` accepts one existing protocol JSON Request per text message and replies with one JSON Response per text message. Connect requires the current protocol version; existing identities require their bearer resume credential. Never place credentials in URLs. Binary messages are rejected. Native clients may omit Origin; browser Origins must exactly match `https://arkline.network`, `https://www.arkline.network`, `https://play.arkline.network`, or `https://test.arkline.network`. `--ws-origin ORIGIN` adds an exact operator-approved origin. Origin checking supplements authentication; it does not authenticate native clients.

`GET /health` returns 200 only when a successful simulation tick occurred within three seconds, otherwise 503. It does not guarantee a complete gameplay journey, external network health or backup health.

## Limits

- 64 simultaneous connections across raw TCP and WebSocket, including handshakes.
- WebSocket admission:16 connections burst,4 per second globally.
- HTTP handshake:16KiB and five seconds; each complete WebSocket message:16KiB; each read operation has a30-second deadline (control frames can finish a read). Idle clients must poll or ping within 30 seconds.
- Outbound response:8MiB, ten-second absolute write deadline, no unbounded outbound queue.
- Messages including ping/pong:60 burst and30 per second per connection,600 burst and300 per second globally. Rate overflow closes the connection.
- Five failed Connect responses close a connection.
- New pilots:256 durable pilots by default (`--ws-max-pilots`1–1024); registration burst five, refill five per minute globally. Existing pilot resume works when registration capacity is reached.
- Active market orders:1024 globally,64 per authenticated pilot for public requests.
- Public ledger export: at most10,000 entries; one export per30 seconds per connection, globally two burst and one per second. Larger exports require operator access.

These are conservative prototype resource ceilings, not a proven capacity or abuse-resistance claim. Global registration quotas do not prevent identity farming. Read-only snapshots and bounded ledger export are available without login; private inventories and resume credentials are not. Operator raw TCP intentionally retains existing capabilities and must not be publicly exposed. Process-local quotas reset on restart; durable pilot and active order counts do not. Cloud edge limits and access policy remain necessary.

On EOF, close, timeout, oversized message, or failed writes, the session releases its presence reference and existing authoritative disconnect safety applies. Multiple authenticated connections for one pilot remain supported. A lost reply leaves action outcome uncertain; clients must resume and inspect state, never replay mutations automatically.
