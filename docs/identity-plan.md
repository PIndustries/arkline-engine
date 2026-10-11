# Website and game identity: current state and integration plan

**Status: private Rust identity foundation deployed and tested; public account access and game integration are not implemented.** The private provider has passed isolated account and OIDC lifecycle checks. Public signup and the account-domain route remain disabled. A website account cannot currently authenticate an Arkline pilot; no game token exchange, account-linking migration or client browser callback is implemented.

## Current private foundation

Arkline has a separate private Rust account-service repository and account store, using a pinned maintained Rust identity provider. It is deployed as a private, persistent workload with registration disabled and no public account route. It does not import Bastion accounts or change existing game pilot credentials.

Isolated acceptance covered registration with captured verification mail, password login and recovery, profile changes, logout, authorization code with S256 PKCE, token signature/claim checks, replay rejection, refresh/revocation, signing-key rotation, restart and backup restoration. The private workload's health, HTTPS issuer configuration and secure host-only cookie attributes were also checked. These results establish a foundation, not a completed public account launch or game sign-in journey.

Owner administrator assignment/handoff, final public proxy/browser acceptance and game account-to-pilot linking remain separate work. Passkey support is not advertised as tested. Exact private runtime credentials and operator access details are intentionally outside this public document.

## Historical website investigation

The following inventory records the earlier Bastion/game investigation. It explains why existing website sessions were not reused as game credentials; it is not the current deployment inventory for the new private Rust provider.

| Component | Source-backed capability at review | Missing boundary at review |
| --- | --- | --- |
| Bastion website | Magic-link and passkey sign-in; renewed signed session cookie; authenticated user and organization roles | No game credential issuance or pilot ownership mapping |
| Bastion enterprise SSO | OIDC relying-party/client flow per organization, issuer discovery, authorization-code exchange, signed state and session nonce, RS256 ID-token signature/issuer/audience/expiry/nonce checks | Not an authorization server; current exchange does not implement PKCE |
| Arkline server | Pilot registration and hashed bearer resume credentials bound to transport sessions | No central account subject, OAuth access-token validation, recovery or account revocation |
| Arkline clients | Saved credentials scoped to connection endpoint; native/browser network transports | No system-browser authorization flow, callback handling or account picker |
| Hosted infrastructure | Separate website, static browser service, production/test worlds and TLS edge | No verified common identity-provider deployment or shared trust configuration |

Bastion source locations: `lib/bastion_web/controllers/oidc_controller.ex`, `lib/p/industries/drivers/oidc/client.ex`, `lib/p/industries/facades/orgs.ex`, `lib/bastion_web/plugs/auth.ex`, `lib/bastion_web/router.ex`, and `lib/bastion_web/endpoint.ex`. Its current routes include `/login`, `/auth/magic/:token`, passkey challenge/verify, `/auth/oidc/:org_slug/start`, and `/auth/oidc/callback`. These are website login routes, not a documented OAuth issuer's authorization/token/JWKS service.

Organization configuration stores `oidc.enabled`, `issuer`, `client_id` and a separate `secret_ref`. The current callback uses the canonical Bastion endpoint URL, then redirects into the website. The cookie is configured as signed with SameSite=Lax; that is not a cross-domain game-session mechanism. Website roles must never silently become game corporation, world-operator or delegated-pilot privileges.

At that earlier review, read-only deployment discovery found no positively identified central identity-provider workload. Explicit Bastion Deployment variables describe Arkline release links, while other environment configuration comes from a Secret reference. Workload names and environment variable names alone cannot establish whether an external organization provider is configured. No credential values or organization identity data were exported during that investigation, and no provider was selected or installed in that pass. The private Rust foundation described above was implemented and verified afterward; public availability and game trust integration remain unimplemented.

## Required private Rust identity boundary

The account-registration, account-management and OAuth/OIDC service must be a separate **private Rust application**. It must not be implemented in Bastion/Elixir or embedded into the public game engine. Bastion may continue serving marketing pages; this does not make it the account authority. The private provider foundation is deployed; no public account portal or game admission integration is enabled.

The public engine should integrate only through standard, configurable OpenID Connect/OAuth interfaces. Forks and independent operators must be able to select their own compliant issuer and register their own client IDs/redirects; official account infrastructure is not a mandatory proprietary dependency for local worlds. Provider configuration is operator-controlled, not supplied by an untrusted game server response.

The private service uses a maintained provider for authorization code + PKCE, durable account/session storage, verification/recovery, key rotation and replay protection. Isolated acceptance exercises these boundaries; production public access still needs its own validation. Maintained Rust OIDC **client** crates do not by themselves implement an authorization server. A standards discovery document or a signed token alone is not a complete OAuth service. Implementation and conformance testing are separate work from publishing these docs.

## Integration design

Arkline uses a separate private Rust account-service foundation and account store; accounts.arkline.network remains the planned public account host, with no account route enabled yet. Do not implicitly import or link Bastion users by matching email. Use a maintained Rust authorization-server component, with original Arkline account integration, rather than growing a custom OAuth issuer inside the game. Website and game clients can use this provider through their own registered standard interfaces.

```text
Native app or browser game -- external browser authorization --> identity provider
             |                    code + PKCE / verified callback        |
             +<--------------------------------------------------------+
             |
             +--> authenticated account-to-world admission
                        | stable account subject + world-scoped permission
                        v
                  Arkline pilot session

Website ------------------ same provider, separate client/session
```

Use Authorization Code with PKCE S256, explicit redirect registration, state/nonce validation and short-lived credentials. Native apps should use the system browser and a registered loopback or platform-verified redirect, not an embedded password form or shipped client secret. This follows [OAuth for Native Apps, RFC 8252](https://www.rfc-editor.org/rfc/rfc8252.html). Apply current token/redirect/replay defenses from [OAuth Security BCP, RFC 9700](https://www.rfc-editor.org/rfc/rfc9700.html).

For the browser, evaluate a same-origin backend-for-frontend that keeps refresh credentials out of JavaScript, with short-lived world admission in memory. A pure public browser client still needs PKCE and an explicit refresh/token-theft policy. Do not transplant the current game resume-token local storage pattern onto central account refresh credentials. Browser architecture tradeoffs and requirements are documented in [RFC 10017](https://www.rfc-editor.org/rfc/rfc10017.html).

## Identity and existing-world preservation

1. Add a stable external account key based on validated issuer and subject, never pilot name, email or mutable display name. If pairwise provider subjects differ by client, use a deliberately configured account-linking service or common subject policy; do not assume cross-client subjects match.
2. Keep pilot identity and world identity separate. One account may have distinct authorized pilots in production and test. Existing SQLite pilot records, inventories and ledger history remain unchanged by linking.
3. Claim an existing pilot only while proving both the old pilot credential and the new account authorization. Name/email matching is insufficient. Record a durable, audited one-time link with uniqueness constraints and race tests. Lost old credentials require a separate operator recovery policy, not automatic account takeover.
4. Admit the client with a short-lived audience-restricted credential or single-use world ticket. Validate issuer, audience, expiration, signature/key policy and replay protection. A website ID token is not automatically an API access token. Define which component issues world tickets, how signing keys rotate and how tickets are consumed before implementing it.
5. Define logout, revocation, refresh, provider downtime and already-connected-session behavior. Preserve standalone/local worlds and an explicit migration window; do not silently invalidate all current pilot credentials on rollout.

The current game supports explicit worker delegation and remote-control leases. Account linking must preserve those checks. Possession of a website session or a broad organization role must not bypass fleet grants, corporation withdrawal rules, world separation or worker ownership.

## Work packages and acceptance gates

| Stage | Concrete deliverable | Required evidence |
| --- | --- | --- |
| Public provider readiness | Publish the privately validated provider through a reviewed account-host route; register production website/native/browser clients | Private lifecycle checks already pass; verify final TLS/proxy/browser/mail/admin behavior before public signup |
| Website integration | Maintained OIDC client configuration, session renewal, explicit existing-user linking | CSRF/state/nonce/code-replay tests; correct logout and same-account behavior |
| Durable account mapping | World/account/pilot ownership schema and migration | Backup/restore, duplicate/race rejection, no changed economic balances or history |
| World admission | Versioned authenticated-connect contract and validated short-lived ticket/token | Wrong issuer/audience/world, expired/replayed token, key rotation and revocation rejected |
| Native/browser UX | Sign-in button, external browser redirect, cancellation, account/pilot selection, safe credential storage | Real macOS, Windows and browser journeys; no token in URL logs or crash reports |
| Recovery and rollout | Account recovery, old-pilot claim window, rollback and operator runbook | Existing pilot resumes, correct account cannot claim another pilot, provider outage fails safely |

Existing Bastion OIDC client code should be reviewed before reuse: PKCE is absent, callback routing is website-specific, and token/userinfo binding and claim edge cases need targeted tests. Source inspection is not a security certification or evidence that a live provider is configured. The integration should use tested libraries and explicit trust boundaries, with a real end-to-end pilot-linking test before changing public admission.
