
use super::*;
use crate::flight3d::cockpit_maps::CockpitSurface;
use crate::flight3d::cockpit_occlusion::{CockpitOcclusion, DepthDraw, DepthMeshCatalog};
use crate::flight3d::cockpit_displays::CockpitDisplayRenderer;
use sha2::{Digest, Sha256};
use std::{ffi::{c_void, CString}, path::PathBuf};

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(a: *const u32, f: *mut *mut c_void, n: *mut i32) -> i32;
    fn CGLCreateContext(f: *mut c_void, s: *mut c_void, c: *mut *mut c_void) -> i32;
    fn CGLGetCurrentContext() -> *mut c_void;
    fn CGLSetCurrentContext(c: *mut c_void) -> i32;
    fn CGLDestroyContext(c: *mut c_void) -> i32;
    fn CGLDestroyPixelFormat(f: *mut c_void) -> i32;
    fn dlsym(handle: *mut c_void, name: *const std::ffi::c_char) -> *mut c_void;
}

struct PrivateContext {
    current: *mut c_void,
    previous: *mut c_void,
}
impl PrivateContext {
    unsafe fn create() -> (Self, Arc<glow::Context>) {
        let previous = CGLGetCurrentContext();
        let mut format = std::ptr::null_mut();
        let mut count = 0;
        assert_eq!(
            CGLChoosePixelFormat(
                [99, 0x3200, 73, 8, 24, 12, 24, 0].as_ptr(),
                &mut format,
                &mut count
            ),
            0
        );
        assert!(!format.is_null() && count > 0);
        let mut current = std::ptr::null_mut();
        let result = CGLCreateContext(format, std::ptr::null_mut(), &mut current);
        assert_eq!(CGLDestroyPixelFormat(format), 0);
        assert_eq!(result, 0);
        assert!(!current.is_null());
        let guard = Self { current, previous };
        assert_eq!(CGLSetCurrentContext(current), 0);
        let gl = Arc::new(glow::Context::from_loader_function(|name| {
            dlsym(
                (-2_isize) as *mut c_void,
                CString::new(name).unwrap().as_ptr(),
            )
            .cast()
        }));
        (guard, gl)
    }
}
impl Drop for PrivateContext {
    fn drop(&mut self) {
        unsafe {
            let _ = CGLSetCurrentContext(self.previous);
            let _ = CGLDestroyContext(self.current);
        }
    }
}



const WIDTH: usize = 1500;
const HEIGHT: usize = 900;
const LABEL_SIZE: f32 = 28.;
const VALUE_SIZE: f32 = 50.;
const LABEL_POS: [[f32;2];3] = [[20.,8.],[20.,120.],[245.,120.]];
const VALUE_POS: [[f32;2];3] = [[20.,40.],[20.,154.],[245.,154.]];
fn projected_fonts(surface: &CockpitSurface) -> (f32,f32) {
    let mut labels=f32::INFINITY;
    let mut values=f32::INFINITY;
    for (points,size,result) in [
        (&LABEL_POS,LABEL_SIZE,&mut labels),
        (&VALUE_POS,VALUE_SIZE,&mut values),
    ] {
        for [x,y] in points {
            if let (Some(a),Some(b))=(surface.project(egui::pos2(*x,*y)),surface.project(egui::pos2(*x,*y+size))) {
                *result=(*result).min(a.distance(b));
            } else { *result=0.; }
        }
    }
    (labels,values)
}
#[test]
fn proposed_forward_bay_font_budget() {
    // A projection preflight only: this does not claim an authored mesh exists.
    let viewport=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(WIDTH as f32,HEIGHT as f32));
    for (name,eye,look) in [
        ("seated",Vec3::new(0.,1.35,0.),Vec3::new(1.8,1.10,0.)),
        ("standing",Vec3::new(-0.85,1.65,0.65),Vec3::new(1.,1.0,0.2)),
    ] {
        let vp=Mat4::perspective_rh_gl(60f32.to_radians(),WIDTH as f32/HEIGHT as f32,0.02,100.)
            *Mat4::look_at_rh(eye,look,Vec3::Y);
        let surface=CockpitSurface::registered(viewport,vp,Mat4::IDENTITY,
            Vec3::new(0.932,1.,0.),Vec3::new(0.02,0.183,0.308),3,egui::vec2(400.,240.)).unwrap();
        let (labels,values)=projected_fonts(&surface);
        eprintln!("PROPOSED PLANE ONLY {name}: label_em={labels:.2}px value_em={values:.2}px");
        assert!(labels>=14. && values>=24.);
    }
}
fn vec3(v: &serde_json::Value) -> Vec3 {
    Vec3::new(v[0].as_f64().unwrap() as f32,v[1].as_f64().unwrap() as f32,v[2].as_f64().unwrap() as f32)
}

/// Explicit asset preview / isolated triangle-picking fixture. No EditorWorld,
/// production registration, desktop window, command transport or live authority.
#[test]
#[ignore = "Explicit original forward-bay asset proof; retained isolated native captures"]
fn native_authored_forward_bay_proof() {
    unsafe {
        let asset=PathBuf::from(std::env::var_os("ARKLINE_FORWARD_BAY_GLB").expect("Explicit proof GLB"));
        let manifest_path=PathBuf::from(std::env::var_os("ARKLINE_FORWARD_BAY_MANIFEST").expect("Bound plane/component manifest"));
        let telemetry_path=PathBuf::from(std::env::var_os("ARKLINE_FORWARD_BAY_TELEMETRY").expect("Recorded confirmed telemetry, never fabricated"));
        let out=PathBuf::from(std::env::var_os("ARKLINE_FORWARD_BAY_EVIDENCE").expect("New retained evidence directory"));
        std::fs::create_dir(&out).unwrap();
        let bytes=std::fs::read(&asset).unwrap();
        let manifest_bytes=std::fs::read(&manifest_path).unwrap();
        let telemetry_bytes=std::fs::read(&telemetry_path).unwrap();
        let manifest:serde_json::Value=serde_json::from_slice(&manifest_bytes).unwrap();
        let telemetry:serde_json::Value=serde_json::from_slice(&telemetry_bytes).unwrap();
        let geometry=ImportedGeometry::decode(&bytes).unwrap();
        assert!(!geometry.uses_generated_normals(),"Strict authored normals required");
        let instrument=&manifest["instrument"];
        let center=vec3(&instrument["center"]);
        let half=vec3(&instrument["half"]);
        let axes=&instrument["axes"];
        let rotation=Mat4::from_cols(vec3(&axes[0]).extend(0.),vec3(&axes[1]).extend(0.),vec3(&axes[2]).extend(0.),glam::Vec4::W);
        let mount=Mat4::from_translation(center)*rotation*Mat4::from_translation(-center);
        assert!((instrument["plane_x"].as_f64().unwrap() as f32-(center.x-0.032)).abs()<1e-6);
        let canvas=egui::vec2(instrument["canvas"][0].as_f64().unwrap() as f32,instrument["canvas"][1].as_f64().unwrap() as f32);
        let exact_meshes=geometry.parts.iter().map(|part| Arc::from(part.vertices.iter()
            .map(|v| Vertex{p:v.p*100.,n:v.n}).collect::<Vec<_>>())).collect::<Vec<Arc<[Vertex]>>>();
        let catalog=Arc::new(DepthMeshCatalog::new(exact_meshes));
        let draws=(0..geometry.parts.len()).map(|i| DepthDraw::whole(&catalog,i,Mat4::IDENTITY).unwrap()).collect();
        let scene=Arc::new(CockpitOcclusion::new(arkline_protocol::ShipClass::Pioneer,catalog,draws,vec![]).unwrap());
        let (_context,gl)=PrivateContext::create();
        let mut preview=ImportedAssetPreview::new(Some(&gl));
        preview.set(&geometry).unwrap();
        let display=Arc::new(Mutex::new(CockpitDisplayRenderer::new(gl.clone()).unwrap()));
        let mut painter=egui_glow::Painter::new(gl.clone(),"",None,false).unwrap();
        let target=CachedFeed::create(&gl,83001,WIDTH as i32,HEIGHT as i32).unwrap();
        let ctx=egui::Context::default();
        let viewport=egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(WIDTH as f32,HEIGHT as f32));
        let charge=telemetry["charge_j"].as_u64().unwrap() as f64/1000.;
        let ore=telemetry["ore"].as_u64().unwrap();
        let mode=telemetry["location_kind"].as_str().unwrap().to_uppercase();
        let rows=[("STATE".to_owned(),mode),("CAPACITOR".to_owned(),format!("{charge:0.0} kJ")),("ORE [u]".to_owned(),ore.to_string())];
        let mut captures=Vec::new();
        for (name,eye,look) in [
            ("seated",Vec3::new(0.,1.35,0.),Vec3::new(1.8,1.10,0.)),
            ("standing",Vec3::new(-0.85,1.65,0.65),Vec3::new(1.,1.0,0.2)),
        ] {
            let vp=Mat4::perspective_rh_gl(60f32.to_radians(),WIDTH as f32/HEIGHT as f32,0.02,100.)
                *Mat4::look_at_rh(eye,look,Vec3::Y);
            let surface=CockpitSurface::registered(viewport,vp,mount,center,half,3,canvas).unwrap()
                .with_registered_occlusion(scene.clone(),vp,mount).with_renderer(display.clone());
            let mut corners=Vec::new();
            let mut clear=0;
            for point in [egui::pos2(0.,0.),egui::pos2(canvas.x,0.),egui::pos2(0.,canvas.y),egui::pos2(canvas.x,canvas.y),egui::pos2(canvas.x*0.5,canvas.y*0.5)] {
                let screen=surface.project(point);
                let hit=screen.and_then(|p|surface.unproject(p));
                let good=hit.is_some_and(|p|p.distance(point)<0.1);
                clear+=usize::from(good);
                corners.push(serde_json::json!({"canvas":[point.x,point.y],"screen":screen.map(|p|[p.x,p.y]),"clear":good}));
            }
            let (label_pixels,value_pixels)=projected_fonts(&surface);
            for frame in 0..3 {
                let output=ctx.run(egui::RawInput{screen_rect:Some(viewport),focused:false,time:Some((frame+1) as f64*0.016),..Default::default()},|ctx|{
                    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx,|ui|{
                        ui.painter().add(preview.shared_callback(viewport,vp,eye,Mat4::IDENTITY).unwrap());
                        surface.paint(ui,|p|{
                            p.rect_filled(surface.canvas_rect(),0.,egui::Color32::from_rgb(5,15,18));
                            for (i,(label,value)) in rows.iter().enumerate(){
                                p.text(egui::pos2(LABEL_POS[i][0],LABEL_POS[i][1]),egui::Align2::LEFT_TOP,label,egui::FontId::proportional(LABEL_SIZE),egui::Color32::from_rgb(160,193,190));
                                p.text(egui::pos2(VALUE_POS[i][0],VALUE_POS[i][1]),egui::Align2::LEFT_TOP,value,egui::FontId::monospace(VALUE_SIZE),egui::Color32::from_rgb(217,239,226));
                            }
                            p.text(egui::pos2(canvas.x-12.,canvas.y-8.),egui::Align2::RIGHT_BOTTOM,"RECORDED · NOT LIVE",egui::FontId::proportional(14.),egui::Color32::from_rgb(210,167,98));
                        });
                        ui.painter().text(egui::pos2(20.,20.),egui::Align2::LEFT_TOP,"ISOLATED AUTHORED-MESH PROOF · IMPORT PREVIEW LIGHTING",egui::FontId::proportional(18.),egui::Color32::WHITE);
                    });
                });
                let primitives=ctx.tessellate(output.shapes,output.pixels_per_point);
                gl.bind_framebuffer(glow::FRAMEBUFFER,Some(target.framebuffer));
                gl.viewport(0,0,WIDTH as i32,HEIGHT as i32);
                gl.disable(glow::SCISSOR_TEST);
                painter.paint_and_update_textures([WIDTH as u32,HEIGHT as u32],output.pixels_per_point,&primitives,&output.textures_delta);
                assert_eq!(gl.get_error(),glow::NO_ERROR);
            }
            let mut raw=vec![0u8;WIDTH*HEIGHT*4];
            gl.read_pixels(0,0,WIDTH as i32,HEIGHT as i32,glow::RGBA,glow::UNSIGNED_BYTE,glow::PixelPackData::Slice(Some(&mut raw)));
            let top_down=raw.chunks_exact(WIDTH*4).rev().flatten().copied().collect::<Vec<_>>();
            image::save_buffer_with_format(out.join(format!("{name}.png")),&top_down,WIDTH as u32,HEIGHT as u32,image::ColorType::Rgba8,image::ImageFormat::Png).unwrap();
            captures.push(serde_json::json!({"view":name,"eye_m":eye.to_array(),"look_target_m":look.to_array(),"width":WIDTH,"height":HEIGHT,"all_5_display_points_clear":clear==5,"display_points":corners,"minimum_label_projected_px":label_pixels,"minimum_value_projected_px":value_pixels,"seated_label_target_px":14,"seated_value_target_px":24,"readability_target_pass":label_pixels>=14. && value_pixels>=24.,"rgba_sha256":hex::encode(Sha256::digest(&top_down))}));
        }
        let receipt=serde_json::json!({"scope":"Isolated original GLB asset preview with actual engine imported-mesh and cockpit-display code; no production registration or command dispatch","geometry_sha256":hex::encode(Sha256::digest(&bytes)),"manifest_sha256":hex::encode(Sha256::digest(&manifest_bytes)),"recorded_telemetry_sha256":hex::encode(Sha256::digest(&telemetry_bytes)),"telemetry":telemetry,"import_report":geometry.report,"captures":captures,"limits":["Preview lighting is not live cockpit lighting","Recorded confirmed values, not live simulation or fabricated progress","Projected font em size is not human reading certification","Full canonical collision/server admission not established by importpreview"]});
        std::fs::write(out.join("receipt.json"),serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        painter.destroy(); preview.destroy(); display.lock().unwrap().destroy(); target.destroy(&gl);
    }
}
