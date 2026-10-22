use arkline_protocol::*;
use arkline_server::Server;
use std::{fs, path::PathBuf, process::Command};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("arkline-operator-cli-{}-{}", std::process::id(), rand::random::<u64>()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

#[test]
fn local_operator_export_preserves_full_v1_audit_and_refuses_existing_destinations() {
    let fixture = Fixture::new();
    let database = fixture.0.join("world.sqlite");
    let output = fixture.0.join("audit.json");
    let mut server = Server::open(&database).unwrap();
    let mut session = None;
    let connected = server.handle_at(&mut session, Request::Connect {
        protocol_version: PROTOCOL_VERSION, world_layout_version: 1,
        supports_room_controls: false, supports_fleet_hangar: false,
        supports_pioneer_interior: true, pilot: "LocalOwner".into(), resume_token: None,
    }, 1000);
    assert!(connected.ok, "{}", connected.message);
    let before = serde_json::to_vec(&server.export_operator_audit().unwrap()).unwrap();
    let run = |destination: &std::path::Path| Command::new(env!("CARGO_BIN_EXE_arkline-audit-export"))
        .arg("--database").arg(&database).arg("--output").arg(destination).output().unwrap();
    let exported = run(&output);
    assert!(exported.status.success(), "{}", String::from_utf8_lossy(&exported.stderr));
    let bytes = fs::read(&output).unwrap();
    let audit: LedgerExport = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(audit.protocol_version, LEDGER_FORMAT_VERSION);
    assert_eq!(audit.protocol_version, 1);
    assert_eq!(audit.snapshot.pilots[0].credits, STARTER_CREDITS);
    assert_eq!(audit.snapshot.pilots[0].ore, 0);
    arkline_verifier::verify(&audit, None).unwrap();
    assert_eq!(bytes, serde_json::to_vec_pretty(&serde_json::from_slice::<LedgerExport>(&before).unwrap()).unwrap());
    assert!(!String::from_utf8_lossy(&bytes).contains(connected.resume_token.as_ref().unwrap()));
    assert!(!String::from_utf8_lossy(&bytes).contains("token_hash"));
    assert_eq!(serde_json::to_vec(&server.export_operator_audit().unwrap()).unwrap(), before);
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&output).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert!(!run(&output).status.success());
    assert_eq!(fs::read(&output).unwrap(), bytes);
    let db_bytes = fs::read(&database).unwrap();
    assert!(!run(&database).status.success());
    assert_eq!(fs::read(&database).unwrap(), db_bytes);
    #[cfg(unix)] {
        let link = fixture.0.join("source-link.json");
        std::os::unix::fs::symlink(&database, &link).unwrap();
        assert!(!run(&link).status.success());
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&database).unwrap(), db_bytes);
    }
}
