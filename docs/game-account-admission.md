# Optional account admission

This integration is opt-in. It does not publish signup, provision an identity provider, recover lost legacy credentials, or replace local worlds with an official account dependency.

Server flags: `--auth-config FILE --auth-jwks PUBLIC_JWKS_FILE`; add `--auth-required` only after an explicit migration decision. Without `--auth-required`, legacy pilot credentials continue working. Required mode rejects legacy Connect; it does not grant ownership to accounts with matching names or email.

Example operator configuration (public trust metadata, no secrets):

```json
{
  "profile": "rauthy037",
  "issuer": "https://accounts.example/auth/v1",
  "audience": "arkline:example-world",
  "required_scopes": ["arkline.play"],
  "allowed_client_ids": ["arkline-native"],
  "allowed_algorithms": ["RS256"],
  "max_lifetime_seconds": 300,
  "clock_skew_seconds": 5
}
```

Copy issuer exactly from verified operator configuration. Provision a separate resource audience for each world. Configure provider issuance accordingly: the Rauthy 0.37 profile requires signed `azp`, payload `typ: Bearer`, `jti`, `iat`, `exp`, `iss`, `sub`, `aud`, `scope`, and JOSE `typ: at+jwt`. The default `rfc9068` profile instead requires `client_id`. Only approved RS256 public keys and clients are accepted; ID tokens, token-provided key URLs, sender-constrained tokens without proof handling, and delegated actor tokens are rejected. Provider compatibility is source-backed; live official token issuance/client registration remains a rollout gate.

`AuthCapabilities` returns optional `Response.auth` metadata. A client must compare it with its independently trusted configuration; a game server response must never silently change browser authorization destinations. Native authorization uses a separate configured public OAuth client, authorization code and PKCE. Account access tokens remain in memory and travel only over verified TLS outside isolated loopback tests.

`AccountConnect` supplies access token, selected pilot, protocol/capability flags, and explicit `link_existing` plus `legacy_resume_token` only when linking. New account-owned pilots receive no permanent resume credential. Existing unlinked pilots require both proofs in one transaction. Subsequent account connections use exact `(issuer, subject)`. Accounts may own multiple pilots; names select a pilot but never establish ownership. Binding conflicts fail without changing world, audit or session. Binding and private identity audit are committed with SQLite world state; public snapshots and economic ledger do not expose account subjects. Back up the complete database with its existing consistency procedure.

Both transports enforce account expiration before requests, including remote wrappers, and cap idle reads at expiration. Closing/expiring a transport uses ordinary disconnect cleanup. Short-lived bearer access tokens are reusable until expiration, not single-use tickets. This release does not promise immediate provider revocation; disable/revoke at provider and observe the configured maximum lifetime, or disconnect affected sessions operationally. No automatic legacy fallback follows failed account authentication.

Trusted JWKS is loaded once from an operator-provisioned bounded file; no request-time network fetch occurs under the world lock. For rotation, distribute overlapping old/new public keys, validate configuration, then restart the server with normal state-preserving rollout. Unknown keys fail closed until the reviewed key set is installed. Provider downtime cannot mint/refresh new tokens but already issued valid tokens work until expiry. No signing private key belongs in server configuration.

Before public enablement: verify actual provider grant/audience/scope and subject stability across registered clients; native callback and browser token architecture; TLS; existing-owner explicit linking; expiry; key rotation; rollback; and operator recovery policy. Lost old resume credentials remain an explicit recovery case, not automatic account takeover.
