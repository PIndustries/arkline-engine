//! Explicit isolated development realm. This does not weaken Connect authentication.
use anyhow::{bail, ensure, Context, Result};
use arkline_protocol::SceneLayoutRevision;
use rusqlite::{Connection, OptionalExtension};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};
pub fn validate_listeners(bind: &str, ws_bind: Option<&str>, origins: &[String]) -> Result<()> {
    for address in std::iter::once(bind).chain(ws_bind) {
        let address: SocketAddr = address
            .parse()
            .context("local development requires a numeric loopback socket address")?;
        ensure!(
            address.ip().is_loopback(),
            "local development listeners must bind loopback only"
        );
    }
    ensure!(
        ws_bind.is_none() || !origins.is_empty(),
        "local development WebSocket requires an explicit local --ws-origin"
    );
    for origin in origins {
        let uri: tungstenite::http::Uri = origin.parse().context("invalid local browser origin")?;
        ensure!(
            matches!(uri.scheme_str(), Some("http" | "https")),
            "local browser origin must use HTTP or HTTPS"
        );
        let authority = uri
            .authority()
            .context("local browser origin requires authority")?;
        let host = authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']');
        ensure!(
            !authority.as_str().contains('@')
                && (host == "localhost"
                    || host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())),
            "local browser origin must be loopback"
        );
        ensure!(
            uri.path() == "/" && uri.query().is_none(),
            "origin must not contain a path or query"
        );
    }
    Ok(())
}
pub(super) fn database_path(directory: &Path) -> Result<PathBuf> {
    ensure!(
        !directory.as_os_str().is_empty(),
        "local development directory required"
    );
    if let Ok(meta) = std::fs::symlink_metadata(directory) {
        ensure!(
            !meta.file_type().is_symlink() && meta.is_dir(),
            "local development directory must not be a symlink"
        );
    }
    std::fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !directory.join("world.sqlite").exists() {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let directory = std::fs::canonicalize(directory)?;
    for name in [
        "world.sqlite",
        "world.sqlite.lock",
        "world.sqlite-wal",
        "world.sqlite-shm",
    ] {
        if let Ok(meta) = std::fs::symlink_metadata(directory.join(name)) {
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "local world files must be regular files"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                ensure!(
                    meta.nlink() == 1,
                    "local world files must not be hardlinked"
                );
            }
        }
    }
    Ok(directory.join("world.sqlite"))
}
const LEGACY_REALM: &str = "legacy";
const LOCAL_LEGACY_REALM: &str = "local-development-v1";
const LOCAL_ORBITAL_REALM: &str = "local-development-orbital-v1";

fn marker_scene(marker: &str, local: bool) -> Result<SceneLayoutRevision> {
    match (local, marker) {
        (false, LEGACY_REALM) | (true, LOCAL_LEGACY_REALM) => Ok(SceneLayoutRevision::LegacyV1),
        (true, LOCAL_ORBITAL_REALM) => Ok(SceneLayoutRevision::OrbitalV1),
        _ => bail!("database identity realm does not match server mode"),
    }
}

fn stored_scene(db: &Connection) -> Result<Option<SceneLayoutRevision>> {
    let has_world: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='world')",
        [],
        |row| row.get(0),
    )?;
    if !has_world {
        return Ok(None);
    }
    let raw: Option<String> = db
        .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))
        .optional()?;
    raw.map(|raw| -> Result<SceneLayoutRevision> {
        let world: serde_json::Value = serde_json::from_str(&raw)?;
        world
            .get("scene_layout")
            .map_or(Ok(SceneLayoutRevision::LegacyV1), |value| {
                serde_json::from_value(value.clone()).map_err(Into::into)
            })
    })
    .transpose()
}

pub(super) fn validate_realm_scene(
    db: &Connection,
    local: bool,
    scene: SceneLayoutRevision,
) -> Result<()> {
    let marker: String =
        db.query_row("SELECT realm FROM identity_realm WHERE id=1", [], |row| {
            row.get(0)
        })?;
    ensure!(marker_scene(&marker, local)? == scene,
        "database scene layout does not match its identity realm; no geometry migration was attempted");
    Ok(())
}

/// The marker is an older-binary rollback guard as well as an identity boundary.
/// Old readers reject LOCAL_ORBITAL_REALM before reading any world JSON. Never
/// relabel an existing legacy realm: its missing scene field still means LegacyV1.
pub(super) fn establish_realm(
    db: &mut Connection,
    local: bool,
    existed: bool,
    creation_scene: SceneLayoutRevision,
) -> Result<SceneLayoutRevision> {
    let table: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='identity_realm')",
        [],
        |r| r.get(0),
    )?;
    let value: Option<String> = if table {
        db.query_row("SELECT realm FROM identity_realm WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()?
    } else {
        None
    };
    if let Some(value) = value {
        let scene = marker_scene(&value, local)?;
        if let Some(stored) = stored_scene(db)? {
            ensure!(stored == scene,
                "database scene layout does not match its identity realm; no geometry migration was attempted");
        }
        return Ok(scene);
    }
    if local && existed {
        bail!("local development refuses an existing unmarked database; choose a new isolated directory");
    }
    let scene = if local {
        creation_scene
    } else {
        SceneLayoutRevision::LegacyV1
    };
    if let Some(stored) = stored_scene(db)? {
        ensure!(stored == scene,
            "unmarked database scene layout is incompatible; no identity or geometry migration was attempted");
    }
    let wanted = match (local, scene) {
        (false, _) => LEGACY_REALM,
        (true, SceneLayoutRevision::LegacyV1) => LOCAL_LEGACY_REALM,
        (true, SceneLayoutRevision::OrbitalV1) => LOCAL_ORBITAL_REALM,
    };
    let tx = db.transaction()?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS identity_realm(id INTEGER PRIMARY KEY CHECK(id=1),realm TEXT NOT NULL)")?;
    tx.execute(
        "INSERT INTO identity_realm(id,realm) VALUES(1,?1)",
        [wanted],
    )?;
    tx.commit()?;
    Ok(scene)
}
#[cfg(test)]
mod tests {
    use super::*;

    // Frozen reader boundary from e156bcd33a70577d6267fc0766144bb1d7a7d5b5,
    // apps/server/src/local_development.rs::establish_realm. This intentionally
    // retains the pre-layout implementation rather than calling new helpers.
    fn legacy_establish_realm(db: &mut Connection, local: bool, existed: bool) -> Result<()> {
        let table: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='identity_realm')",
            [], |row| row.get(0),
        )?;
        let value: Option<String> = if table {
            db.query_row("SELECT realm FROM identity_realm WHERE id=1", [], |row| {
                row.get(0)
            })
            .optional()?
        } else {
            None
        };
        let wanted = if local {
            "local-development-v1"
        } else {
            "legacy"
        };
        if let Some(value) = value {
            ensure!(
                value == wanted,
                "database identity realm does not match server mode"
            );
            return Ok(());
        }
        if local && existed {
            bail!("local development refuses an existing unmarked database; choose a new isolated directory");
        }
        let tx = db.transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS identity_realm(id INTEGER PRIMARY KEY CHECK(id=1),realm TEXT NOT NULL)")?;
        tx.execute(
            "INSERT INTO identity_realm(id,realm) VALUES(1,?1)",
            [wanted],
        )?;
        tx.commit()?;
        Ok(())
    }

    #[test]
    fn orbital_marker_blocks_frozen_legacy_reader_before_any_world_write() {
        let mut db = Connection::open_in_memory().unwrap();
        assert_eq!(
            establish_realm(&mut db, true, false, SceneLayoutRevision::OrbitalV1).unwrap(),
            SceneLayoutRevision::OrbitalV1
        );
        db.execute_batch("CREATE TABLE world(id INTEGER PRIMARY KEY,json TEXT NOT NULL); CREATE TABLE ledger(sequence INTEGER PRIMARY KEY,json TEXT NOT NULL);").unwrap();
        let raw = r#"{"scene_layout":"orbital_v1","surface_props":[{"sentinel":"preserve"}]}"#;
        db.execute("INSERT INTO world(id,json) VALUES(1,?1)", [raw])
            .unwrap();
        db.execute(
            "INSERT INTO ledger(sequence,json) VALUES(1,'preserved ledger')",
            [],
        )
        .unwrap();
        let changes = db.total_changes();
        for local in [true, false] {
            assert!(legacy_establish_realm(&mut db, local, true).is_err());
            assert_eq!(db.total_changes(), changes);
            let after: String = db
                .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))
                .unwrap();
            assert_eq!(after, raw);
            let marker: String = db
                .query_row("SELECT realm FROM identity_realm WHERE id=1", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(marker, LOCAL_ORBITAL_REALM);
        }
        assert_eq!(
            establish_realm(&mut db, true, true, SceneLayoutRevision::LegacyV1).unwrap(),
            SceneLayoutRevision::OrbitalV1
        );
        validate_realm_scene(&db, true, SceneLayoutRevision::OrbitalV1).unwrap();
        assert_eq!(db.total_changes(), changes);
    }

    #[test]
    fn legacy_marker_and_missing_scene_remain_accepted_without_relabeling() {
        let mut db = Connection::open_in_memory().unwrap();
        establish_realm(&mut db, true, false, SceneLayoutRevision::LegacyV1).unwrap();
        db.execute_batch("CREATE TABLE world(id INTEGER PRIMARY KEY,json TEXT NOT NULL); INSERT INTO world(id,json) VALUES(1,'{}');").unwrap();
        let changes = db.total_changes();
        legacy_establish_realm(&mut db, true, true).unwrap();
        assert_eq!(
            establish_realm(&mut db, true, true, SceneLayoutRevision::OrbitalV1).unwrap(),
            SceneLayoutRevision::LegacyV1
        );
        validate_realm_scene(&db, true, SceneLayoutRevision::LegacyV1).unwrap();
        assert_eq!(db.total_changes(), changes);
        let marker: String = db
            .query_row("SELECT realm FROM identity_realm WHERE id=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(marker, LOCAL_LEGACY_REALM);
    }

    #[test]
    fn all_listeners_and_origins_are_loopback_fail_closed() {
        for address in [
            "0.0.0.0:0",
            "[::]:0",
            "192.0.2.1:7777",
            "localhost:7777",
            "127.0.0.1.evil:1",
        ] {
            assert!(validate_listeners(address, None, &[]).is_err());
        }
        assert!(validate_listeners(
            "127.0.0.1:0",
            Some("0.0.0.0:0"),
            &["http://localhost:9000".into()]
        )
        .is_err());
        for origin in [
            "https://play.arkline.network",
            "http://localhost@evil.test",
            "http://localhost:9000/path",
            "http://localhost:9000/?token=x",
        ] {
            assert!(validate_listeners("127.0.0.1:0", Some("[::1]:0"), &[origin.into()]).is_err());
        }
        assert!(validate_listeners(
            "127.0.0.1:0",
            Some("[::1]:0"),
            &["http://localhost:9000".into(), "http://[::1]:9000".into()]
        )
        .is_ok());
        assert!(validate_listeners("127.0.0.1:0", Some("[::1]:0"), &[]).is_err());
    }
}
