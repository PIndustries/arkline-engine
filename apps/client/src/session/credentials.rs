//! Existing credential namespaces, persistence and recovery guidance.

use super::*;

pub(crate) fn resume_guidance(message: String) -> String {
    if message.contains("resume token required for existing pilot") {
        format!("{message}. This pilot needs its saved credential from the original browser or device. Browser and native credentials are stored separately. Retrying alone cannot recover it; use the original client or an authorized recovery process.")
    } else {
        message
    }
}

pub(crate) fn credentials_path(dir: &std::path::Path, server: &str, pilot: &str) -> PathBuf {
    let identity = serde_json::to_vec(&(server, pilot)).unwrap();
    dir.join(format!(
        "pilot-{}.token",
        hex::encode(Sha256::digest(identity))
    ))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn save_token(path: &std::path::Path, token: &str) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid credential directory")?;
    fs::create_dir_all(parent).map_err(|e| format!("Cannot create credential directory: {e}"))?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        if path.exists() {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("Cannot save resume credential: {e}"))?;
    file.write_all(token.as_bytes())
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}
