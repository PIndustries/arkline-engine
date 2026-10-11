# Next bounded game-admission implementation

Status: proposal, not exposed by the server. The access-token library remains disabled by default and unconnected to transports. Existing registration/resume behavior is unchanged. No account is linked by email, display name, or matching pilot name.

## 1. Prove the provider contract separately

The signed fixture tests exercise verifier policy; they do not prove a real provider produces compatible game tokens. A disposable provider fixture must register a public PKCE S256 game client with RS256, a lifetime at most 900 seconds, approved game scopes and a single environment's resource audience. Its client ID must be operator-approved by the verifier. Use the explicit provider profile only when configured by the operator.

Exercise actual authorization-code exchange, successful game access-token verification, rejection of the same exchange's ID token, identity-only token rejection, another client's rejection, wrong-world rejection, key rollover and expired-token denial. Keep account/password/token material temporary and out of repositories, command arguments and logs. Only public keys and redacted outcomes may be retained. Current account-portal identity-only registration does not provide game scopes or game admission. A real provider game-token fixture has not yet been verified.

## 2. Add a dormant authorization boundary

Introduce a session type containing the real connection pilot plus either legacy authentication or a verified account principal: exact issuer, subject, approved client ID, resource realm, granted scopes and absolute expiry. Replace the transport-owned `Option<String>` in server TCP/WebSocket handlers with this type; pass it into a single authorization gateway before ordinary dispatch and before unwrapping remote commands. Do not infer account authority from an effective remote pilot.

Start with explicit `disabled` and `optional` policies; do not deploy or introduce account-required migration yet. Missing configuration stays disabled; malformed enabled configuration fails startup rather than silently falling back. Advertise protocol capability only when the complete admission/linking boundary is implemented. Disabled servers reject new account requests without changing sessions or economic state.

Trusted JWKS loading/refresh belongs outside the simulation mutex. Bound response size, redirects, timeouts, key count and cache age; fail closed for unknown keys and expired caches. A failed refresh must not replace the last validated cache. Do not fetch token-supplied URLs. An initial operator-supplied key file can keep discovery out of the first implementation, but needs an explicit age/replacement policy.

## 3. Persist dual-proof links transactionally

Add an explicit existing-pilot link operation requiring both the current pilot resume credential and a valid account access token in the same request. Verify the account token outside the simulation lock, then recheck expiry, pilot credential and link conflicts inside the same SQLite transaction that stores the link. Use a private linkage table with unique pilot ownership and identity `(issuer, subject)` plus explicit world/realm binding; do not add identity or tokens to public snapshots, economic exports or ledger events.

For the first slice, permit one account principal to link one pilot per world. Reject conflicting ownership; retrying the identical link may be idempotent. Account login only opens an already linked pilot. New account-pilot creation, transfers, unlinking and recovery need separate product/security decisions. Never turn an account login into an unrestricted permanent resume token.

The initial link can preserve the existing resume credential as an explicitly documented independent legacy recovery credential. This means account/provider revocation does not revoke that credential. Account-only migration requires a deliberate owner-visible transition, not implicit invalidation or a claim of unified revocation.

## 4. Expiry, scopes and background authority

Admission scope alone is not permission for every command. Define exact scopes for play/control, read-only telemetry and linking before exposing them; classify every request exhaustively, including nested remote-control and autonomous-job requests. Reject ID tokens and sender-constrained tokens without proof verification. Require the correct game resource and approved client in addition to scopes. Production/Test use separate resource policies; audience-array membership must not accidentally create cross-world authority.

Expire sessions on server ticks as well as request arrival, so an idle socket cannot keep thrust, remote-control leases or delegated jobs alive indefinitely. Reuse ordinary disconnect cleanup without double decrementing presence or disrupting other valid sessions for that pilot. Reauthorization must prove the same principal; principal switching needs explicit teardown. No refresh token is stored by the game server. Provider logout/client revocation alone does not immediately revoke locally validated access tokens; bounded expiry is the initial limit, pending a separate revocation design.

## Required acceptance tests

- Default-disabled legacy connect/resume regression; malformed enabled configuration fails closed; unknown account requests make no state change.
- Actual provider fixture above, plus configured client/profile isolation and duplicate-key rejection.
- TCP and WebSocket parity for every command category, including nested remote requests; no scope escalation via delegation.
- Link requires both proofs; wrong resume token, wrong account, expired proof, conflict and concurrent link attempts reject without credential/ledger changes. Crash/restart preserves exactly one committed link.
- Account login cannot issue a new permanent resume token; public snapshots/exports contain no account identifiers or credentials.
- Production token denied by Test policy and vice versa; key/cache outage and rollover behavior is deterministic.
- Idle expiry stops manual input, remote leases and autonomous work as defined; second valid session survives; reconnect/re-auth cannot replay stale controls.
- Migration/recovery semantics are explicit and tested before enabling any official world.

Public integration changes belong in the engine's protocol, server session/authorization/persistence and client transport modules. Account-provider implementation and administrative credentials remain outside the public engine.
