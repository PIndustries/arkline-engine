# Deploying Arkline

This directory contains the private-world baseline and the public transport examples. Applying `private-world.yaml` alone does not publish an endpoint. See [public relays](PUBLIC.md), [verified transport](../docs/transport-security.md), and [web origin TLS](web-origin-tls/README.md) for the additional components. Source manifests alone are not evidence of a completed deployment.

## Target prerequisites

Use the explicitly selected private Kubernetes cluster through its existing operator access. Do not change local contexts or copy cluster credentials into this repository. The prepared manifest expects Linux amd64 nodes and a block-backed `general` StorageClass with volume expansion. Verify those facts, node health, free capacity, storage replica health, and CNI NetworkPolicy enforcement before applying.

The target's existing ingress and tunnel services are outside this deployment's scope. Longhorn replication does not establish an off-cluster backup or a recovery SLA. Keep infrastructure inventory and credential locations in private operator records.

## Server constraints

The server supports JSON-lines over private TCP and the same game protocol over WebSocket. `--ws-bind` enables `/ws` and simulation-aware `/health`; adding `--tls-cert` and `--tls-key` terminates native TLS on that listener. Public clients use certificate-verified WSS. Keep the raw TCP listener on loopback/private operator access because Connect carries a bearer resume token. Do not expose TCP7777 publicly. The baseline below uses authenticated Kubernetes port-forward over SSH for initial private validation; the public path uses a verified HTTPS tunnel origin.

One process owns one SQLite world, including its WAL and lifetime lock file. Use exactly **one replica** and one block-backed RWO volume. Extra pods are not scale-out or high availability. Do not force-delete a pod while its node may still be writing; confirm fencing before recovery. Every node/process restart cancels live automation and restores durable economy according to existing server rules.

The transport currently allows 64 connections, one blocking worker thread each. A global mutex serializes simulation and requests. Resource requests/limits here are initial validation budgets, not load-test results. The baseline TCP readiness check proves only a listening socket. For a TLS origin use the certificate-verified `--tls-health` executable probe described in the transport guide; it also requires a recent successful simulation tick.

## Build and release boundary

Commit the exact source being deployed before building its image. Build from an export of that clean commit, never a working tree containing credentials or unreviewed edits:

```sh
# Substitute an approved exact commit and an existing private registry repository.
git archive --format=tar COMMIT > /secure/operator-work/arkline-source.tar
# Extract into a fresh operator-owned directory, then:
docker build --platform linux/amd64 -f deploy/Dockerfile \
  --build-arg SOURCE_COMMIT=COMMIT -t PRIVATE_IMAGE:COMMIT .
```

The Dockerfile uses Rust 1.89 (the locally verified compiler version) and Debian Bookworm. The source allowlist in `Dockerfile.dockerignore` excludes local worlds, credentials, build artifacts, and repository history. A successful Linux container build has to be verified separately. Record the resulting immutable digest; select a private registry and use its existing authorized credential path. Never put registry credentials in Git or console output.

Before apply, replace the manifest's deliberately invalid image placeholder with the immutable private digest and configure a namespace-scoped imagePullSecret if that registry requires one. The manifests create only a dedicated namespace, ClusterIP Service, deny-all NetworkPolicy, one StatefulSet, and a 10Gi PVC. No public Ingress, NodePort, or LoadBalancer is included. NetworkPolicy must be verified under this CNI; Kubernetes API port-forward traffic is a distinct access path.

The StatefulSet retains its PVC on deletion/scaling. The storage class still deletes backing storage if the PVC itself is deleted: do not delete the namespace/PVC as routine rollback. Service-account token mounting is disabled; the process runs UID10001 with a read-only root filesystem and dropped capabilities.

## Operator validation after authorized apply

On the authorized operator host, set `ARKLINE_KUBECONFIG` to the existing private kubeconfig path. The placeholder below is not a credential. Do not export or print credential contents.

```sh
# On the operator host, after the reviewed manifest has been copied there:
ARKLINE_KUBECONFIG=/path/to/operator/kubeconfig
sudo kubectl --kubeconfig="$ARKLINE_KUBECONFIG" apply -f private-world.yaml
sudo kubectl --kubeconfig="$ARKLINE_KUBECONFIG" -n arkline rollout status statefulset/arkline
sudo kubectl --kubeconfig="$ARKLINE_KUBECONFIG" -n arkline port-forward \
  --address 127.0.0.1 service/arkline 17777:7777
# Separate workstation terminal, preserving loopback-only access:
ssh -N -L 127.0.0.1:17777:127.0.0.1:17777 OPERATOR_HOST
```

Connect the native client to `127.0.0.1:17777`. Keep pilot resume credentials in its normal private credential store; do not print Connect request/response payloads.

Required evidence before calling this hosted validation successful:

1. Pod Ready; exact image digest and source commit recorded; PVC bound to the intended block storage.
2. Real authenticated client connects through the tunnel, moves, mines, returns, and stores ore. Independent ledger replay passes.
3. Controlled server restart retains pilot identity, ore, station inventory and ledger, without a second writer or unexpected resume of automation.
4. An unauthorized direct in-cluster connection is blocked; there is no public game listener or ingress route.
5. A SQLite-consistent backup and separate restore are tested. Use SQLite's backup API or stop the sole writer before backing up the volume; copying only a live `.sqlite` file can omit WAL data. Longhorn replication is not a backup.

Public deployment additionally needs admission/account abuse controls, appropriate rate limits and measured capacity, monitoring of ticks/ledger/volume health, and an operator recovery procedure. The public WebSocket implementation and TLS configuration do not by themselves establish availability or account-system readiness. Existing economic hash chains are not signed global consensus or a community reward system.
