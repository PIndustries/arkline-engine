//! New modules are included directly until the renderer integration owns their exports.
#[allow(dead_code)]
#[path = "../src/material_program.rs"]
mod material_program;
#[allow(dead_code)]
#[path = "../src/spatial_material.rs"]
mod spatial_material;

use arkline_protocol::material_graph::{
    MaterialGraphDocument, MaterialNodeOp, MaterialParameterValue,
};
use spatial_material::*;
use std::sync::Arc;

fn revision(
    generation: u64,
    number: u64,
    document: &MaterialGraphDocument,
) -> Arc<MaterialRevision> {
    Arc::new(MaterialRevision::new(generation, number, document).unwrap())
}
fn changed_parameters(document: &MaterialGraphDocument) -> MaterialGraphDocument {
    let mut changed = document.clone();
    changed
        .parameters
        .iter_mut()
        .find(|p| p.id == 6)
        .unwrap()
        .value = MaterialParameterValue::LinearColor([0.8, 0.02, 0.03]);
    changed
}
fn changed_topology(document: &MaterialGraphDocument) -> MaterialGraphDocument {
    let mut changed = document.clone();
    changed
        .nodes
        .iter_mut()
        .find(|n| matches!(n.op, MaterialNodeOp::SurfaceOutput { .. }))
        .unwrap()
        .op = MaterialNodeOp::SurfaceOutput { surface: 17 };
    changed
}
fn failure() -> MaterialProgramDiagnostic {
    MaterialProgramDiagnostic {
        phase: MaterialProgramErrorPhase::Fragment,
        node_ids: vec![6],
        message: "test compiler failure".into(),
    }
}

#[test]
fn coalesces_edits_and_keeps_parameter_updates_out_of_the_compiler() {
    let base = MaterialGraphDocument::pioneer_default();
    let changed = changed_parameters(&base);
    let first = revision(1, 1, &base);
    let latest = revision(1, 2, &changed);
    assert_eq!(first.lowered().topology_key, latest.lowered().topology_key);
    let mut state = MaterialProgramState::default();
    state.queue(Some(first.clone())).unwrap();
    state.queue(Some(latest.clone())).unwrap();
    let prepared = state
        .prepare_with(|r| {
            assert_eq!(r.stamp(), latest.stamp());
            Ok(7)
        })
        .unwrap();
    assert!(state.applied().is_none());
    assert!(matches!(
        state.commit(prepared),
        Ok(PreparedMaterialUpdate::Program { candidate: 7, .. })
    ));
    assert_eq!(state.status().compilation_count, 1);
    assert_eq!(state.applied().unwrap().stamp(), latest.stamp());
    let third = revision(1, 3, &base);
    state.queue(Some(third.clone())).unwrap();
    let prepared = state
        .prepare_with::<()>(|_| panic!("parameters must not compile"))
        .unwrap();
    assert!(matches!(
        state.commit(prepared),
        Ok(PreparedMaterialUpdate::Parameters(_))
    ));
    assert_eq!(state.applied().unwrap().stamp(), third.stamp());
    assert_eq!(state.status().compilation_count, 1);
    assert!(!state.queue(Some(third)).unwrap());
    assert!(state
        .prepare_with::<()>(|_| panic!("unchanged must not compile"))
        .is_none());
}

#[test]
fn stale_or_conflicting_revisions_cannot_replace_the_latest_request() {
    let base = MaterialGraphDocument::pioneer_default();
    let mut state = MaterialProgramState::default();
    let first = revision(4, 1, &base);
    state.queue(Some(first.clone())).unwrap();
    let prepared = state.prepare_with(|_| Ok(10)).unwrap();
    let latest = revision(4, 2, &base);
    state.queue(Some(latest.clone())).unwrap();
    assert!(matches!(
        state.commit(prepared),
        Err(PreparedMaterialUpdate::Program { candidate: 10, .. })
    ));
    assert!(state.applied().is_none());
    assert_eq!(
        state.queue(Some(first)),
        Err(MaterialQueueError::StaleRevision)
    );
    let conflict = revision(4, 2, &changed_parameters(&base));
    assert_eq!(
        state.queue(Some(conflict)),
        Err(MaterialQueueError::ConflictingRevision)
    );
    assert_eq!(state.status().requested, Some(latest.stamp()));
    let newer_project = revision(5, 0, &base);
    state.queue(Some(newer_project.clone())).unwrap();
    assert_eq!(
        state.queue(Some(latest)),
        Err(MaterialQueueError::StaleRevision)
    );
    assert_eq!(state.status().requested, Some(newer_project.stamp()));
}

#[test]
fn failed_topology_preserves_last_good_and_retries_only_explicitly() {
    let base = MaterialGraphDocument::pioneer_default();
    let mut state = MaterialProgramState::default();
    let good = revision(1, 1, &base);
    state.queue(Some(good.clone())).unwrap();
    let prepared = state.prepare_with(|_| Ok(())).unwrap();
    assert!(state.commit(prepared).is_ok());
    let bad_document = changed_topology(&base);
    state.queue(Some(revision(1, 2, &bad_document))).unwrap();
    assert!(state.prepare_with::<()>(|_| Err(failure())).is_none());
    assert_eq!(state.applied().unwrap().stamp(), good.stamp());
    assert_eq!(state.status().phase, MaterialProgramPhase::Rejected);
    state
        .queue(Some(revision(1, 3, &changed_parameters(&bad_document))))
        .unwrap();
    assert!(state
        .prepare_with::<()>(|_| panic!("same failing topology must not compile"))
        .is_none());
    assert_eq!(state.status().compilation_count, 2);
    assert!(state.retry_failed());
    let prepared = state.prepare_with(|_| Ok(())).unwrap();
    assert!(state.commit(prepared).is_ok());
    assert_eq!(state.status().compilation_count, 3);
    assert_eq!(state.status().phase, MaterialProgramPhase::Applied);
}

#[test]
fn disable_is_deferred_until_commit_and_keeps_stale_watermark() {
    let base = MaterialGraphDocument::pioneer_default();
    let mut state = MaterialProgramState::default();
    state.queue(Some(revision(1, 5, &base))).unwrap();
    let prepared = state.prepare_with(|_| Ok(())).unwrap();
    assert!(state.commit(prepared).is_ok());
    state.queue(None).unwrap();
    assert_eq!(state.status().phase, MaterialProgramPhase::Pending);
    assert!(state.applied().is_some());
    let prepared = state
        .prepare_with::<()>(|_| panic!("disable must not compile"))
        .unwrap();
    assert!(matches!(
        state.commit(prepared),
        Ok(PreparedMaterialUpdate::Disabled)
    ));
    assert!(state.applied().is_none());
    assert_eq!(state.status().phase, MaterialProgramPhase::Disabled);
    assert_eq!(
        state.queue(Some(revision(1, 4, &base))),
        Err(MaterialQueueError::StaleRevision)
    );
    assert_eq!(
        state.queue(Some(revision(1, 5, &changed_parameters(&base)))),
        Err(MaterialQueueError::ConflictingRevision)
    );
}

#[test]
fn layout_edits_preserve_lowered_content_and_parameter_pack() {
    let original = MaterialGraphDocument::pioneer_default();
    let mut moved = original.clone();
    moved.layout.insert(6, [700., 300.]);
    moved.nodes.reverse();
    let a = revision(1, 1, &original);
    let b = revision(1, 2, &moved);
    assert_eq!(a.content_key(), b.content_key());
    assert_eq!(a.lowered().fragment_function, b.lowered().fragment_function);
    assert_eq!(a.packed_params(), b.packed_params());
    assert_eq!(a.packed_seeds(), b.packed_seeds());
}

#[test]
fn cached_driver_errors_follow_current_node_ids_without_recompiling() {
    let original = MaterialGraphDocument::pioneer_default();
    let first = revision(1, 1, &original);
    let line = first
        .lowered()
        .source_map
        .iter()
        .find(|s| s.node_id == 6)
        .unwrap()
        .start_line
        + MATERIAL_GRAPH_FIRST_LINE
        - 1;
    let mut state = MaterialProgramState::default();
    state.queue(Some(first.clone())).unwrap();
    assert!(state
        .prepare_with::<()>(|_| Err(MaterialProgramDiagnostic {
            phase: MaterialProgramErrorPhase::Fragment,
            message: format!("ERROR: 0:{line}: synthetic failure"),
            node_ids: vec![6],
        }))
        .is_none());
    let mut renamed = original;
    for node in &mut renamed.nodes {
        for (index, (_, _, input)) in node.op.inputs().into_iter().enumerate() {
            node.op.set_input(index, input + 100).unwrap();
        }
        node.id += 100;
    }
    renamed.layout = renamed
        .layout
        .into_iter()
        .map(|(id, p)| (id + 100, p))
        .collect();
    let next = revision(1, 2, &renamed);
    assert_eq!(first.lowered().topology_key, next.lowered().topology_key);
    state.queue(Some(next)).unwrap();
    assert!(state
        .prepare_with::<()>(|_| panic!("cached topology must not recompile"))
        .is_none());
    assert!(state.status().diagnostics[0].node_ids.contains(&106));
    assert!(!state.status().diagnostics[0].node_ids.contains(&6));
    assert_eq!(state.status().compilation_count, 1);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "Explicit private CGL generated-material execution; no app window or world"]
fn native_spatial_material_executes_and_rejects_bad_candidates() {
    use eframe::glow::{self, HasContext};
    use material_program::{
        compile_material_program, MaterialShaderSource, SpatialMaterialUniforms,
    };
    use std::ffi::{c_void, CString};
    #[link(name = "OpenGL", kind = "framework")]
    unsafe extern "C" {
        fn CGLChoosePixelFormat(
            attributes: *const u32,
            format: *mut *mut c_void,
            count: *mut i32,
        ) -> i32;
        fn CGLCreateContext(
            format: *mut c_void,
            share: *mut c_void,
            context: *mut *mut c_void,
        ) -> i32;
        fn CGLSetCurrentContext(context: *mut c_void) -> i32;
        fn CGLDestroyContext(context: *mut c_void) -> i32;
        fn CGLDestroyPixelFormat(format: *mut c_void) -> i32;
    }
    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
    }
    const VERTEX: &str = "#version 150\nout vec2 uv;void main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);uv=p;gl_Position=vec4(p*2.0-1.0,0.0,1.0);}";
    const PREFIX: &str = concat!(
        "#version 150\n",
        include_str!("../src/shaders/sky.glsl"),
        include_str!("../src/shaders/materials.glsl")
    );
    const SUFFIX: &str = "in vec2 uv;uniform int ark_graph_enabled;uniform int station;out vec4 color;void main(){vec3 p=vec3((uv.x-.5)*72.0,0.0,(uv.y-.5)*38.0);ArkSurface authored=ArkSurface(vec3(.1),.5,0.,vec3(0.));ArkSurface s=authored;if(ark_graph_enabled!=0){s=ark_graph_surface(p,authored);}vec3 n=normalize(vec3(.2,.8,.4));vec3 v=normalize(vec3(.1,.5,1.));vec3 lit;if(station!=0){lit=ark_station_environment(n,v,s.base_color,s.roughness,s.metallic);}else{lit=ark_brdf(n,v,normalize(vec3(-.5,.8,.65)),s.base_color,s.roughness,s.metallic)*3.5+ark_environment(n,v,s.base_color,s.roughness,s.metallic,0.,vec3(0,1,0));}color=vec4(ark_display(lit+s.emission),1.);}";
    unsafe {
        let attributes = [99u32, 0x3200, 73, 8, 24, 12, 24, 0]; // accelerated core 3.2
        let mut format = std::ptr::null_mut();
        let mut count = 0;
        assert_eq!(
            CGLChoosePixelFormat(attributes.as_ptr(), &mut format, &mut count),
            0
        );
        let mut context = std::ptr::null_mut();
        assert_eq!(
            CGLCreateContext(format, std::ptr::null_mut(), &mut context),
            0
        );
        CGLDestroyPixelFormat(format);
        assert_eq!(CGLSetCurrentContext(context), 0);
        let gl = glow::Context::from_loader_function(|name| {
            dlsym(
                (-2_isize) as *mut c_void,
                CString::new(name).unwrap().as_ptr(),
            )
            .cast()
        });
        eprintln!(
            "material GL: {} / {} / {}",
            gl.get_parameter_string(glow::VENDOR),
            gl.get_parameter_string(glow::RENDERER),
            gl.get_parameter_string(glow::VERSION)
        );
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            96,
            96,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        let fbo = gl.create_framebuffer().unwrap();
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        assert_eq!(
            gl.check_framebuffer_status(glow::FRAMEBUFFER),
            glow::FRAMEBUFFER_COMPLETE
        );
        let vao = gl.create_vertex_array().unwrap();
        gl.bind_vertex_array(Some(vao));
        gl.viewport(0, 0, 96, 96);
        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::BLEND);
        gl.disable(glow::DEPTH_TEST);
        let base = MaterialGraphDocument::pioneer_default();
        let first = revision(1, 1, &base);
        let source = MaterialShaderSource::new(PREFIX, &first, SUFFIX);
        let candidate = compile_material_program(&gl, VERTEX, &source, &first, |gl, program| {
            Ok(SpatialMaterialUniforms::new(gl, program))
        })
        .unwrap();
        let mut program = candidate.program();
        let mut cache = None;
        // Install against a distinct disposable placeholder to exercise old-resource deletion.
        let old = gl.create_program().unwrap();
        let mut placeholder = old;
        candidate.install(&gl, &mut placeholder, |uniforms| cache = Some(uniforms));
        assert!(!gl.is_program(old));
        assert_eq!(placeholder, program);
        let uniforms = cache.unwrap();
        gl.use_program(Some(program));
        uniforms.set_enabled(&gl, true);
        let station = gl.get_uniform_location(program, "station");
        let render = |revision: &MaterialRevision, light: i32| {
            uniforms.upload(&gl, revision);
            gl.uniform_1_i32(station.as_ref(), light);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            let mut pixels = vec![0; 96 * 96 * 4];
            gl.read_pixels(
                0,
                0,
                96,
                96,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            pixels
        };
        let hangar = render(&first, 1);
        let space = render(&first, 0);
        assert_ne!(
            hangar, space,
            "real existing lighting functions must differ"
        );
        let min = hangar.chunks_exact(4).map(|p| p[1]).min().unwrap();
        let max = hangar.chunks_exact(4).map(|p| p[1]).max().unwrap();
        assert!(
            max - min > 20,
            "body-space graph must produce spatially different surface pixels"
        );
        let second = revision(1, 2, &changed_parameters(&base));
        assert_ne!(
            hangar,
            render(&second, 1),
            "uniform parameter update must change pixels without compilation"
        );
        for (parameter_id, value, name) in [
            (1, MaterialParameterValue::Vector3([0.45; 3]), "scale"),
            (3, MaterialParameterValue::Seed(92), "seed"),
            (7, MaterialParameterValue::Scalar(0.9), "roughness"),
            (8, MaterialParameterValue::Scalar(1.0), "metallic"),
        ] {
            let mut document = base.clone();
            document
                .parameters
                .iter_mut()
                .find(|p| p.id == parameter_id)
                .unwrap()
                .value = value;
            let edited = revision(1, 3, &document);
            assert_eq!(first.lowered().topology_key, edited.lowered().topology_key);
            let pixels = render(&edited, 1);
            let changed = hangar
                .chunks_exact(4)
                .zip(pixels.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
            assert!(changed > 0, "{name} alone must change GPU pixels");
            eprintln!(
                "spatial material {name}: {changed}/9216 pixels changed; same shader topology"
            );
        }
        assert_eq!(
            hangar,
            render(&first, 1),
            "same revision must reproduce exact pixels on this context"
        );
        let invalid =
            MaterialShaderSource::new("#version 150\nthis deliberately fails", &first, SUFFIX);
        assert!(
            compile_material_program(&gl, VERTEX, &invalid, &first, |gl, p| Ok(
                SpatialMaterialUniforms::new(gl, p)
            ))
            .is_err()
        );
        assert!(
            gl.is_program(program),
            "failed candidate must retain installed program"
        );
        assert_eq!(hangar, render(&first, 1));
        let mut rejected_uniform_program = None;
        let rejected = compile_material_program(&gl, VERTEX, &source, &first, |_, candidate| {
            rejected_uniform_program = Some(candidate);
            Err::<(), _>("test uniform cache failure".into())
        });
        assert!(matches!(
            rejected,
            Err(MaterialProgramDiagnostic {
                phase: MaterialProgramErrorPhase::Uniforms,
                ..
            })
        ));
        assert!(
            !gl.is_program(rejected_uniform_program.unwrap()),
            "a uniform-cache failure must dispose the entire candidate"
        );
        assert!(gl.is_program(program));
        let no_gate = MaterialShaderSource::new(
            PREFIX,
            &first,
            "in vec2 uv;out vec4 color;void main(){ArkSurface s=ark_graph_surface(vec3(uv,0),ArkSurface(vec3(.1),.5,0.,vec3(0.)));color=vec4(s.base_color,1);}",
        );
        assert!(matches!(
            compile_material_program(&gl, VERTEX, &no_gate, &first, |_, _| Ok(())),
            Err(MaterialProgramDiagnostic {
                phase: MaterialProgramErrorPhase::Uniforms,
                ..
            })
        ));
        let another_topology = revision(1, 4, &changed_topology(&base));
        assert!(matches!(
            compile_material_program(&gl, VERTEX, &source, &another_topology, |_, _| Ok(())),
            Err(MaterialProgramDiagnostic {
                phase: MaterialProgramErrorPhase::Fragment,
                ..
            })
        ));
        let replacement = compile_material_program(&gl, VERTEX, &source, &first, |gl, p| {
            Ok(SpatialMaterialUniforms::new(gl, p))
        })
        .unwrap();
        let previous = program;
        replacement.install(&gl, &mut program, |_| {});
        gl.use_program(Some(program));
        assert!(
            !gl.is_program(previous),
            "obsolete installed program must be deleted after swap"
        );
        assert_eq!(gl.get_error(), glow::NO_ERROR);
        gl.use_program(None);
        gl.delete_program(program);
        gl.delete_vertex_array(vao);
        gl.delete_framebuffer(fbo);
        gl.delete_texture(texture);
        CGLSetCurrentContext(std::ptr::null_mut());
        CGLDestroyContext(context);
    }
}
