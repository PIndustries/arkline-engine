# Private preview security

Report issues privately to the P.Industries repository maintainers. Do not publish
credentials, SQLite databases, or pilot resume tokens in issues or pull requests.

The current transport is plaintext and intended for loopback. Pilot names are not
authentication: existing pilots require the random bearer token issued at creation.
Tokens are stored as hashes on the server and in permission-restricted client files.
Losing a credential currently requires operator intervention; no account recovery
or credential-rotation service exists in this preview.

Server snapshots and audit exports deliberately expose the local test world's
economic state. This is not a privacy design for a production public universe.

Hash linkage detects alteration relative to a known checkpoint; it is not a digital
signature or proof that the operator simulated events honestly. Do not treat
prototype credits or any reported uptime as financial assets or payout claims.
