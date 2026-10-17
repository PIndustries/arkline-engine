//! Optional external account admission. Account bindings never enter public snapshots.
use crate::{access_tokens::*, *};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Binding {
    pub issuer: String,
    pub subject: String,
}
pub(crate) struct Admission {
    pub binding: Binding,
    pub link_existing: bool,
}
pub(crate) struct Authenticator {
    verifier: AccessTokenVerifier,
    pub required: bool,
    capability: AuthCapabilities,
}

pub(crate) fn authorize(
    world: &World,
    pilot: &str,
    account: &Admission,
    legacy: Option<&str>,
) -> Result<()> {
    if let Some(existing) = world.pilots.get(pilot) {
        match &existing.identity {
            Some(binding) => ensure!(
                binding == &account.binding,
                "account does not own this pilot"
            ),
            None => {
                ensure!(
                    account.link_existing,
                    "existing pilot requires explicit credential linking"
                );
                ensure!(
                    legacy.is_some_and(|s| token_hash(s) == existing.token_hash),
                    "valid legacy pilot credential required to link"
                );
            }
        }
    } else {
        ensure!(
            !account.link_existing && legacy.is_none(),
            "cannot link an unknown pilot"
        );
    }
    Ok(())
}
impl Server {
    /// Keys are operator-provisioned public JWKS, never token-controlled URLs.
    pub fn configure_auth(
        &mut self,
        config: AccessTokenConfig,
        jwks: &[u8],
        required: bool,
    ) -> Result<()> {
        ensure!(
            !self.is_local_development(),
            "local development cannot enable official account authentication"
        );
        ensure!(
            config.required_scopes.iter().any(|s| s == "arkline.play"),
            "arkline.play scope is required"
        );
        let capability = AuthCapabilities {
            issuer: config.issuer.clone(),
            audience: config.audience.clone(),
            profile: match config.profile {
                AccessTokenProfile::Rfc9068 => "rfc9068",
                AccessTokenProfile::Rauthy037 => "rauthy037",
            }
            .into(),
            required_scope: "arkline.play".into(),
            auth_required: required,
        };
        let verifier = GameAuthConfig {
            access_tokens: Some(config),
        }
        .build(jwks)?
        .context("missing verifier")?;
        self.auth = Some(Authenticator {
            verifier,
            required,
            capability,
        });
        Ok(())
    }
    /// Both public transports use this boundary. Expiry applies before any nested command.
    /// Bearer access tokens are reusable until expiry; this is not a one-use world ticket.
    pub fn handle_authenticated(
        &mut self,
        pilot: &mut Option<String>,
        expires: &mut Option<u64>,
        request: Request,
    ) -> Response {
        let now = now_ms();
        if expires.is_some_and(|e| now / 1000 >= e) {
            self.disconnect(pilot);
            *expires = None;
            return Response::error("account session expired; sign in again");
        }
        if matches!(request, Request::AuthCapabilities) {
            let mut r = Response::error("account authentication is disabled");
            r.ok = true;
            r.auth = self.auth.as_ref().map(|a| a.capability.clone());
            if r.auth.is_some() {
                r.message = "Account authentication available".into();
            }
            return r;
        }
        if let Request::AccountConnect {
            access_token,
            pilot: name,
            legacy_resume_token,
            link_existing,
            protocol_version,
            world_layout_version,
            supports_room_controls,
            supports_fleet_hangar,
            supports_pioneer_interior,
        } = request
        {
            let Some(auth) = &self.auth else {
                return Response::error("account authentication is disabled");
            };
            let identity = match auth.verifier.verify(&access_token) {
                Ok(i) => i,
                Err(_) => return Response::error("game access token rejected"),
            };
            let expiry = identity.expires_at_seconds;
            let admission = Admission {
                binding: Binding {
                    issuer: identity.issuer,
                    subject: identity.subject,
                },
                link_existing,
            };
            let r = self.handle_identity_at(
                pilot,
                Request::Connect {
                    protocol_version,
                    world_layout_version,
                    pilot: name,
                    resume_token: legacy_resume_token,
                    supports_room_controls,
                    supports_fleet_hangar,
                    supports_pioneer_interior,
                },
                now,
                Some(admission),
            );
            if r.ok {
                *expires = Some(expiry);
            }
            return r;
        }
        let connect = matches!(request, Request::Connect { .. });
        let response = self.handle_at(pilot, request, now);
        if connect && response.ok {
            *expires = None;
        }
        response
    }
}
