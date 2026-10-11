# Dormant game access-token validator

This is a library foundation for issue [#9](https://github.com/PIndustries/arkline-engine/issues/9), not an account-login feature. The server does not read this configuration from environment variables or disk, and no TCP, WebSocket, game command, registration or client-login path calls it. Existing pilot resume credentials and local operation remain unchanged.

`arkline_server::access_tokens::GameAuthConfig` defaults to disabled (`{}` or `access_tokens: null`). An explicit configuration can construct an immutable verifier with a caller-supplied trusted JWKS buffer. A successful result identifies token claims only: it does not establish pilot ownership, create a pilot, admit a connection, or grant any command.

## Proposed access-token contract

The first profile accepts RS256-signed RFC 9068 JWT access tokens with `typ` exactly `at+jwt` or `application/at+jwt`. It rejects absent/generic `JWT`/ID-token types, symmetric algorithms, unknown keys, unsupported critical headers, token-supplied key URLs/embedded keys, and encrypted/compressed tokens. It never performs a network request.

**Compatibility with the official account provider is not yet verified.** Its current identity-only tokens are not a game API contract. Before integration, the issuer must demonstrably issue this access-token type with a game resource audience and approved game scopes. An issuer must not label ID tokens with this access-token type. If it cannot support this profile, choose and independently review a different unambiguous access-token contract; do not relax validation to accept generic identity tokens.

Example library configuration (illustrative values, not deployed endpoints or existing grants):

```json
{
  "access_tokens": {
    "issuer": "https://issuer.example.test/",
    "audience": "arkline:test-world",
    "required_scopes": ["game.connect"],
    "allowed_algorithms": ["RS256"],
    "max_lifetime_seconds": 300,
    "clock_skew_seconds": 5
  }
}
```

The exact HTTPS issuer string must match; URL equivalence is not account identity. Audience matching requires the configured resource in the signed audience string/array. Audience arrays are intentionally supported: membership is sufficient, not a single-audience-only restriction. Production and Test must configure different audiences, and the issuing policy must not put both into a token intended for only one environment. All configured scopes are required as exact space-separated values, never substring matches. Scope names here are proposals, not available grants.

`iss`, `sub`, `aud`, `exp`, `iat`, `jti`, `client_id` and `scope` are mandatory. The signature, issuer, audience, expiry and optional not-before use the maintained `jsonwebtoken` verifier. Additional policy bounds signed lifetime to the configured maximum (at most 900 seconds), future issuance/not-before skew to at most 30 seconds, and identifiers/scope lengths. Expired tokens are rejected without expiry grace. The returned expiry must be enforced on every future external command. There is no refresh token, token persistence, replay cache or immediate-revocation promise in this module; `jti` is required metadata, not replay protection.

## Key provenance and limits

The future caller must obtain keys from operator-trusted issuer metadata/configuration. This module cannot prove the supplied keys belong to the configured issuer. It accepts at most 64 KiB of JWKS JSON and 16 keys. Every key needs a bounded unique `kid`; duplicates fail the entire set, including duplicates across algorithms. Missing/unknown token `kid` fails without lookup or fetch.

Only explicitly RS256-advertised RSA public keys are selected. RSA moduli must be 2048–4096 bits, with public exponent 65537. If supplied, `use` must be `sig` and `key_ops` exactly `["verify"]`. Other public algorithms may coexist but are never selected. Private/symmetric key material is rejected. Access tokens are limited to 12 KiB, leaving room within the current transport request limit for future admission metadata.

Key rotation is an explicit replacement of an immutable verifier after validating a new trusted buffer. Failed replacement cannot alter the old verifier. **No discovery, refresh timer, JWKS cache age or outage policy exists yet.** These must be implemented before admission; never perform provider I/O while holding the simulation mutex. Never log bearer tokens or propagate verifier/parser errors containing token contents; this API returns bounded generic errors.

## Tests and remaining gates

Signed-token tests generate ephemeral RSA fixture keys in memory. They cover default-disabled configuration, exact issuer/resource audience/scopes, missing claims, expiry/future issuance/not-before/lifetime, ID-token rejection despite matching key and audience, signature/algorithm/key confusion, token-controlled key sources, duplicate/missing IDs, private/weak/misused keys, size bounds and explicit trusted-key replacement.

Before exposing login:

- Verify the issuer's real token profile, game audience and permitted scopes using a disposable fixture. Identity-only scopes must not gain game authority.
- Add a transport-independent scoped session principal and enforce expiry/permissions on TCP and WebSocket commands, nested remote commands and disconnect cleanup.
- Add transactional explicit pilot linking that proves both the old resume token and account identity `(issuer, subject)`. Never link by email, display name or pilot name. Keep links out of public snapshots and the economic ledger.
- Prevent OAuth login from issuing a permanent unrestricted resume credential. Define legacy/account-only migration, recovery, revocation and refresh behavior separately.
- Implement exact callback/state/PKCE S256 flows and safe credential storage for native, browser and terminal clients; preserve old-server capability handling without silent credential downgrade.
- Complete real admission/link/restart/authorization-denial tests before enabling any official environment.

References: [RFC 9068](https://www.rfc-editor.org/rfc/rfc9068.html), [OAuth security BCP](https://www.rfc-editor.org/rfc/rfc9700.html), [jsonwebtoken](https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/).
