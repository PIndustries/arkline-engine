//! Explicit isolated development realm. This does not weaken Connect authentication.
use anyhow::{bail, ensure, Context, Result};
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
pub(super) fn establish_realm(db: &mut Connection, local: bool, existed: bool) -> Result<()> {
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
#[cfg(test)]
mod tests {
    use super::*;
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
