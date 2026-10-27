use arkline_server::access_tokens::{
    GameAuthConfig, ValidationError, MAX_JWKS_BYTES, MAX_TOKEN_BYTES,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use rsa::{pkcs1::EncodeRsaPrivateKey, traits::PublicKeyParts, RsaPrivateKey};
use serde_json::{json, Value};
use std::{
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

// Runtime-only disposable keys: no private fixtures or production provider material in source.
fn key() -> &'static RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap())
}
fn public_key() -> Value {
    json!({"kty":"RSA","alg":"RS256","use":"sig","kid":"fixture-1", "n":URL_SAFE_NO_PAD.encode(key().n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(key().e().to_bytes_be())})
}
fn config_json() -> Value {
    json!({"access_tokens":{"issuer":"https://issuer.example.test/","audience":"arkline:test-world","required_scopes":["game.connect"],"allowed_client_ids":["public-client"],"allowed_algorithms":["RS256"],"max_lifetime_seconds":300,"clock_skew_seconds":5}})
}
fn verifier() -> arkline_server::access_tokens::AccessTokenVerifier {
    let config: GameAuthConfig = serde_json::from_value(config_json()).unwrap();
    config
        .build(&serde_json::to_vec(&json!({"keys":[public_key()]})).unwrap())
        .unwrap()
        .unwrap()
}
fn claims() -> Value {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    json!({"iss":"https://issuer.example.test/","sub":"account-123","aud":"arkline:test-world","client_id":"public-client","jti":"fixture-token","iat":now-10,"exp":now+120,"scope":"game.connect telemetry.read"})
}
fn header() -> Header {
    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some("at+jwt".into());
    header.kid = Some("fixture-1".into());
    header
}
fn signed(header: &Header, claims: &Value) -> String {
    encode(
        header,
        claims,
        &EncodingKey::from_rsa_der(key().to_pkcs1_der().unwrap().as_bytes()),
    )
    .unwrap()
}
fn rejected(verifier: &arkline_server::access_tokens::AccessTokenVerifier, token: &str) {
    assert!(matches!(
        verifier.verify(token),
        Err(ValidationError::InvalidAccessToken)
    ));
}
#[test]
fn default_is_disabled_and_misconfiguration_is_not_silently_enabled() {
    assert!(GameAuthConfig::default()
        .build(b"not even parsed when disabled")
        .unwrap()
        .is_none());
    let empty: GameAuthConfig = serde_json::from_str("{}").unwrap();
    assert!(empty.build(b"").unwrap().is_none());
    assert!(serde_json::from_str::<GameAuthConfig>(r#"{"access_token":{}}"#).is_err());
    let bytes = serde_json::to_vec(&json!({"keys":[public_key()]})).unwrap();
    for (field, value) in [
        ("issuer", json!("http://issuer.example.test/")),
        (
            "issuer",
            json!("https://user:password@issuer.example.test/"),
        ),
        ("issuer", json!("https://issuer.example.test/?x=1")),
        ("audience", json!("")),
        ("required_scopes", json!([])),
        ("required_scopes", json!(["game.connect", "game.connect"])),
        ("required_scopes", json!(["game.connect other"])),
        ("allowed_algorithms", json!([])),
        ("max_lifetime_seconds", json!(901)),
        ("clock_skew_seconds", json!(31)),
    ] {
        let mut config = config_json();
        config["access_tokens"][field] = value;
        let config: GameAuthConfig = serde_json::from_value(config).unwrap();
        assert!(
            matches!(
                config.build(&bytes),
                Err(ValidationError::InvalidConfiguration)
            ),
            "{field}"
        );
    }
    let mut config = config_json();
    config["access_tokens"]["allowed_algorithms"] = json!(["HS256"]);
    assert!(serde_json::from_value::<GameAuthConfig>(config).is_err());
}
#[test]
fn accepts_signed_resource_access_token_without_granting_pilot_identity() {
    let verifier = verifier();
    let verified = verifier.verify(&signed(&header(), &claims())).unwrap();
    assert_eq!(verified.subject, "account-123");
    assert_eq!(verified.issuer, "https://issuer.example.test/");
    assert_eq!(verified.client_id, "public-client");
    assert!(verified.scopes.contains("game.connect"));
    let mut head = header();
    head.typ = Some("application/at+jwt".into());
    let mut value = claims();
    value["aud"] = json!(["another-resource", "arkline:test-world"]);
    assert!(verifier.verify(&signed(&head, &value)).is_ok());
}
#[test]
fn rejects_wrong_issuer_audience_missing_claims_scopes_and_time() {
    let verifier = verifier();
    let original = claims();
    let now = original["iat"].as_u64().unwrap() + 10;
    for (field, value) in [
        ("iss", json!("https://other.example.test/")),
        ("iss", json!("https://issuer.example.test")),
        ("aud", json!("arkline:production-world")),
        ("aud", json!(["openid", "public-client"])),
        ("scope", json!("openid profile")),
        ("scope", json!("game.connect.evil")),
        ("scope", json!("game.connect\ttelemetry.read")),
        ("sub", json!("")),
        ("client_id", json!("")),
        ("jti", json!("")),
        ("exp", json!(now - 60)),
        ("exp", json!(now + 900)),
        ("iat", json!(now + 60)),
        ("nbf", json!(now + 60)),
        ("exp", json!(-1)),
        ("exp", json!("9999999999")),
        ("iat", json!(u64::MAX)),
    ] {
        let mut modified = original.clone();
        modified[field] = value;
        rejected(&verifier, &signed(&header(), &modified));
    }
    for field in [
        "iss",
        "aud",
        "sub",
        "exp",
        "iat",
        "jti",
        "client_id",
        "scope",
    ] {
        let mut modified = original.clone();
        modified.as_object_mut().unwrap().remove(field);
        rejected(&verifier, &signed(&header(), &modified));
    }
}
#[test]
fn requires_every_configured_scope_and_rejects_unsigned_tokens() {
    let mut config = config_json();
    config["access_tokens"]["required_scopes"] = json!(["game.connect", "ship.control"]);
    let config: GameAuthConfig = serde_json::from_value(config).unwrap();
    let verifier = config
        .build(&serde_json::to_vec(&json!({"keys":[public_key()]})).unwrap())
        .unwrap()
        .unwrap();
    rejected(&verifier, &signed(&header(), &claims()));
    let mut value = claims();
    value["scope"] = json!("game.connect ship.control");
    assert!(verifier.verify(&signed(&header(), &value)).is_ok());
    let token = format!(
        "{}.{}.",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"at+jwt","kid":"fixture-1"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&value).unwrap())
    );
    rejected(&verifier, &token);
}

#[test]
fn rejects_id_tokens_even_with_matching_signature_audience_and_claims() {
    let verifier = verifier();
    for typ in [None, Some("JWT"), Some("id+jwt")] {
        let mut head = header();
        head.typ = typ.map(str::to_owned);
        rejected(&verifier, &signed(&head, &claims()));
    }
    // Editing an otherwise valid ID token header cannot preserve its signature.
    let mut head = header();
    head.typ = Some("JWT".into());
    let id_token = signed(&head, &claims());
    let parts: Vec<_> = id_token.split('.').collect();
    let forged = format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header()).unwrap()),
        parts[1],
        parts[2]
    );
    rejected(&verifier, &forged);
}
#[test]
fn rejects_wrong_algorithm_signature_kid_and_untrusted_header_key_sources() {
    let verifier = verifier();
    let mut head = header();
    head.alg = Algorithm::RS512;
    rejected(&verifier, &signed(&head, &claims()));
    head = header();
    head.alg = Algorithm::HS256;
    rejected(
        &verifier,
        &encode(
            &head,
            &claims(),
            &EncodingKey::from_secret(b"not-a-shared-server-secret"),
        )
        .unwrap(),
    );
    for kid in [None, Some("unknown")] {
        let mut head = header();
        head.kid = kid.map(str::to_owned);
        rejected(&verifier, &signed(&head, &claims()));
    }
    let other_key = RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
    rejected(
        &verifier,
        &encode(
            &header(),
            &claims(),
            &EncodingKey::from_rsa_der(other_key.to_pkcs1_der().unwrap().as_bytes()),
        )
        .unwrap(),
    );
    for field in ["jku", "x5u"] {
        let mut head = serde_json::to_value(header()).unwrap();
        head[field] = json!("https://untrusted.example.test/keys");
        rejected(
            &verifier,
            &signed(&serde_json::from_value(head).unwrap(), &claims()),
        );
    }
    let mut head = header();
    head.crit = Some(vec!["unknown-extension".into()]);
    rejected(&verifier, &signed(&head, &claims()));
    head = header();
    head.jwk = Some(serde_json::from_value(public_key()).unwrap());
    rejected(&verifier, &signed(&head, &claims()));
    head = header();
    head.extras.insert("b64", false);
    rejected(&verifier, &signed(&head, &claims()));
    rejected(&verifier, &"a".repeat(MAX_TOKEN_BYTES + 1));
    rejected(&verifier, "not.a.token");
}
#[test]
fn rejects_ambiguous_weak_private_oversized_or_misused_trusted_keys() {
    let config: GameAuthConfig = serde_json::from_value(config_json()).unwrap();
    let valid = public_key();
    let mut sets = vec![
        json!({"keys":[]}),
        json!({"keys":[valid.clone(),valid.clone()]}),
        json!({"keys":vec![valid.clone();17]}),
    ];
    for (field, value) in [
        ("kid", Value::Null),
        ("kid", json!("")),
        ("use", json!("enc")),
        ("key_ops", json!(["sign"])),
        ("alg", json!("RS512")),
        ("n", json!("AAE")),
        ("e", json!("Aw")),
        ("d", json!("private-material-must-never-be-loaded")),
    ] {
        let mut key = valid.clone();
        key[field] = value;
        sets.push(json!({"keys":[key]}));
    }
    // Duplicate IDs remain ambiguous even when the other key advertises a different algorithm.
    let mut duplicate = valid.clone();
    duplicate["alg"] = json!("RS512");
    sets.push(json!({"keys":[valid,duplicate]}));
    for set in sets {
        assert!(matches!(
            config.build(&serde_json::to_vec(&set).unwrap()),
            Err(ValidationError::InvalidTrustedKeys)
        ));
    }
    assert!(matches!(
        config.build(&vec![b' '; MAX_JWKS_BYTES + 1]),
        Err(ValidationError::InvalidTrustedKeys)
    ));
}
#[test]
fn explicit_key_rotation_requires_new_trusted_configuration() {
    let config: GameAuthConfig = serde_json::from_value(config_json()).unwrap();
    let previous = verifier();
    let mut next_key = public_key();
    next_key["kid"] = json!("fixture-2");
    let mut head = header();
    head.kid = Some("fixture-2".into());
    let token = signed(&head, &claims());
    rejected(&previous, &token);
    let next = config
        .build(&serde_json::to_vec(&json!({"keys":[next_key]})).unwrap())
        .unwrap()
        .unwrap();
    assert!(next.verify(&token).is_ok());
    rejected(&next, &signed(&header(), &claims()));
    // A failed replacement cannot mutate the existing immutable verifier.
    assert!(config.build(b"broken JWKS").is_err());
    assert!(next.verify(&token).is_ok());
}

fn rauthy_claims() -> Value {
    let mut claims = claims();
    claims.as_object_mut().unwrap().remove("client_id");
    claims["azp"] = json!("public-client");
    claims["typ"] = json!("Bearer");
    claims["aud"] = json!(["public-client", "arkline:test-world"]);
    claims
}
fn rauthy_verifier() -> arkline_server::access_tokens::AccessTokenVerifier {
    let mut config = config_json();
    config["access_tokens"]["profile"] = json!("rauthy037");
    let config: GameAuthConfig = serde_json::from_value(config).unwrap();
    config
        .build(&serde_json::to_vec(&json!({"keys":[public_key()]})).unwrap())
        .unwrap()
        .unwrap()
}
#[test]
fn rauthy_profile_is_explicit_and_never_a_generic_claim_fallback() {
    let strict = verifier();
    let rauthy = rauthy_verifier();
    let token = signed(&header(), &rauthy_claims());
    assert_eq!(rauthy.verify(&token).unwrap().client_id, "public-client");
    rejected(&strict, &token);
    rejected(&rauthy, &signed(&header(), &claims()));
    let mut missing_azp = rauthy_claims();
    missing_azp.as_object_mut().unwrap().remove("azp");
    missing_azp["client_id"] = json!("public-client");
    rejected(&rauthy, &signed(&header(), &missing_azp));
    for (field, value) in [
        ("azp", json!("")),
        ("azp", Value::Null),
        ("typ", json!("Id")),
        ("typ", Value::Null),
        ("typ", json!("bearer")),
        ("client_id", json!("conflicting-client")),
        ("aud", json!(["public-client"])),
        ("scope", json!("openid profile")),
        ("iss", json!("https://other.example.test/")),
    ] {
        let mut value_claims = rauthy_claims();
        value_claims[field] = value;
        rejected(&rauthy, &signed(&header(), &value_claims));
    }
    let mut head = header();
    head.typ = Some("JWT".into());
    rejected(&rauthy, &signed(&head, &rauthy_claims()));
    let mut id_claims = rauthy_claims();
    id_claims["typ"] = json!("Id");
    // Even a signed ID payload under an access-token JOSE type is denied.
    rejected(&rauthy, &signed(&header(), &id_claims));
    let mut config = config_json();
    config["access_tokens"]["profile"] = json!("automatic");
    assert!(serde_json::from_value::<GameAuthConfig>(config).is_err());
}
#[test]
fn sender_constrained_tokens_are_rejected_in_every_profile_until_proofs_exist() {
    for (verifier, claims) in [(verifier(), claims()), (rauthy_verifier(), rauthy_claims())] {
        for cnf in [
            json!({"jkt":"thumbprint"}),
            json!({"x5t#S256":"thumbprint"}),
            json!({}),
            Value::Null,
        ] {
            let mut value = claims.clone();
            value["cnf"] = cnf;
            rejected(&verifier, &signed(&header(), &value));
        }
        assert!(verifier.verify(&signed(&header(), &claims)).is_ok());
    }
}

#[test]
fn approved_clients_are_exact_bounded_and_profile_specific() {
    let keys = serde_json::to_vec(&json!({"keys":[public_key()]})).unwrap();
    let mut missing = config_json();
    missing["access_tokens"]
        .as_object_mut()
        .unwrap()
        .remove("allowed_client_ids");
    assert!(serde_json::from_value::<GameAuthConfig>(missing).is_err());
    for ids in [
        json!([]),
        json!([""]),
        json!(["*"]),
        json!(["public-client", "public-client"]),
        json!([" public-client"]),
        json!(["public client"]),
        json!(["x".repeat(257)]),
        json!(vec!["x"; 33]),
    ] {
        let mut config = config_json();
        config["access_tokens"]["allowed_client_ids"] = ids;
        let config: GameAuthConfig = serde_json::from_value(config).unwrap();
        assert!(matches!(
            config.build(&keys),
            Err(ValidationError::InvalidConfiguration)
        ));
    }
    for (profile, field, base) in [
        ("rfc9068", "client_id", claims()),
        ("rauthy037", "azp", rauthy_claims()),
    ] {
        let mut config = config_json();
        config["access_tokens"]["profile"] = json!(profile);
        config["access_tokens"]["allowed_client_ids"] = json!(["public-client", "second-client"]);
        let config: GameAuthConfig = serde_json::from_value(config).unwrap();
        let verifier = config.build(&keys).unwrap().unwrap();
        for client in ["public-client", "second-client"] {
            let mut value = base.clone();
            value[field] = json!(client);
            assert_eq!(
                verifier
                    .verify(&signed(&header(), &value))
                    .unwrap()
                    .client_id,
                client
            );
        }
        for client in [
            "Public-client",
            "public-client-other",
            "",
            "portal-client",
            "public-client ",
        ] {
            let mut value = base.clone();
            value[field] = json!(client);
            rejected(&verifier, &signed(&header(), &value));
        }
    }
}

fn admitted_server() -> arkline_server::Server {
    let mut server = arkline_server::Server::open(":memory:").unwrap();
    configure(&mut server, false);
    server
}
fn configure(server: &mut arkline_server::Server, required: bool) {
    let mut cfg = config_json()["access_tokens"].clone();
    cfg["required_scopes"] = json!(["arkline.play"]);
    server
        .configure_auth(
            serde_json::from_value(cfg).unwrap(),
            &serde_json::to_vec(&json!({"keys":[public_key()]})).unwrap(),
            required,
        )
        .unwrap();
}
fn legacy(name: &str, token: Option<String>) -> arkline_protocol::Request {
    arkline_protocol::Request::Connect {
        pilot: name.into(),
        resume_token: token,
        protocol_version: arkline_protocol::PROTOCOL_VERSION,
        supports_room_controls: true,
        world_layout_version: 1,
        supports_fleet_hangar: true,
        supports_pioneer_interior: true,
        supports_docked_traversal_v2: false,
    }
}
fn account(
    name: &str,
    subject: &str,
    link: bool,
    legacy_resume_token: Option<String>,
) -> arkline_protocol::Request {
    let mut c = claims();
    c["scope"] = json!("arkline.play");
    c["sub"] = json!(subject);
    arkline_protocol::Request::AccountConnect {
        access_token: signed(&header(), &c),
        pilot: name.into(),
        legacy_resume_token,
        link_existing: link,
        protocol_version: arkline_protocol::PROTOCOL_VERSION,
        supports_room_controls: true,
        world_layout_version: 1,
        supports_fleet_hangar: true,
        supports_pioneer_interior: true,
        supports_docked_traversal_v2: false,
    }
}
#[test]
fn dual_proof_binding_and_expiry_preserve_legacy_ownership() {
    let mut s = admitted_server();
    let mut p = None;
    let mut expiry = None;
    let r = s.handle(&mut p, legacy("Owner", None));
    assert!(r.ok);
    let old = r.resume_token.unwrap();
    let original = r.snapshot.unwrap();
    s.disconnect(&mut p);
    assert!(
        !s.handle_authenticated(&mut p, &mut expiry, account("Owner", "sub1", false, None))
            .ok
    );
    assert!(
        !s.handle_authenticated(
            &mut p,
            &mut expiry,
            account("Owner", "sub1", true, Some("wrong".into()))
        )
        .ok
    );
    assert!(p.is_none());
    let r = s.handle_authenticated(
        &mut p,
        &mut expiry,
        account("Owner", "sub1", true, Some(old.clone())),
    );
    assert!(r.ok, "{}", r.message);
    assert!(r.resume_token.is_none());
    assert!(expiry.is_some());
    assert_eq!(
        r.snapshot.as_ref().unwrap().ledger_hash,
        original.ledger_hash
    );
    assert_eq!(
        r.snapshot.as_ref().unwrap().pilots[0].credits,
        original.pilots[0].credits
    );
    assert!(!serde_json::to_string(&r).unwrap().contains("sub1"));
    assert!(
        !s.handle_authenticated(
            &mut p,
            &mut expiry,
            account("Owner", "other", true, Some(old.clone()))
        )
        .ok
    );
    assert!(
        s.handle_authenticated(&mut p, &mut expiry, account("Owner", "sub1", false, None))
            .ok
    );
    expiry = Some(1);
    assert!(
        !s.handle_authenticated(&mut p, &mut expiry, arkline_protocol::Request::Undock)
            .ok
    );
    assert!(p.is_none());
    assert!(
        s.handle_authenticated(&mut p, &mut expiry, legacy("Owner", Some(old)))
            .ok
    );
    assert!(expiry.is_none());
}
#[test]
fn account_new_pilot_no_permanent_credential_and_required_policy() {
    let mut s = admitted_server();
    let mut p = None;
    let mut expiry = None;
    configure(&mut s, true);
    assert!(
        !s.handle_authenticated(&mut p, &mut expiry, legacy("New", None))
            .ok
    );
    let r = s.handle_authenticated(&mut p, &mut expiry, account("New", "sub2", false, None));
    assert!(r.ok, "{}", r.message);
    assert!(r.resume_token.is_none());
    s.disconnect(&mut p);
    expiry = None;
    assert!(
        !s.handle_authenticated(&mut p, &mut expiry, legacy("New", None))
            .ok
    );
    assert!(
        !s.handle_authenticated(&mut p, &mut expiry, account("New", "wrong", false, None))
            .ok
    );
    assert!(
        s.handle_authenticated(&mut p, &mut expiry, account("New", "sub2", false, None))
            .ok
    );
    let caps = s
        .handle_authenticated(
            &mut p,
            &mut expiry,
            arkline_protocol::Request::AuthCapabilities,
        )
        .auth
        .unwrap();
    assert!(caps.auth_required);
    assert_eq!(caps.required_scope, "arkline.play");
}
#[test]
fn binding_survives_restart_and_failed_token_changes_nothing() {
    let path = std::env::temp_dir().join(format!(
        "arkline-account-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let mut s = arkline_server::Server::open(&path).unwrap();
    configure(&mut s, false);
    let mut p = None;
    let mut expiry = None;
    let r = s.handle_authenticated(
        &mut p,
        &mut expiry,
        account("Persistent", "stable", false, None),
    );
    assert!(r.ok);
    let hash = r.snapshot.unwrap().ledger_hash;
    drop(s);
    let mut s = arkline_server::Server::open(&path).unwrap();
    configure(&mut s, false);
    p = None;
    expiry = None;
    assert!(
        !s.handle_authenticated(
            &mut p,
            &mut expiry,
            account("Persistent", "intruder", false, None)
        )
        .ok
    );
    let r = s.handle_authenticated(
        &mut p,
        &mut expiry,
        account("Persistent", "stable", false, None),
    );
    assert!(r.ok);
    assert_eq!(r.snapshot.unwrap().ledger_hash, hash);
    drop(s);
    let db = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM identity_audit", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    drop(db);
    std::fs::remove_file(&path).unwrap();
    let _ = std::fs::remove_file(path.with_extension("sqlite.lock"));
}
#[test]
fn failed_signed_claim_and_public_caps_do_not_create_or_link() {
    let mut s = admitted_server();
    let mut p = None;
    let mut e = None;
    let existing = s.handle(&mut p, legacy("Untouched", None));
    let token = existing.resume_token.unwrap();
    s.disconnect(&mut p);
    let mut request = account("Untouched", "claimant", true, Some(token.clone()));
    if let arkline_protocol::Request::AccountConnect { access_token, .. } = &mut request {
        access_token.push('x');
    }
    assert!(!s.handle_authenticated(&mut p, &mut e, request).ok);
    assert!(p.is_none());
    assert!(
        !s.handle_authenticated(
            &mut p,
            &mut e,
            account("Untouched", "claimant", false, None)
        )
        .ok
    );
    assert!(s.handle(&mut p, legacy("Untouched", Some(token))).ok);
    assert!(s
        .check_public_request(None, &account("Another", "x", false, None), 1)
        .is_err());
}

#[test]
fn real_tcp_and_websocket_share_account_admission() {
    use std::{
        io::{BufRead, Write},
        net::{TcpListener, TcpStream},
        process::{Command, Stdio},
        time::Duration,
    };
    struct Fixture {
        child: std::process::Child,
        dir: std::path::PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let tcp_addr = tcp.local_addr().unwrap();
    let ws = TcpListener::bind("127.0.0.1:0").unwrap();
    let ws_addr = ws.local_addr().unwrap();
    let dir = std::env::temp_dir().join(format!("arkline-admission-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let mut config = config_json()["access_tokens"].clone();
    config["required_scopes"] = json!(["arkline.play"]);
    std::fs::write(
        dir.join("config.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("jwks.json"),
        serde_json::to_vec(&json!({"keys":[public_key()]})).unwrap(),
    )
    .unwrap();
    drop(tcp);
    drop(ws);
    let child = Command::new(env!("CARGO_BIN_EXE_arkline-server"))
        .args([
            "--bind",
            &tcp_addr.to_string(),
            "--ws-bind",
            &ws_addr.to_string(),
            "--auth-required",
            "--auth-config",
        ])
        .arg(dir.join("config.json"))
        .arg("--auth-jwks")
        .arg(dir.join("jwks.json"))
        .arg("--db")
        .arg(dir.join("world.sqlite"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _fixture = Fixture { child, dir };
    let stream = (0..100)
        .find_map(|_| match TcpStream::connect(tcp_addr) {
            Ok(s) => Some(s),
            Err(_) => {
                std::thread::sleep(Duration::from_millis(20));
                None
            }
        })
        .expect("local fixture started");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut tcp = std::io::BufReader::new(stream);
    let send = |tcp: &mut std::io::BufReader<TcpStream>, r: arkline_protocol::Request| {
        serde_json::to_writer(tcp.get_mut(), &r).unwrap();
        tcp.get_mut().write_all(b"\n").unwrap();
        let mut line = String::new();
        tcp.read_line(&mut line).unwrap();
        serde_json::from_str::<arkline_protocol::Response>(&line).unwrap()
    };
    assert!(!send(&mut tcp, legacy("Transport", None)).ok);
    assert!(
        send(
            &mut tcp,
            account("Transport", "transport-subject", false, None)
        )
        .ok
    );
    let socket = TcpStream::connect(ws_addr).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let (mut websocket, _) = tungstenite::client(format!("ws://{ws_addr}/ws"), socket).unwrap();
    websocket
        .send(tungstenite::Message::Text(
            serde_json::to_string(&account("Transport", "wrong", false, None))
                .unwrap()
                .into(),
        ))
        .unwrap();
    let rejected: arkline_protocol::Response =
        serde_json::from_str(websocket.read().unwrap().to_text().unwrap()).unwrap();
    assert!(!rejected.ok);
    websocket
        .send(tungstenite::Message::Text(
            serde_json::to_string(&account("Transport", "transport-subject", false, None))
                .unwrap()
                .into(),
        ))
        .unwrap();
    let admitted: arkline_protocol::Response =
        serde_json::from_str(websocket.read().unwrap().to_text().unwrap()).unwrap();
    assert!(admitted.ok);
    assert!(admitted.resume_token.is_none());
    assert!(send(&mut tcp, arkline_protocol::Request::Undock).ok);
    websocket
        .send(tungstenite::Message::Text(
            serde_json::to_string(&arkline_protocol::Request::Snapshot)
                .unwrap()
                .into(),
        ))
        .unwrap();
    let snapshot: arkline_protocol::Response =
        serde_json::from_str(websocket.read().unwrap().to_text().unwrap()).unwrap();
    assert!(snapshot.ok);
    assert_ne!(
        snapshot.snapshot.unwrap().pilots[0].location,
        arkline_protocol::Location::Docked
    );
}
#[test]
fn conflicting_links_serialize_to_one_owner() {
    let mut s = admitted_server();
    let mut p = None;
    let old = s.handle(&mut p, legacy("Race", None)).resume_token.unwrap();
    s.disconnect(&mut p);
    let server = std::sync::Arc::new(std::sync::Mutex::new(s));
    let attempts = [
        account("Race", "one", true, Some(old.clone())),
        account("Race", "two", true, Some(old)),
    ];
    let threads: Vec<_> = attempts
        .into_iter()
        .map(|request| {
            let s = server.clone();
            std::thread::spawn(move || {
                s.lock()
                    .unwrap()
                    .handle_authenticated(&mut None, &mut None, request)
                    .ok
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| t.join().unwrap() as usize)
            .sum::<usize>(),
        1
    );
}
