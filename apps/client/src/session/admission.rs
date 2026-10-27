//! World-layout admission and response consistency gates.

use super::*;

/// Old snapshots retain their exact layout. A declared descriptor must validate;
/// unsupported revisions are never replaced with guessed client coordinates.
pub(crate) fn snapshot_world_layout(snapshot: &Snapshot) -> Result<&WorldLayoutDefinition, String> {
    static LEGACY: std::sync::OnceLock<WorldLayoutDefinition> = std::sync::OnceLock::new();
    let definition = snapshot
        .world_layout
        .as_ref()
        .unwrap_or_else(|| LEGACY.get_or_init(legacy_world_layout));
    definition.validate().map_err(|error| {
        format!("Unsupported world layout: {error}. Reconnect with a compatible client/server.")
    })?;
    Ok(definition)
}

pub(crate) fn validate_snapshot_scene(
    snapshot: &Snapshot,
    assets: Option<&AssetsView>,
) -> Result<(), String> {
    let layout = snapshot_world_layout(snapshot)?;
    if assets
        .and_then(|assets| assets.hangar.as_ref())
        .is_some_and(|hangar| hangar.layout_revision != layout.fleet_layout)
    {
        return Err("Unsupported world layout: station hangar revision contradicts the snapshot descriptor. Reconnect with a compatible client/server.".into());
    }
    Ok(())
}

/// Admission and polling must reject unsupported geometry before credentials,
/// Ready events or a response can escape the transport worker.
pub(crate) fn validate_world_response(response: &Response) -> Result<(), String> {
    if let Some(snapshot) = response.snapshot.as_ref() {
        validate_snapshot_scene(snapshot, response.assets.as_ref())?;
    }
    if let Some(ledger) = response.ledger.as_ref() {
        validate_snapshot_scene(&ledger.snapshot, response.assets.as_ref())?;
        if let Some(snapshot) = response.snapshot.as_ref() {
            if snapshot_world_layout(snapshot)? != snapshot_world_layout(&ledger.snapshot)? {
                return Err("Unsupported world layout: response and ledger describe different realms. Reconnect with a compatible client/server.".into());
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_world_admission_response(response: &Response) -> Result<(), String> {
    if response.ok && response.snapshot.is_none() {
        return Err("Unsupported world layout: admission response omitted its authoritative world snapshot. Reconnect with a compatible client/server.".into());
    }
    validate_world_response(response)
}
