# Verified web origins

These files describe the official marketing and browser/download origin TLS
configuration. They supplement the existing deployments; they are not a complete
hosting installation. Adapt namespaces, selectors, names and upstream ports for
an independent network. No private keys or tunnel credentials belong in Git.

## Boundaries

- Browser/download TLS terminates directly in the existing web Nginx process.
- Marketing TLS terminates in an unprivileged proxy in the same pod as the web
  application. The only subsequent hop is `127.0.0.1:4000` inside that pod.
- Existing HTTP services remain for their existing consumers and rollback.
  Arkline's public tunnel routes must explicitly select the new HTTPS services.
- The public edge and origin still read requests. This is encrypted transport,
  not end-to-end payload encryption that hides data from service operators.

The TLS proxy is pinned by image digest, has a read-only root, no capabilities,
no privilege escalation, bounded resources and its own temporary volume. The
existing application container's privileges are not changed by this patch.

## Issuance and staging

1. Provision the dedicated CA Secret in cert-manager's cluster-resource
   namespace through the operator's secret system. Apply the ClusterIssuer and
   game Certificates in [origin-certificates.yaml](../origin-certificates.yaml).
   Never mount the CA private key in application pods. Retain a protected backup.
2. Apply [certificates.yaml](certificates.yaml). Verify both Certificates are
   Ready and their DNS identities and renewal times match the intended origins.
3. Create the public CA ConfigMap `arkline-origin-ca` in the connector namespace,
   and mount it read-only at `/etc/arkline-origin-ca` in every connector replica.
   This bundle contains only the public root certificate.
4. Create configuration maps from the reviewed Nginx files:

```sh
kubectl -n arkline create configmap arkline-web-tls-nginx \
  --from-file=nginx.conf=deploy/web-origin-tls/web-nginx.conf \
  --dry-run=client -o yaml | kubectl apply -f -
kubectl -n bastion create configmap arkline-marketing-tls-nginx \
  --from-file=nginx.conf=deploy/web-origin-tls/marketing-nginx.conf \
  --dry-run=client -o yaml | kubectl apply -f -
```

5. Save the current deployment templates and tunnel route configuration in
   protected operator storage. Review the strategic patches against the current
   deployments, including pod group permissions, volumes and probes. Apply with
   a resource-version guard when other deployment work is possible:

```sh
kubectl -n arkline patch deployment arkline-web --type=strategic \
  --patch-file deploy/web-origin-tls/web.patch.yaml
kubectl -n bastion patch deployment bastion --type=strategic \
  --patch-file deploy/web-origin-tls/marketing.patch.yaml
kubectl apply -f deploy/web-origin-tls/services.yaml
kubectl apply -f deploy/web-origin-tls/network-policy.yaml
```

The patches preserve existing application images and service ports. Wait for
all new replicas to be ready. Do not replace an existing application ingress
policy without auditing its unrelated consumers. The marketing patch deliberately
does not introduce a new pod-wide ingress restriction.

## Verified route configuration

Before changing public routes, connect directly through a loopback port-forward
using the public CA and expected DNS identity. Verify actual HTML/download
content; assert a wrong CA and wrong name are rejected. An HTTPS kubelet probe
checks availability but does **not** verify certificate identity and is not
sufficient evidence.

| Public hosts | HTTPS origin service | Expected origin identity |
| --- | --- | --- |
| `play.arkline.network`, `downloads.arkline.network` | `arkline-web-tls.arkline.svc:8443` | `web.origin.arkline.network` |
| `arkline.network`, `www.arkline.network` | `arkline-marketing-tls.bastion.svc:8443` | `marketing.origin.arkline.network` |

For each selected Cloudflare Tunnel route set `service` to the HTTPS URL,
`originRequest.originServerName` to the expected identity,
`originRequest.caPool` to `/etc/arkline-origin-ca/ca.crt`, and
`originRequest.noTLSVerify` to `false`. Read the current configuration immediately
before updating it, preserve all other routes, and compare a complete readback.
Keep the final HTTP404 catch-all.

At the public edge require TLS 1.2 or newer, enable TLS 1.3, and redirect HTTP to
HTTPS on the same host while preserving path/query. Independently verify that
Arkline stays on its own domain, play/download content remains unchanged, and
unrelated parent-company services remain healthy.

## Renewal and rollback

The dedicated cert-manager leaves last 90 days and renew 30 days before expiry.
The Nginx configuration uses fixed filename variables with certificate caching
disabled, so renewed projected Secret files are loaded on each new handshake.
User-controlled SNI is never used as a filename. Existing sessions continue.
A missing or mismatched pair fails the handshake; no plaintext fallback is
configured. A pair read during a projection change can fail transiently; the
next handshake reads the new files. Monitor certificate expiry and issuer health.
See [Nginx's certificate-loading documentation](https://nginx.org/en/docs/http/ngx_http_ssl_module.html#ssl_certificate).

For CA rotation distribute a bundle containing old and new roots to every
connector before changing leaves. Verify the presented new identities before
removing old trust. Never set `noTLSVerify` as a recovery shortcut.

If public verification fails after route migration, restore only the affected
prior route entries from the protected backup, preserving other current routes.
The old HTTP service remains available for this bounded rollback. Then restore
the affected prior pod template if necessary. Do not replace database volumes
or restore player state as part of a web TLS rollback.

Release automation must update only the application image/release configuration
and preserve these commands, mounts, ports and services. Versioned browser and
download directories remain immutable. A new image alone is not proof that its
TLS route, content and certificate renewal work.
