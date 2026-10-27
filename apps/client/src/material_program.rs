//! Context-owned material shader compilation. No project files or world state.
use crate::spatial_material::{
    MaterialProgramDiagnostic, MaterialProgramErrorPhase, MaterialRevision,
    MATERIAL_GRAPH_FIRST_LINE,
};
use arkline_protocol::material_compile::LoweredMaterial;
use eframe::glow::{self, HasContext};

const GRAPH_FIRST_LINE: usize = MATERIAL_GRAPH_FIRST_LINE;
const SUFFIX_FIRST_LINE: usize = 100_000;
const MAX_DRIVER_LOG_BYTES: usize = 8192;

/// The renderer supplies its trusted lighting/main template around compiler output.
/// #line makes node diagnostics independent of injected shadow code and ES precision.
pub struct MaterialShaderSource {
    fragment: String,
    topology_key: String,
    declares_gate: bool,
}
impl MaterialShaderSource {
    pub fn new(prefix: &str, revision: &MaterialRevision, suffix: &str) -> Self {
        let graph = revision.lowered();
        Self {
            fragment: format!(
                "{prefix}\n#line {GRAPH_FIRST_LINE}\n{}\n#line {SUFFIX_FIRST_LINE}\n{suffix}",
                graph.fragment_function
            ),
            topology_key: graph.topology_key.clone(),
            declares_gate: prefix.contains("uniform int ark_graph_enabled;")
                || suffix.contains("uniform int ark_graph_enabled;"),
        }
    }
    pub fn fragment(&self) -> &str {
        &self.fragment
    }
}

/// Program and every program-specific uniform cache must be created together.
#[must_use = "install or destroy the candidate on its current GL context"]
pub struct CompiledMaterialProgram<U> {
    program: glow::Program,
    uniforms: U,
}
impl<U> CompiledMaterialProgram<U> {
    #[cfg(test)]
    #[allow(dead_code)] // Direct-module integration harness also exercises this helper.
    pub fn program(&self) -> glow::Program {
        self.program
    }
    /// Caller holds the renderer lock and its GL context. Cache replacement is
    /// infallible and must include local lighting as well as mesh uniforms.
    pub unsafe fn install(
        self,
        gl: &glow::Context,
        current_program: &mut glow::Program,
        replace_uniforms: impl FnOnce(U),
    ) {
        replace_uniforms(self.uniforms);
        let old = std::mem::replace(current_program, self.program);
        gl.delete_program(old);
    }
    pub unsafe fn destroy(self, gl: &glow::Context) {
        gl.delete_program(self.program);
    }
}

/// Must run on the same current context that will later draw the program.
/// Existing mesh buffers, shadow maps and background programs are unaffected.
pub unsafe fn compile_material_program<U>(
    gl: &glow::Context,
    vertex: &str,
    source: &MaterialShaderSource,
    revision: &MaterialRevision,
    uniforms: impl FnOnce(&glow::Context, glow::Program) -> Result<U, String>,
) -> Result<CompiledMaterialProgram<U>, MaterialProgramDiagnostic> {
    if !source.declares_gate {
        return Err(diagnostic(
            MaterialProgramErrorPhase::Uniforms,
            "Material shader template must declare uniform int ark_graph_enabled",
            revision.lowered(),
        ));
    }
    if source.topology_key != revision.lowered().topology_key {
        return Err(diagnostic(
            MaterialProgramErrorPhase::Fragment,
            "Material source belongs to a different shader topology",
            revision.lowered(),
        ));
    }
    let program = gl.create_program().map_err(|message| {
        diagnostic(
            MaterialProgramErrorPhase::Link,
            &message,
            revision.lowered(),
        )
    })?;
    for (kind, text, phase) in [
        (
            glow::VERTEX_SHADER,
            vertex,
            MaterialProgramErrorPhase::Vertex,
        ),
        (
            glow::FRAGMENT_SHADER,
            source.fragment(),
            MaterialProgramErrorPhase::Fragment,
        ),
    ] {
        let shader = match gl.create_shader(kind) {
            Ok(shader) => shader,
            Err(message) => {
                gl.delete_program(program);
                return Err(diagnostic(phase, &message, revision.lowered()));
            }
        };
        let text = if gl.version().is_embedded {
            text.replacen(
                "#version 150",
                "#version 300 es\nprecision highp float;\nprecision highp int;",
                1,
            )
        } else {
            text.to_owned()
        };
        gl.shader_source(shader, &text);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            let log = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            gl.delete_program(program);
            return Err(diagnostic(phase, &log, revision.lowered()));
        }
        gl.attach_shader(program, shader);
        // Attached shaders stay alive until link/program deletion.
        gl.delete_shader(shader);
    }
    gl.bind_attrib_location(program, 0, "position");
    gl.bind_attrib_location(program, 1, "normal");
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        let log = gl.get_program_info_log(program);
        gl.delete_program(program);
        return Err(diagnostic(
            MaterialProgramErrorPhase::Link,
            &log,
            revision.lowered(),
        ));
    }
    // Uniforms can legitimately be optimized out, including the gate when the
    // authored-surface graph is an exact pass-through. The trusted template
    // declaration is checked above; no project-provided shader text is accepted.
    match uniforms(gl, program) {
        Ok(uniforms) => Ok(CompiledMaterialProgram { program, uniforms }),
        Err(message) => {
            gl.delete_program(program);
            Err(diagnostic(
                MaterialProgramErrorPhase::Uniforms,
                &message,
                revision.lowered(),
            ))
        }
    }
}

pub struct SpatialMaterialUniforms {
    enabled: Option<glow::UniformLocation>,
    params: Option<glow::UniformLocation>,
    seeds: Option<glow::UniformLocation>,
}
impl SpatialMaterialUniforms {
    pub unsafe fn new(gl: &glow::Context, program: glow::Program) -> Self {
        Self {
            enabled: gl.get_uniform_location(program, "ark_graph_enabled"),
            params: gl.get_uniform_location(program, "ark_graph_params[0]"),
            seeds: gl.get_uniform_location(program, "ark_graph_seeds[0]"),
        }
    }
    /// Upload once per pass or installed parameter revision, after binding program.
    pub unsafe fn upload(&self, gl: &glow::Context, revision: &MaterialRevision) {
        if !revision.packed_params().is_empty() {
            gl.uniform_4_f32_slice(self.params.as_ref(), revision.packed_params());
        }
        if !revision.packed_seeds().is_empty() {
            gl.uniform_4_u32_slice(self.seeds.as_ref(), revision.packed_seeds());
        }
    }
    /// Set for every draw, including station/other-ship draws, to prevent leakage.
    pub unsafe fn set_enabled(&self, gl: &glow::Context, enabled: bool) {
        gl.uniform_1_i32(self.enabled.as_ref(), i32::from(enabled));
    }
}

fn diagnostic(
    phase: MaterialProgramErrorPhase,
    log: &str,
    graph: &LoweredMaterial,
) -> MaterialProgramDiagnostic {
    let mut message = log.to_owned();
    if message.len() > MAX_DRIVER_LOG_BYTES {
        let mut end = MAX_DRIVER_LOG_BYTES;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push_str(" [driver log truncated]");
    }
    MaterialProgramDiagnostic {
        phase,
        node_ids: Vec::new(),
        message,
    }
    .remapped(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::{
        material_compile::lower_material, material_graph::MaterialGraphDocument,
    };

    #[test]
    fn driver_lines_map_to_nodes_independent_of_template_prefix() {
        let graph = lower_material(&MaterialGraphDocument::pioneer_default()).unwrap();
        let span = graph.source_map.first().unwrap();
        let line = GRAPH_FIRST_LINE + span.start_line - 1;
        for log in [
            format!("ERROR: 0:{line}: syntax error"),
            format!("0({line}) : error"),
        ] {
            let result = diagnostic(MaterialProgramErrorPhase::Fragment, &log, &graph);
            assert!(result.node_ids.contains(&span.node_id));
            let vertex = diagnostic(MaterialProgramErrorPhase::Vertex, &log, &graph);
            assert!(vertex.node_ids.is_empty());
        }
    }

    #[test]
    fn driver_logs_are_bounded_at_utf8_boundaries() {
        let graph = lower_material(&MaterialGraphDocument::pioneer_default()).unwrap();
        let result = diagnostic(
            MaterialProgramErrorPhase::Link,
            &"界".repeat(10_000),
            &graph,
        );
        assert!(result.message.len() < MAX_DRIVER_LOG_BYTES + 64);
        assert!(result.message.ends_with("[driver log truncated]"));
    }
}
