# Game access-token validation

## Current source boundary · 26 October 2026

The verifier is no longer an unconnected library. [Server identity admission](../apps/server/src/identity.rs) calls it for `AccountConnect`, shared by TCP and WebSocket handlers. Authentication remains opt-in: the server accepts `--auth-config JSON --auth-jwks PUBLIC_JWKS`, optionally `--auth-required`; both input files are required together. The configuration JSON is an `AccessTokenConfig` object, not the `GameAuthConfig` wrapper in the historical example below. `configure_auth` requires the exact `arkline.play` scope. Isolated local-development mode cannot enable official account authentication.

Trusted keys are operator-provisioned public JWKS; tokens do not select key URLs. The verifier rejects sender-constraint and actor claims (`cnf` and `act`), including null values. Approved client IDs, issuer, audience, signature, profile and lifetime checks remain explicit. Verification identifies claims; the separate admission layer decides pilot ownership. Neither this documentation recovery nor fixture tests prove compatibility with a deployed provider.

See [current admission status and remaining gates](game-admission-plan.md), [verifier source](../apps/server/src/access_tokens.rs), [server arguments](../apps/server/src/main.rs) and [signed-token/admission regression tests](../apps/server/tests/access_tokens.rs). No test suite was rerun as part of this documentation-only restoration.

## Historical design record

> The following text is preserved from `bee9c865b1c736ba21f359b67debae967db25a13`. Its dormant/unconnected descriptions, future-tense implementation steps and illustrative scopes describe that earlier checkpoint, not the current server. The current-source section above takes precedence.

## Original dormant-validator design

This is a library foundation for issue [#9](https://github.com/PIndustries/arkline-engine/issues/9), not an account-login feature. The server does not read this configuration from environment variables or disk, and no TCP, WebSocket, game command, registration or client-login path calls it. Existing pilot resume credentials and local operation remain unchanged.

`arkline_server::access_tokens::GameAuthConfig` defaults to disabled (`{}` or `access_tokens: null`). An explicit configuration can construct an immutable verifier with a caller-supplied trusted JWKS buffer. A successful result identifies token claims only: it does not establish pilot ownership, create a pilot, admit a connection, or grant any command.

### Proposed access-token contract

Both profiles accept RS256-signed access tokens with JOSE `typ` exactly `at+jwt` or `application/at+jwt`. The default `rfc9068` profile requires the signed `client_id` claim. The opt-in `rauthy037` profile instead requires signed `azp` and payload `typ: "Bearer"`; it never falls back to `client_id`. The operator selects the profile in trusted configuration, never from token content. It rejects absent/generic `JWT`/ID-token types, symmetric algorithms, unknown keys, unsupported critical headers, token-supplied key URLs/embedded keys, and encrypted/compressed tokens. It never performs a network request. **Any `cnf` claim, including null, is rejected in both profiles** until sender-constrained proof validation (such as DPoP or mTLS binding) is implemented.

**Live provider compatibility is not yet verified.** The optional Rauthy 0.37 profile is grounded in upstream commit `ce35a70fbc43203b1a6910d3db30389228825f46`: access tokens use `azp`, payload `Bearer`, JOSE `at+jwt` and resource audiences; ID tokens use payload `Id` and JOSE `JWT`. See the pinned [token construction](https://github.com/sebadob/rauthy/blob/ce35a70fbc43203b1a6910d3db30389228825f46/src/service/src/token_set.rs#L190-L220), [JOSE signing type](https://github.com/sebadob/rauthy/blob/ce35a70fbc43203b1a6910d3db30389228825f46/src/service/src/token_set.rs#L294-L296) and [claims](https://github.com/sebadob/rauthy/blob/ce35a70fbc43203b1a6910d3db30389228825f46/src/jwt/src/claims.rs#L14-L36). This source evidence is not a successful live fixture.

Game clients must explicitly configure RS256, a lifetime within the verifier's maximum, and approved/requested game scopes; provider defaults are not sufficient. The intended game resource must be authorized up front; token exchange must not be used to add a previously ungranted resource. Machine credentials are not player ownership: requiring `sub` is necessary but does not replace an issuer policy excluding inappropriate grants. Before integration, verify real user access tokens and ID-token denial against a disposable provider fixture. Never relax these profiles into a generic identity-token fallback.

Example library configuration (illustrative values, not deployed endpoints or existing grants):

```json
{
  "access_tokens": {
    "profile": "rfc9068",
    "issuer": "https://issuer.example.test/",
    "audience": "arkline:test-world",
    "required_scopes": ["game.connect"],
    "allowed_client_ids": ["approved-game-client"],
    "allowed_algorithms": ["RS256"],
    "max_lifetime_seconds": 300,
    "clock_skew_seconds": 5
  }
}
```

The exact HTTPS issuer string must match; URL equivalence is not account identity. Audience matching requires the configured resource in the signed audience string/array. Audience arrays are intentionally supported: membership is sufficient, not a single-audience-only restriction. Production and Test must configure different audiences, and the issuing policy must not put both into a token intended for only one environment. All configured scopes are required as exact space-separated values, never substring matches. Scope names here are proposals, not available grants.

`iss`, `sub`, `aud`, `exp`, `iat`, `jti` and `scope` are mandatory in both profiles. `client_id` is mandatory only in the default RFC profile; `azp` and payload `typ: "Bearer"` are mandatory only in the explicitly selected Rauthy profile. The signature, issuer, audience, expiry and optional not-before use the maintained `jsonwebtoken` verifier. Additional policy bounds signed lifetime to the configured maximum (at most 900 seconds), future issuance/not-before skew to at most 30 seconds, and identifiers/scope lengths. Expired tokens are rejected without expiry grace. The returned expiry must be enforced on every future external command. There is no refresh token, token persistence, replay cache or immediate-revocation promise in this module; `jti` is required metadata, not replay protection.

### Key provenance and limits

The future caller must obtain keys from operator-trusted issuer metadata/configuration. This module cannot prove the supplied keys belong to the configured issuer. It accepts at most 64 KiB of JWKS JSON and 16 keys. Every key needs a bounded unique `kid`; duplicates fail the entire set, including duplicates across algorithms. Missing/unknown token `kid` fails without lookup or fetch.

Only explicitly RS256-advertised RSA public keys are selected. RSA moduli must be 2048–4096 bits, with public exponent 65537. If supplied, `use` must be `sig` and `key_ops` exactly `["verify"]`. Other public algorithms may coexist but are never selected. Private/symmetric key material is rejected. Access tokens are limited to 12 KiB, leaving room within the current transport request limit for future admission metadata.

Key rotation is an explicit replacement of an immutable verifier after validating a new trusted buffer. Failed replacement cannot alter the old verifier. **No discovery, refresh timer, JWKS cache age or outage policy exists yet.** These must be implemented before admission; never perform provider I/O while holding the simulation mutex. Never log bearer tokens or propagate verifier/parser errors containing token contents; this API returns bounded generic errors.

### Tests and remaining gates

Signed-token tests generate ephemeral RSA fixture keys in memory. They cover default-disabled configuration, exact issuer/resource audience/scopes, missing claims, expiry/future issuance/not-before/lifetime, ID-token rejection despite matching key and audience, signature/algorithm/key confusion, token-controlled key sources, duplicate/missing IDs, private/weak/misused keys, size bounds and explicit trusted-key replacement. Profile tests verify no `azp`/`client_id` fallback, reject ID payloads and wrong JOSE types, and reject `cnf` in every profile.

Before exposing login:

- Verify the issuer's real token profile, game audience and permitted scopes using a disposable fixture. Identity-only scopes must not gain game authority.
- Add a transport-independent scoped session principal and enforce expiry/permissions on TCP and WebSocket commands, nested remote commands and disconnect cleanup.
- Add transactional explicit pilot linking that proves both the old resume token and account identity `(issuer, subject)`. Never link by email, display name or pilot name. Keep links out of public snapshots and the economic ledger.
- Prevent OAuth login from issuing a permanent unrestricted resume credential. Define legacy/account-only migration, recovery, revocation and refresh behavior separately.
- Implement exact callback/state/PKCE S256 flows and safe credential storage for native, browser and terminal clients; preserve old-server capability handling without silent credential downgrade.
- Complete real admission/link/restart/authorization-denial tests before enabling any official environment.

References: [RFC 9068](https://www.rfc-editor.org/rfc/rfc9068.html), [OAuth security BCP](https://www.rfc-editor.org/rfc/rfc9700.html), [jsonwebtoken](https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/).

Approved client IDs are mandatory when enabled: 1–32 distinct, bounded identifiers, exact case-sensitive membership only. Wildcards and whitespace are rejected. The signed profile-specific client identifier must match; a portal or developer client does not gain game access merely by sharing an issuer. This supplements resource audience and scope checks, not user consent or proof of pilot ownership.
