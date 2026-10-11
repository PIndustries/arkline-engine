# Transport security

Arkline's authoritative server must read movement, inventory and combat commands
in order to validate them. Transport encryption protects these messages in
transit; it is not application encryption that hides them from the game authority.
When Cloudflare terminates public TLS, the edge can also read those messages.
Private end-to-end encrypted chat would require a separate protocol and key design.

## Native client trust

Desktop clients explicitly select Rustls's ring provider and the public WebPKI
root store. This must work when building the client alone; workspace feature
unification is not a substitute for configuring its TLS dependency. Build .7
fixes the missing provider in earlier standalone native clients. The browser uses
its platform TLS stack. Both verify the public relay certificate and hostname.

An opt-in read-only test exercises the actual native transport against both
public relays:

```sh
cargo test -p arkline-client native_verified_wss_reaches_both_official_relays -- --ignored
```

## Current deployment and new server support

The initial official deployment uses certificate-verified public WSS, an encrypted
Cloudflare Tunnel, and a private plaintext connector-to-origin hop. NetworkPolicy
limits access to that origin, but does not encrypt it. Native origin TLS support
is now available in source; this statement is **not evidence that the running
official deployment has switched to TLS origins**. Rollout evidence is tracked in
[issue #15](https://github.com/PIndustries/arkline-engine/issues/15).

The native Rust server can terminate TLS itself:

```sh
arkline-server --bind 127.0.0.1:7777 --ws-bind 0.0.0.0:7778 \
  --tls-cert /run/arkline-tls/tls.crt --tls-key /run/arkline-tls/tls.key \
  --db /data/arkline.sqlite
```

Both PEM paths and `--ws-bind` are required together. Invalid or mismatched keys
fail startup before opening the world. With TLS configured, `/ws` and `/health`
both require TLS; plaintext is not an automatic fallback. Rustls uses its safe
TLS 1.2/1.3 defaults and HTTP/1.1 ALPN. Connection admission, absolute handshake
and message deadlines, origin checks and bounded payloads remain in force.
The raw TCP listener is still plaintext: bind it to loopback and do not publish
it. The TLS listener authenticates the server; it does not currently demand
client certificates. Pilot credentials and origin/network restrictions remain
separate controls.

## Certificate issuance and rollout

Use a dedicated operator CA or a managed certificate issuer. Issue a separate
leaf certificate for each environment with a DNS SAN matching its configured
origin identity, serverAuth usage and a short documented renewal lifetime.
Keep the CA signing key outside game pods. Store leaf keys in Kubernetes Secrets,
mount read-only with permissions readable only by the server UID, and grant no
workload API permission to list Secrets. Never commit keys or embed them in clients.

For a Cloudflare Tunnel route, use an `https://` service URL, an explicit
`originServerName` matching that SAN, and `caPool` pointing at the mounted public
CA bundle in **every** connector replica. Keep `noTLSVerify: false`. The public
CA bundle contains no private key. Preserve all other hostname routes.
See [Cloudflare origin parameters](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/origin-parameters/).

Prepare and verify the CA mount before switching routes. Back up the world,
retain prior immutable images, then deploy and prove Test first. Coordinate the
listener/probe/service and route changes as a maintenance transition: a TLS-only
listener cannot serve a connector still configured for HTTP. Do not run a second
writer on a world's SQLite file. Production follows only after Test's verified
WSS reconnect, gameplay and state-preservation checks pass.

Use a verified HTTPS health check with the expected name and CA. Kubernetes
`httpGet.scheme: HTTPS` alone does **not** verify the server certificate, so it
is not certificate-validation evidence. The server includes a verified readiness command suitable for an exec probe:

```sh
arkline-server --tls-health 127.0.0.1:7778 \
  --tls-ca /run/arkline-ca/ca.crt --tls-server-name relay-origin.example
```

It checks the leaf name, CA trust and simulation health with bounded connection
and I/O timeouts. Use a literal loopback address in probes to avoid DNS latency.
The [cert-manager example](../deploy/origin-certificates.yaml) renews separate
90-day leaves 30 days before expiry; CA expiry and issuer health still require
operator monitoring. The [example patch](../deploy/origin-tls.example.yaml) shows the server-side
mounts and probe; it is not a standalone deployment or a tunnel route change.
Check expiry separately and alert well
before renewal. Do not replace simulation readiness with a bare TCP-open probe.

## Rotation and failure recovery

Certificates are validated before startup and reloaded for each admitted new
TLS connection. Existing TLS sessions keep their established identity. Kubernetes
projected Secret files resolve through one `..data` generation so a rotation
cannot pair a new certificate with an old key. A malformed or mismatched update
rejects new connections; it never falls back to plaintext or an older identity.
Use a dedicated cert-manager CA Issuer/Certificate or your operator's issuer to
renew leaves before expiry. Keep the CA key in the issuer's protected Secret,
never mounted in game pods. Secret projection can lag renewal; verify the leaf
serial/expiry actually presented by each server after rotation. For ordinary
files, atomically switch a directory containing the matching pair. Rotation does
not require a world restart. Invalid credentials fail readiness until repaired.

For CA rotation: distribute a bundle trusting old and new CAs to all connectors;
verify they rolled out, rotate Test then Production leaves, then remove the old
CA only after every leaf and connector is verified. Roll back to the previous
valid certificate/image if needed. Never recover by disabling certificate or
hostname verification. Revoke/replace compromised keys and resume credentials
according to the actual exposure; TLS does not encrypt databases or local files.

## Verification

```sh
cargo build -p arkline-server
python3 tools/tests/test_origin_tls.py
```

The integration suite uses a temporary certificate and isolated database to test
verified TLS health and WebSocket messages, wrong identity and untrusted CA
rejection, plaintext rejection, unauthorized Origin, and incomplete TLS startup.
Repeat the checks on each deployed origin with the actual trusted CA, then prove
external Production/Test WSS and stored player state after rollout. Website,
download and account origins must be audited independently before claiming that
**every** public-service hop is encrypted.
