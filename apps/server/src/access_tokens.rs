//! Configurable game access-token signature and claim validation.
//!
//! Only explicitly profiled `at+jwt` access tokens for a configured resource are accepted.
//! Provider profile compatibility is explicit. Pilot admission separately enforces
//! ownership, dual-proof linking and transport lifetime.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

pub const MAX_TOKEN_BYTES: usize = 12 * 1024;
pub const MAX_JWKS_BYTES: usize = 64 * 1024;
const MAX_KEYS: usize = 16;

/// Parsed from operator configuration; no environment variables are read here.
#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GameAuthConfig {
    pub access_tokens: Option<AccessTokenConfig>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessTokenConfig {
    #[serde(default)]
    pub profile: AccessTokenProfile,
    pub issuer: String,
    pub audience: String,
    pub required_scopes: Vec<String>,
    /// Explicit operator-approved game clients; no wildcard or claim-selected policy.
    pub allowed_client_ids: Vec<String>,
    pub allowed_algorithms: Vec<SigningAlgorithm>,
    /// Maximum signed exp-iat lifetime; must be 1..=900 seconds.
    pub max_lifetime_seconds: u64,
    /// Accepted clock skew; must be <=30 seconds.
    pub clock_skew_seconds: u64,
}

/// Operator-selected signed-claim contract. Never inferred from token contents.
#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccessTokenProfile {
    #[default]
    Rfc9068,
    /// Pinned upstream 0.37 format: signed `azp` and payload `typ: Bearer`.
    /// This is source-backed, not yet validated with a live game-token fixture.
    Rauthy037,
}

/// Intentionally narrow first profile. No symmetric, token-selected, or `none` algorithms.
#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum SigningAlgorithm {
    RS256,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    InvalidConfiguration,
    InvalidTrustedKeys,
    InvalidAccessToken,
    ClockUnavailable,
}
impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidConfiguration => "invalid game access-token configuration",
            Self::InvalidTrustedKeys => "invalid trusted game signing keys",
            Self::InvalidAccessToken => "game access token rejected",
            Self::ClockUnavailable => "server clock unavailable",
        })
    }
}
impl std::error::Error for ValidationError {}

/// Validated claims; ownership and session admission are enforced separately.
pub struct VerifiedIdentity {
    pub issuer: String,
    pub subject: String,
    pub client_id: String,
    pub scopes: BTreeSet<String>,
    /// The caller must enforce this on every command, not just initial admission.
    pub expires_at_seconds: u64,
}

pub struct AccessTokenVerifier {
    config: AccessTokenConfig,
    keys: Vec<(String, DecodingKey)>,
    validation: Validation,
}

impl GameAuthConfig {
    /// Builds from an operator-trusted JWKS byte buffer only. No discovery, networking,
    /// disk reads, token-provided URLs, key refresh, or fallback to legacy admission.
    /// A caller must establish issuer/key provenance before supplying this buffer.
    pub fn build(
        &self,
        trusted_jwks: &[u8],
    ) -> Result<Option<AccessTokenVerifier>, ValidationError> {
        self.access_tokens
            .as_ref()
            .map(|config| AccessTokenVerifier::new(config.clone(), trusted_jwks))
            .transpose()
    }
}

fn bounded_text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
fn scope_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c == 0x21 || (0x23..=0x5b).contains(&c) || (0x5d..=0x7e).contains(&c))
}
impl AccessTokenVerifier {
    fn new(config: AccessTokenConfig, bytes: &[u8]) -> Result<Self, ValidationError> {
        let bad = ValidationError::InvalidConfiguration;
        let issuer = Url::parse(&config.issuer).map_err(|_| bad)?;
        if issuer.scheme() != "https"
            || issuer.host_str().is_none()
            || !issuer.username().is_empty()
            || issuer.password().is_some()
            || issuer.query().is_some()
            || issuer.fragment().is_some()
            || !bounded_text(&config.issuer, 2048)
            || !bounded_text(&config.audience, 256)
            || config.audience.trim() != config.audience
            || config.allowed_client_ids.is_empty()
            || config.allowed_client_ids.len() > 32
            || !config.allowed_client_ids.iter().all(|id| {
                bounded_text(id, 256) && !id.chars().any(char::is_whitespace) && id != "*"
            })
            || config
                .allowed_client_ids
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != config.allowed_client_ids.len()
            || config.required_scopes.is_empty()
            || config.required_scopes.len() > 16
            || !config.required_scopes.iter().all(|s| scope_name(s))
            || config.required_scopes.iter().collect::<BTreeSet<_>>().len()
                != config.required_scopes.len()
            || config.allowed_algorithms != [SigningAlgorithm::RS256]
            || !(1..=900).contains(&config.max_lifetime_seconds)
            || config.clock_skew_seconds > 30
        {
            return Err(bad);
        }
        let bad = ValidationError::InvalidTrustedKeys;
        if bytes.len() > MAX_JWKS_BYTES {
            return Err(bad);
        }
        // Inspect public-key metadata before constructing reusable verification keys.
        let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| bad)?;
        let entries = raw
            .get("keys")
            .and_then(serde_json::Value::as_array)
            .ok_or(bad)?;
        if entries.is_empty() || entries.len() > MAX_KEYS {
            return Err(bad);
        }
        let jwks: JwkSet = serde_json::from_slice(bytes).map_err(|_| bad)?;
        let mut ids = BTreeSet::new();
        let mut keys = Vec::new();
        for (entry, jwk) in entries.iter().zip(&jwks.keys) {
            let id = jwk.common.key_id.as_deref().ok_or(bad)?;
            if !bounded_text(id, 128) || !ids.insert(id.to_owned()) {
                return Err(bad);
            }
            // A mixed issuer JWKS may contain other public algorithms; they are never selected.
            // Private/symmetric material is rejected, not silently accepted as a trusted key.
            if ["d", "p", "q", "dp", "dq", "qi", "oth", "k"]
                .iter()
                .any(|name| entry.get(name).is_some())
            {
                return Err(bad);
            }
            if entry.get("kty").and_then(|v| v.as_str()) != Some("RSA") {
                continue;
            }
            if entry.get("alg").and_then(|v| v.as_str()) != Some("RS256") {
                continue;
            }
            if entry.get("use").is_some_and(|v| v != "sig") {
                return Err(bad);
            }
            if entry
                .get("key_ops")
                .is_some_and(|v| v != &serde_json::json!(["verify"]))
            {
                return Err(bad);
            }
            let modulus = URL_SAFE_NO_PAD
                .decode(entry.get("n").and_then(|v| v.as_str()).ok_or(bad)?)
                .map_err(|_| bad)?;
            // Bound verification cost and enforce full-size 2048..4096-bit RSA moduli.
            let bits = modulus
                .len()
                .saturating_mul(8)
                .saturating_sub(modulus.first().map_or(8, |b| b.leading_zeros() as usize));
            if !(2048..=4096).contains(&bits)
                || modulus.first() == Some(&0)
                || entry.get("e").and_then(|v| v.as_str()) != Some("AQAB")
            {
                return Err(bad);
            }
            keys.push((id.to_owned(), DecodingKey::from_jwk(jwk).map_err(|_| bad)?));
        }
        if keys.is_empty() {
            return Err(bad);
        }
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&config.issuer]);
        validation.set_audience(&[&config.audience]);
        validation.set_required_spec_claims(&["iss", "sub", "aud", "exp"]);
        validation.validate_nbf = true;
        validation.leeway = config.clock_skew_seconds;
        Ok(Self {
            config,
            keys,
            validation,
        })
    }

    pub fn verify(&self, token: &str) -> Result<VerifiedIdentity, ValidationError> {
        let bad = ValidationError::InvalidAccessToken;
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Err(bad);
        }
        let header = decode_header(token).map_err(|_| bad)?;
        if header.alg != Algorithm::RS256
            || !matches!(header.typ.as_deref(), Some("at+jwt" | "application/at+jwt"))
            || header.crit.is_some()
            || header.enc.is_some()
            || header.zip.is_some()
            || header.jku.is_some()
            || header.jwk.is_some()
            || header.x5u.is_some()
            || header.x5c.is_some()
            || header
                .extras
                .get::<serde_json::Value>("b64")
                .map_err(|_| bad)?
                .is_some()
        {
            return Err(bad);
        }
        let kid = header.kid.as_deref().ok_or(bad)?;
        let (_, key) = self.keys.iter().find(|(id, _)| id == kid).ok_or(bad)?;
        let claims = decode::<Claims>(token, key, &self.validation)
            .map_err(|_| bad)?
            .claims;
        let client_id = match self.config.profile {
            AccessTokenProfile::Rfc9068 => claims.client_id.as_deref().ok_or(bad)?,
            AccessTokenProfile::Rauthy037 => {
                // No generic client_id/azp fallback or token-selected profile.
                if claims.typ.as_deref() != Some("Bearer") || claims.client_id.is_some() {
                    return Err(bad);
                }
                claims.azp.as_deref().ok_or(bad)?
            }
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ValidationError::ClockUnavailable)?
            .as_secs();
        if !bounded_text(&claims.sub, 256)
            || !bounded_text(client_id, 256)
            || !self
                .config
                .allowed_client_ids
                .iter()
                .any(|id| id == client_id)
            || !bounded_text(&claims.jti, 256)
            || claims.iss != self.config.issuer
            || claims.exp <= now
            || claims.iat > now.saturating_add(self.config.clock_skew_seconds)
            || claims.exp <= claims.iat
            || claims.exp - claims.iat > self.config.max_lifetime_seconds
            || claims.nbf.is_some_and(|nbf| {
                nbf >= claims.exp || nbf > now.saturating_add(self.config.clock_skew_seconds)
            })
            || claims.scope.len() > 2048
        {
            return Err(bad);
        }
        let scopes = claims.scope.split(' ').collect::<BTreeSet<_>>();
        if scopes.len() > 32
            || !scopes.iter().all(|s| scope_name(s))
            || !self
                .config
                .required_scopes
                .iter()
                .all(|s| scopes.contains(s.as_str()))
        {
            return Err(bad);
        }
        Ok(VerifiedIdentity {
            issuer: claims.iss,
            subject: claims.sub,
            client_id: client_id.to_owned(),
            scopes: scopes.into_iter().map(str::to_owned).collect(),
            expires_at_seconds: claims.exp,
        })
    }
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    client_id: Option<String>,
    azp: Option<String>,
    typ: Option<String>,
    // Reject presence even when null: no DPoP/mTLS proof validation exists here.
    #[serde(default, rename = "cnf", deserialize_with = "reject_sender_constraint")]
    _cnf: (),
    #[serde(default, rename = "act", deserialize_with = "reject_sender_constraint")]
    _act: (),
    jti: String,
    exp: u64,
    iat: u64,
    nbf: Option<u64>,
    scope: String,
}

fn reject_sender_constraint<'de, D: serde::Deserializer<'de>>(_: D) -> Result<(), D::Error> {
    Err(serde::de::Error::custom(
        "sender-constrained access tokens require proof validation",
    ))
}
