//! Immutable, validated spatial material revisions shared by editor and renderers.
//! This is authoring presentation data, never an authoritative world command.
use arkline_protocol::material_compile::{lower_material, LoweredMaterial};
use arkline_protocol::material_graph::{
    AssetMaterialBinding, MaterialDiagnostic, MaterialGraphDocument,
};
use std::sync::Arc;
pub(crate) const MATERIAL_GRAPH_FIRST_LINE: usize = 1000;

/// A new project/controller must advance generation; document edits advance revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MaterialStamp {
    pub generation: u64,
    pub revision: u64,
}

/// Only the trusted graph lowerer can supply the shader source in a revision.
#[derive(Clone, Debug)]
pub struct MaterialRevision {
    stamp: MaterialStamp,
    lowered: LoweredMaterial,
    packed_params: Vec<f32>,
    packed_seeds: Vec<u32>,
}
impl MaterialRevision {
    pub fn new(
        generation: u64,
        revision: u64,
        document: &MaterialGraphDocument,
    ) -> Result<Self, Vec<MaterialDiagnostic>> {
        let lowered = lower_material(document)?;
        Ok(Self {
            stamp: MaterialStamp {
                generation,
                revision,
            },
            packed_params: lowered.params.iter().flatten().copied().collect(),
            packed_seeds: lowered.seeds.iter().flatten().copied().collect(),
            lowered,
        })
    }
    pub fn stamp(&self) -> MaterialStamp {
        self.stamp
    }
    pub fn generation(&self) -> u64 {
        self.stamp.generation
    }
    pub fn revision(&self) -> u64 {
        self.stamp.revision
    }
    pub fn binding(&self) -> &AssetMaterialBinding {
        &self.lowered.binding
    }
    pub fn lowered(&self) -> &LoweredMaterial {
        &self.lowered
    }
    pub fn content_key(&self) -> &str {
        &self.lowered.content_key
    }
    pub(crate) fn packed_params(&self) -> &[f32] {
        &self.packed_params
    }
    pub(crate) fn packed_seeds(&self) -> &[u32] {
        &self.packed_seeds
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaterialProgramPhase {
    #[default]
    Disabled,
    Pending,
    Applied,
    Rejected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialProgramErrorPhase {
    Binding,
    Vertex,
    Fragment,
    Link,
    Uniforms,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialProgramDiagnostic {
    pub phase: MaterialProgramErrorPhase,
    pub node_ids: Vec<u32>,
    pub message: String,
}
impl MaterialProgramDiagnostic {
    pub(crate) fn remapped(&self, graph: &LoweredMaterial) -> Self {
        let mut result = self.clone();
        result.node_ids.clear();
        if result.phase == MaterialProgramErrorPhase::Fragment {
            for line in result.message.lines() {
                for number in line
                    .split(|c: char| !c.is_ascii_digit())
                    .filter_map(|v| v.parse::<usize>().ok())
                {
                    let Some(local_line) =
                        number.checked_sub(MATERIAL_GRAPH_FIRST_LINE).map(|n| n + 1)
                    else {
                        continue;
                    };
                    for span in &graph.source_map {
                        if (span.start_line..=span.end_line).contains(&local_line) {
                            result.node_ids.push(span.node_id);
                        }
                    }
                }
            }
            result.node_ids.sort_unstable();
            result.node_ids.dedup();
        }
        result
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaterialProgramStatus {
    pub requested: Option<MaterialStamp>,
    pub applied: Option<MaterialStamp>,
    pub phase: MaterialProgramPhase,
    pub diagnostics: Vec<MaterialProgramDiagnostic>,
    /// Actual candidate compilation attempts, not graph edits or draws.
    pub compilation_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))] // Browser has no authoring host yet.
pub enum MaterialQueueError {
    StaleRevision,
    ConflictingRevision,
}

/// CPU state only. Every GL operation belongs in the context's paint callback.
#[derive(Default)]
pub struct MaterialProgramState {
    requested: Option<Arc<MaterialRevision>>,
    applied: Option<Arc<MaterialRevision>>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    watermark: Option<(MaterialStamp, String)>,
    pending: bool,
    failed_topology: Option<(String, MaterialProgramDiagnostic)>,
    status: MaterialProgramStatus,
}

/// A complete context-local candidate or a change that needs no compilation.
/// The renderer must commit the current request before installing its candidate.
#[must_use = "commit this update or dispose its context-local candidate"]
pub enum PreparedMaterialUpdate<T> {
    Disabled,
    Parameters(Arc<MaterialRevision>),
    Program {
        revision: Arc<MaterialRevision>,
        candidate: T,
    },
}
impl<T> PreparedMaterialUpdate<T> {
    pub fn revision(&self) -> Option<&Arc<MaterialRevision>> {
        match self {
            Self::Disabled => None,
            Self::Parameters(revision) | Self::Program { revision, .. } => Some(revision),
        }
    }
}

fn same_request(a: Option<&MaterialRevision>, b: Option<&MaterialRevision>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.stamp == b.stamp && a.content_key() == b.content_key(),
        _ => false,
    }
}
impl MaterialProgramState {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn status(&self) -> &MaterialProgramStatus {
        &self.status
    }
    pub fn applied(&self) -> Option<&Arc<MaterialRevision>> {
        self.applied.as_ref()
    }
    /// Coalesces work to the latest request; this method never compiles or calls GL.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn queue(
        &mut self,
        revision: Option<Arc<MaterialRevision>>,
    ) -> Result<bool, MaterialQueueError> {
        if let Some(next) = &revision {
            if let Some((stamp, content_key)) = &self.watermark {
                if next.stamp < *stamp {
                    return Err(MaterialQueueError::StaleRevision);
                }
                if next.stamp == *stamp && next.content_key() != content_key {
                    return Err(MaterialQueueError::ConflictingRevision);
                }
            }
        }
        if same_request(self.requested.as_deref(), revision.as_deref()) {
            return Ok(false);
        }
        if let Some(next) = &revision {
            self.watermark = Some((next.stamp, next.content_key().into()));
        }
        self.status.requested = revision.as_ref().map(|r| r.stamp);
        self.status.phase = MaterialProgramPhase::Pending;
        self.status.diagnostics.clear();
        self.requested = revision;
        self.pending = true;
        Ok(true)
    }
    /// Run from a paint callback. The closure produces an entire program/cache pair.
    /// Tests can substitute a fake compiler without pretending to establish GPU results.
    #[cfg(test)]
    #[allow(dead_code)] // Used by the direct-module lifecycle integration harness.
    pub fn prepare_with<T>(
        &mut self,
        compile: impl FnOnce(&MaterialRevision) -> Result<T, MaterialProgramDiagnostic>,
    ) -> Option<PreparedMaterialUpdate<T>> {
        self.prepare_validated(|_| Ok(()), compile)
    }
    /// Backend binding validation runs even when only parameters or assignments
    /// changed. A supported shader topology never implies a supported asset slot.
    pub fn prepare_validated<T>(
        &mut self,
        validate: impl FnOnce(&MaterialRevision) -> Result<(), MaterialProgramDiagnostic>,
        compile: impl FnOnce(&MaterialRevision) -> Result<T, MaterialProgramDiagnostic>,
    ) -> Option<PreparedMaterialUpdate<T>> {
        if !std::mem::take(&mut self.pending) {
            return None;
        }
        let Some(revision) = self.requested.clone() else {
            return Some(PreparedMaterialUpdate::Disabled);
        };
        if let Err(diagnostic) = validate(&revision) {
            self.status.phase = MaterialProgramPhase::Rejected;
            self.status.diagnostics = vec![diagnostic];
            return None;
        }
        if self
            .applied
            .as_ref()
            .is_some_and(|old| old.lowered.topology_key == revision.lowered.topology_key)
        {
            return Some(PreparedMaterialUpdate::Parameters(revision));
        }
        if let Some((key, diagnostic)) = &self.failed_topology {
            if *key == revision.lowered.topology_key {
                self.status.phase = MaterialProgramPhase::Rejected;
                self.status.diagnostics = vec![diagnostic.remapped(revision.lowered())];
                return None;
            }
        }
        self.status.compilation_count = self.status.compilation_count.saturating_add(1);
        match compile(&revision) {
            Ok(candidate) => Some(PreparedMaterialUpdate::Program {
                revision,
                candidate,
            }),
            Err(diagnostic) => {
                self.failed_topology =
                    Some((revision.lowered.topology_key.clone(), diagnostic.clone()));
                self.status.phase = MaterialProgramPhase::Rejected;
                self.status.diagnostics = vec![diagnostic];
                None
            }
        }
    }
    /// Rejects a candidate superseded since preparation. On success the caller must
    /// synchronously install the complete candidate before drawing with this state.
    pub fn commit<T>(
        &mut self,
        update: PreparedMaterialUpdate<T>,
    ) -> Result<PreparedMaterialUpdate<T>, PreparedMaterialUpdate<T>> {
        if !same_request(
            self.requested.as_deref(),
            update.revision().map(Arc::as_ref),
        ) {
            return Err(update);
        }
        self.applied = self.requested.clone();
        self.status.applied = self.applied.as_ref().map(|r| r.stamp);
        self.status.phase = if self.applied.is_some() {
            MaterialProgramPhase::Applied
        } else {
            MaterialProgramPhase::Disabled
        };
        self.status.diagnostics.clear();
        Ok(update)
    }
    /// Explicit retry for a recovered driver/context, never an automatic per-frame loop.
    #[cfg(test)]
    #[allow(dead_code)] // Used by the direct-module lifecycle integration harness.
    pub fn retry_failed(&mut self) -> bool {
        if self.status.phase != MaterialProgramPhase::Rejected {
            return false;
        }
        self.failed_topology = None;
        self.pending = true;
        self.status.phase = MaterialProgramPhase::Pending;
        self.status.diagnostics.clear();
        true
    }
}
