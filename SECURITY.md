# Security and responsible reporting

For an initial private security report, contact the existing P.Industries contact
address, info@p.industries, with subject "Arkline security report". Include the
affected version and a concise description; do not include credentials, player
databases or resume tokens. Maintainers can arrange an appropriate follow-up.
This is not a promise of a dedicated response team or response-time guarantee.
Do not post exploit details or sensitive data in public issues or community chat.

Official public connections use WebSockets over TLS (WSS). The separate raw TCP
listener is intended for loopback or trusted private connections; never expose it
with bearer credentials over plaintext Internet transport. Pilot names are not
authentication: existing pilots require the random bearer token issued at creation.
Tokens are stored as hashes on the server, in permission-restricted native client
files, or in the browser client's local storage. Browser storage is not a native
credential vault; clearing site data can lose the resume key.
Losing a credential currently requires operator intervention; no account recovery
or credential-rotation service exists in this preview.

Server snapshots and audit exports deliberately expose the connected world's
economic state. This is not a privacy design for a production public universe.

Hash linkage detects alteration relative to a known checkpoint; it is not a digital
signature or proof that the operator simulated events honestly. Do not treat
prototype credits or any reported uptime as financial assets or payout claims.

## Supported releases

Report against the current published early-access release where practical, and
include whether the issue also occurs on the Test world. There is no long-term
maintenance or security-backport commitment for older previews. Keep backups and
review migration notes before upgrading independent servers.
