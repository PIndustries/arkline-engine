# Native account sign-in

This is an opt-in integration preview, not an announcement that official account login is live. A server must have account admission enabled, and the operator must register a compatible public OAuth client before this flow is usable. Existing public clients continue to use legacy pilot credentials until an explicit rollout. Browser and terminal OAuth flows, automatic refresh, a pilot picker and lost-credential recovery are not implemented in this slice.

## Configure a native client

Supply a local JSON file through `arkline-client --oauth-config /absolute/path/native-account.json`. The file contains public configuration only, never a client secret or password:

```json
{
  "server": "wss://relay.example.test/ws",
  "issuer": "https://identity.example.test/",
  "authorization_url": "https://identity.example.test/authorize",
  "token_url": "https://identity.example.test/token",
  "client_id": "arkline-native-test",
  "scopes": ["arkline.play"]
}
```

These are example addresses, not working official endpoints. Use the actual provider's registered endpoints. Authorization and token endpoints must share the configured HTTPS issuer origin. The game endpoint must use WSS. Configuration is supplied by the operator/user, never adopted from an untrusted server response. An explicit `--server` must match the configuration; local credential scopes and `--local` cannot be combined with this account configuration.

Register a public authorization-code client with S256 PKCE and the native loopback redirect `http://127.0.0.1:{ephemeral-port}/oauth/callback`. The provider must support variable loopback ports for this registered path. Do not broadly wildcard web redirects or embed a client secret in the game. Configure resource audience, access-token signing profile and `arkline.play` consistently with the [server admission configuration](game-account-admission.md). An identity-only token is not a game access token.

## User journey

1. Select the intended pilot and choose **Sign in and enter the universe**.
2. Arkline opens the system browser. Enter credentials only at the configured identity provider.
3. A short-lived loopback listener accepts the callback with the matching state; PKCE binds token exchange to this client request. Other callbacks do not consume the login.
4. The native client sends the access token over verified WSS using account admission. The server validates it and checks pilot ownership.
5. Expiration requires sign-in again. Tokens remain in memory; no permanent legacy credential is minted from OAuth login. A failed or expired account session does not fall back to legacy authentication.

Browser cancellation, callback timeout and token-exchange failure return to the client with a generic error that does not include tokens or provider response bodies. Authorization callbacks have size/time limits, and token HTTP responses have a bounded body. Network work runs outside the rendering thread.

## Existing pilots

Ordinary account login cannot claim a legacy pilot by matching its name. **Link this pilot using its saved legacy credential** explicitly supplies both proofs: the account access token and the existing endpoint-scoped pilot credential. The server stores the ownership binding transactionally without replacing the inventory or economy.

If the original credential is missing, this integration cannot recover it. It must come from the original authorized client or a separately implemented operator recovery process. A new QA pilot, changing the error text or selecting another network is not evidence that the existing pilot was recovered.

## Validation and remaining gates

Automated tests cover callback state/issuer/duplicate rejection, cancellation, bounded reads, token endpoint/pilot/expiry binding, server token validation, explicit linking, ownership rejection, session expiration and real TCP/WebSocket admission with disposable test keys. They do not establish a completed official provider/browser/native journey.

Before official enablement: register and verify the provider client/audience/scope and loopback redirect behavior; complete real browser sign-in and an existing-pilot link/restart; test rejection and expiry through the deployed TLS path; establish lost-credential recovery and revocation policy. Short-lived locally verified access tokens remain usable until expiration; immediate provider revocation is not claimed.
