use arkline_protocol::{Request, PROTOCOL_VERSION};
use arkline_server::Server;
fn connect(token: Option<String>) -> Request {
    serde_json::from_value(serde_json::json!({"command":"connect","protocol_version":PROTOCOL_VERSION,"pilot":"LocalEngineer","resume_token":token})).unwrap()
}
fn directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("arkline-local-{}", rand::random::<u64>()))
}
#[test]
fn local_identity_is_persistent_authenticated_and_cannot_resume_in_other_realm() {
    let dir = directory();
    let mut local = Server::open_local_development(&dir).unwrap();
    assert!(local.is_local_development());
    let mut session = None;
    let r = local.handle_at(&mut session, connect(None), 0);
    assert!(r.ok);
    let token = r.resume_token.unwrap();
    assert!(!local.handle_at(&mut None, connect(None), 0).ok);
    assert!(
        !local
            .handle_at(&mut None, connect(Some("wrong".into())), 0)
            .ok
    );
    let before = r.snapshot.unwrap().pilots[0].credits;
    drop(local);
    assert!(Server::open(dir.join("world.sqlite")).is_err());
    let mut local = Server::open_local_development(&dir).unwrap();
    let r = local.handle_at(&mut None, connect(Some(token.clone())), 0);
    assert!(r.ok);
    assert_eq!(r.snapshot.unwrap().pilots[0].credits, before);
    let other = directory();
    std::fs::create_dir_all(&other).unwrap();
    let mut ordinary = Server::open(other.join("world.sqlite")).unwrap();
    assert!(!ordinary.is_local_development());
    assert!(
        !ordinary
            .handle_at(&mut None, connect(Some(token.clone())), 0)
            .ok
    );
    assert!(ordinary.handle_at(&mut None, connect(None), 0).ok);
    assert!(!ordinary.handle_at(&mut None, connect(Some(token)), 0).ok);
    drop(ordinary);
    assert!(Server::open_local_development(&other).is_err());
    drop(local);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(other).unwrap();
}
#[test]
fn existing_unmarked_world_is_legacy_only_and_local_refusal_preserves_contents() {
    let dir = directory();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("world.sqlite");
    {
        let mut s = Server::open(&path).unwrap();
        assert!(s.handle_at(&mut None, connect(None), 0).ok);
    }
    let before = {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("DROP TABLE identity_realm").unwrap();
        db.query_row::<String, _, _>("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap()
    };
    assert!(Server::open_local_development(&dir).is_err());
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        let after: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after);
    }
    assert!(Server::open(&path).is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn public_binding_and_db_override_fail_before_creating_world() {
    for args in [
        vec!["--bind", "0.0.0.0:0"],
        vec![
            "--ws-bind",
            "0.0.0.0:0",
            "--ws-origin",
            "http://localhost:1",
        ],
        vec!["--db", "ignored.sqlite"],
    ] {
        let dir = directory();
        let r = std::process::Command::new(env!("CARGO_BIN_EXE_arkline-server"))
            .arg("--local-development")
            .arg(&dir)
            .args(args)
            .output()
            .unwrap();
        assert!(!r.status.success());
        assert!(!dir.exists());
    }
}
#[cfg(unix)]
#[test]
fn development_directory_and_db_cannot_alias_another_world() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = directory();
    std::fs::create_dir_all(&root).unwrap();
    let real = root.join("real");
    std::fs::create_dir(&real).unwrap();
    let alias = root.join("alias");
    symlink(&real, &alias).unwrap();
    assert!(Server::open_local_development(&alias).is_err());
    {
        let _s = Server::open_local_development(&real).unwrap();
        assert_eq!(
            std::fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let other = root.join("other");
    std::fs::create_dir(&other).unwrap();
    symlink(real.join("world.sqlite"), other.join("world.sqlite")).unwrap();
    assert!(Server::open_local_development(&other).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
