//! Session infrastructure facade; callers keep their existing crate-root names.

use super::*;

mod admission;
mod credentials;
mod local_host;
#[cfg(not(target_arch = "wasm32"))]
mod native;

pub(crate) use admission::{
    snapshot_world_layout, validate_snapshot_scene, validate_world_admission_response,
    validate_world_response,
};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use credentials::save_token;
pub(crate) use credentials::{credentials_path, resume_guidance};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use local_host::start_local_server;
pub(crate) use local_host::LocalServer;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use native::exchange_with_timeout;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{connect_transport, exchange, worker, WorkerIdentity};
