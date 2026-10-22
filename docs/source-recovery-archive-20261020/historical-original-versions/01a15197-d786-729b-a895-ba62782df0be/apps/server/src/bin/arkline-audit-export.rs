//! Explicit local-file audit access. No listener, token, role or gameplay route.
use anyhow::{bail, ensure, Context, Result};
use std::{fs, io::Write, path::PathBuf};

fn run() -> Result<()> {
    let mut database = None;
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("arkline-audit-export --database EXISTING.sqlite --output NEW_AUDIT.json\nRequires existing OS permission to read the database. Reads a committed full economic audit without starting a server or advancing simulation. Output is confidential; never overwritten. No network or auditor credential is created.");
                return Ok(());
            }
            "--database" if database.is_none() => {
                database = Some(PathBuf::from(args.next().context("--database requires PATH")?));
            }
            "--output" if output.is_none() => {
                output = Some(PathBuf::from(args.next().context("--output requires PATH")?));
            }
            _ => bail!("unknown or duplicate argument {arg}; use --help"),
        }
    }
    let database = database.context("--database is required")?;
    let output = output.context("--output is required")?;
    // Existing source, sidecars, symlinks and hardlinks cannot be overwritten:
    // create_new below is exclusive. Check aliases before reading private data.
    ensure!(!output.try_exists()? && !output.is_symlink(), "audit output must be a new file");
    let database = database.canonicalize().context("resolve existing database")?;
    let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    let resolved_output = parent.canonicalize()?.join(output.file_name().context("output requires a filename")?);
    for suffix in ["", "-wal", "-shm", ".lock"] {
        ensure!(resolved_output != PathBuf::from(format!("{}{suffix}", database.display())), "output aliases database or sidecar");
    }
    let export = arkline_server::operator_audit::export_saved_audit(&database)?;
    let bytes = serde_json::to_vec_pretty(&export)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&output).context("create confidential new audit file")?;
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        drop(file);
        let _ = fs::remove_file(&output);
    }
    result?;
    println!("Saved full audit format {}: {} unchanged economic entries. Verify with arkline-verifier --file {}. This is a committed saved-state export, not a live simulation sample.", export.protocol_version, export.entries.len(), output.display());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("AUDIT EXPORT UNAVAILABLE: {error:#}");
        std::process::exit(1);
    }
}
