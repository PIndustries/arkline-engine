# Public early-access relays

Arkline's desktop and browser clients use the same JSON protocol over WebSocket
text messages. Public connections must use **WSS**. The standalone server's raw
TCP listener remains available for local tooling and private operator tunnels;
it must not be exposed as a public plaintext endpoint.

The official host layout is:

| Host | Purpose |
| --- | --- |
| `arkline.network`, `www.arkline.network` | Project website redirect |
| `play.arkline.network` | Rust/WebAssembly client |
| `relay.arkline.network/ws` | Production WebSocket world |
| `test-relay.arkline.network/ws` | Separate Test WebSocket world |
| `downloads.arkline.network` | Versioned client/server/source archives and checksums |

A production channel names the persistent official world; it does not imply a
completed game, unlimited capacity, or an availability guarantee. This is early
access. Test uses its own database and credentials. Never point both processes
at one SQLite file or share production pilot credentials with another operator.

## Origin deployment

1. Build the server from the approved clean source revision using `Dockerfile`.
2. Retain the existing one-replica world StatefulSet and persistent volume. Add
   `--ws-bind 0.0.0.0:7778`, explicitly allow the browser's HTTPS origins using
   `--ws-origin`, and expose only port 7778 to the dedicated tunnel connector.
3. Set the pod label `arkline-public-origin: "true"` and add a private Service
   port for 7778. Keep raw TCP7777 under the default-deny network policy.
4. Use a separate one-replica StatefulSet and PVC for the Test world. Never
   clone live resume credentials into it. Back up Production before upgrades.
5. Build the browser with `apps/client/web/build.sh`. Prepare reviewed `web/`
   and `downloads/VERSION/` directories plus `web-nginx.conf` renamed to
   `nginx.conf`; build that staging directory with `web.Dockerfile`. Include
   matching source, license, dependency notices and checksums in public downloads.
6. Configure a dedicated authenticated Cloudflare Tunnel. Store its token in
   the `arkline-tunnel` Kubernetes Secret, never in source or browser assets.
   `public-edge.yaml` supplies the isolated connector, web deployment and narrow
   origin network allowances. Replace image placeholders with immutable digests
   and use the operator's existing image pull secret when necessary.
7. Route only the documented hostnames to their intended private Services. The
   last tunnel rule must be an HTTP404 catch-all. Publish proxied DNS records
   only after origin checks pass; do not overwrite unrelated DNS records.

TLS terminates at Cloudflare, and the connector uses an encrypted outbound
Cloudflare Tunnel. The initial deployment uses origin HTTP/WS, limited by Kubernetes NetworkPolicy to the
connector inside the cluster. This is not end-to-end application encryption
against the edge provider or a compromised cluster. Native origin TLS is supported
with `--tls-cert` and `--tls-key`; see [verified transport and rotation](../docs/transport-security.md).
Do not treat source support as proof of a deployed TLS migration.

## Validation and rollback

Before updating the website's availability flags, verify HTTPS and WSS from
outside the cluster, a real pilot's flight/mining/deposit/reconnect journey,
Production/Test isolation, invalid-origin and malformed-message rejection,
checksums, source availability, and browser rendering/input. A successful build
or DNS response alone does not prove gameplay works.

`/health` on the WebSocket origin reports success only after a successful
simulation tick within three seconds. See [transport limits](../apps/server/PUBLIC_TRANSPORT.md)
for admission, payload and request limits. Keep backups and previous immutable images available. Roll
back server code only when its schema can read the retained world; restore to
an isolated volume first when that is uncertain. Never overwrite live player
progress with a test fixture or force a second SQLite writer during recovery.

The official edge limits relay `/ws` connection attempts to 20 per 10 seconds
per source IP and Cloudflare data center, with a 10-second block after exceeding
the limit. This counts HTTP upgrade attempts, not messages inside an established
WebSocket. The server separately bounds messages and active sessions. Shared
networks may share an IP budget; these controls do not prevent identity farming.

Observe connection capacity, registration limits, simulation health, storage
and reconnect behavior before increasing traffic. Add durable monitoring,
scheduled off-cluster backups and measured load budgets as the audience grows.
Native platform signing/notarization and device-specific graphics/audio testing
are separate release checks.

## Browser release paths

Stage each complete browser build under `web/VERSION/`, retaining previous version
directories. Create `release.conf` in the reviewed image build context with
`location = / { return 302 https://play.arkline.network/VERSION/$is_args$args; }`, replacing VERSION with the
verified build number. Relative module and WASM imports then stay within that
release even during rolling deployments. Do not change files within a published
version. The same origin preserves existing browser pilot credentials.

For `play.arkline.network`, configure Cloudflare to bypass edge caching and respect
origin browser cache headers. The origin sends `Cache-Control: no-cache`; do not
override it with a browser TTL. Purge legacy unversioned browser assets when
introducing this layout. Download archives remain versioned independently.
