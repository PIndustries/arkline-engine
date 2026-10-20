//! Source-backed runtime inspection. Selecting a model never creates world state.
use super::*;
use arkline_protocol::{DroneClass, ExteriorFixtureKind, SurfacePropKind, ALL_SHIP_CLASSES};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[path = "model_catalog_windows.rs"]
mod windows;

#[derive(Clone, Debug)]
pub struct RuntimeModelDescriptor {
    pub id: String,
    pub name: String,
    pub category: String,
    pub source: String,
    pub notes: Vec<String>,
    pub unavailable: Option<String>,
    /// Source specification in XYZ meters, when the source defines one.
    pub spec_dimensions_m: Option<[f64; 3]>,
}

#[derive(Clone, Debug)]
pub struct ModelGeometryInfo {
    pub min_m: [f32; 3],
    pub max_m: [f32; 3],
    pub triangles: usize,
    pub batches: usize,
}

pub struct RuntimeInspectionModel {
    // Bounds and transforms share the recipe's raw coordinate frame. Only the
    // public measured report converts flight units into meters.
    pub(super) parts: Vec<InspectionDraw>,
    pub(super) min: Vec3,
    pub(super) max: Vec3,
    pub info: ModelGeometryInfo,
}

#[derive(Clone, Copy)]
pub(super) struct InspectionDraw {
    pub part: ModelPart,
    pub material: Material,
    /// Expanded triangle-corner range in the existing GPU mesh: first, count.
    /// None retains the complete runtime draw.
    pub range: Option<(usize, usize)>,
}
impl InspectionDraw {
    pub(super) fn selected_vertices<'a>(
        &self,
        vertices: &'a [Vertex],
    ) -> Result<&'a [Vertex], String> {
        let count = i32::try_from(vertices.len())
            .map_err(|_| "Runtime inspection vertex count exceeds the GPU draw range")?;
        let (first, count) = triangle_draw_range(count, self.range).ok_or(
            "Runtime inspection range must be nonempty, triangle aligned and within its GPU mesh",
        )?;
        Ok(&vertices[first as usize..first as usize + count as usize])
    }
}

pub fn runtime_model_catalog() -> Vec<RuntimeModelDescriptor> {
    descriptors().clone()
}

fn descriptors() -> &'static Vec<RuntimeModelDescriptor> {
    static CATALOG: OnceLock<Vec<RuntimeModelDescriptor>> = OnceLock::new();
    CATALOG.get_or_init(build_descriptors)
}

pub fn load_runtime_model(id: &str) -> Result<Arc<RuntimeInspectionModel>, String> {
    let descriptor = descriptors()
        .iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| format!("Unknown runtime model: {id}"))?;
    if let Some(reason) = &descriptor.unavailable {
        return Err(reason.clone());
    }
    type Cached = BTreeMap<String, Result<Arc<RuntimeInspectionModel>, String>>;
    static MODELS: OnceLock<Mutex<Cached>> = OnceLock::new();
    let mut cache = MODELS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| "Runtime model cache is unavailable")?;
    cache
        .entry(id.to_owned())
        .or_insert_with(|| {
            let (parts, meters_per_unit) = recipe(id)?;
            measure(parts, meters_per_unit).map(Arc::new)
        })
        .clone()
}

pub(super) fn ship_slug(class: ShipClass) -> &'static str {
    match class {
        ShipClass::Pioneer => "pioneer",
        ShipClass::Prospector => "prospector",
        ShipClass::Hauler => "hauler",
        ShipClass::Vanguard => "vanguard",
        ShipClass::Borealis => "borealis",
        ShipClass::Stratum => "stratum",
        ShipClass::Monolith => "monolith",
    }
}

fn ship_class(slug: &str) -> Option<ShipClass> {
    ALL_SHIP_CLASSES
        .into_iter()
        .find(|class| ship_slug(*class) == slug)
}

fn descriptor(id: &str, name: &str, category: &str, source: &str) -> RuntimeModelDescriptor {
    RuntimeModelDescriptor {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        source: source.into(),
        notes: Vec::new(),
        unavailable: None,
        spec_dimensions_m: None,
    }
}

const PRIMITIVES: [(&str, &str); 10] = [
    ("cube", "Cube"),
    ("torus", "Gate torus"),
    ("sphere", "Low resolution sphere"),
    ("rock", "Asteroid rock mesh"),
    ("legacy_hull", "Legacy hull primitive"),
    ("planet_sphere", "Planet sphere mesh"),
    ("thin_torus", "Thin effect torus"),
    ("surface_patch", "Spherical ground patch mesh"),
    ("armor_block", "Chamfered armor block"),
    ("industrial_shell", "Faceted industrial shell"),
];

fn build_descriptors() -> Vec<RuntimeModelDescriptor> {
    let mut entries = Vec::new();
    for class in ALL_SHIP_CLASSES {
        let spec = class.spec();
        let slug = ship_slug(class);
        let source = if class == ShipClass::Pioneer {
            "assets/ships/pioneer/pioneer.glb"
        } else {
            "apps/client/src/flight3d.rs::build_ship_parts"
        };
        let mut hull = descriptor(&format!("ship/{slug}"), spec.name, "Ships", source);
        hull.spec_dimensions_m = Some([spec.length_m, spec.height_m, spec.width_m]);
        hull.notes.push("Hull geometry; articulated mining turrets are separate entries. Dimensions use XYZ order.".into());
        if class == ShipClass::Pioneer {
            hull.notes
                .push(format!("Embedded hull SHA-256 {}", pioneer_asset_sha256()));
        }
        entries.push(hull);
        let mut turret = descriptor(
            &format!("turret/{slug}"),
            &format!("{} mining turret", spec.name),
            "Equipment",
            "apps/client/src/flight3d.rs::turret_parts",
        );
        turret.notes.push("Actual class-scaled module 0 mount at neutral aim, including its fitted pedestal. Module 1 is the other runtime mount; no laser effect or simulation is started.".into());
        entries.push(turret);
        let mut cockpit = descriptor(
            &format!("cockpit/{slug}"),
            &format!("{} cockpit enclosure", spec.name),
            "Cockpits",
            "apps/client/src/flight3d.rs::cached_cockpit_parts",
        );
        cockpit.notes.push("Shared enclosure, console and screen-backing draw recipe at its actual class mount scale. HUD pixels and camera feeds are not part of the model.".into());
        if class == ShipClass::Pioneer {
            cockpit.notes.push("Pioneer canopy surfaces belong to the separately listed authored hull; this isolated console assembly does not claim a separately registered canopy model.".into());
        }
        entries.push(cockpit);
    }
    for (class, slug, name) in [
        (DroneClass::Recon, "recon", "Recon drone"),
        (DroneClass::Mining, "mining", "Mining drone"),
        (DroneClass::Attack, "attack", "Attack drone"),
    ] {
        let mut entry = descriptor(
            &format!("drone/{slug}"),
            name,
            "Drones",
            "apps/client/src/drone_models.rs",
        );
        let (min, max) = drone_model_bounds(class);
        entry.notes.push(format!(
            "Measured render envelope {:?} m XYZ; no new physical rating.",
            ((max - min) * 100.0).to_array()
        ));
        entries.push(entry);
    }
    let mut station = descriptor(
        "station/exterior",
        "Station exterior",
        "Stations",
        "apps/client/src/flight3d.rs::station_batches_geometry",
    );
    station.spec_dimensions_m = Some([STATION_DIAMETER_M, STATION_HEIGHT_M, STATION_DIAMETER_M]);
    station.notes.push("Exact uploaded static exterior batches. Animated approach lights are effects rather than separate hull geometry.".into());
    entries.push(station);
    for (kind, slug, name) in [
        (SurfacePropKind::Crate, "crate", "Construction crate"),
        (SurfacePropKind::Pylon, "pylon", "Construction pylon"),
        (
            SurfacePropKind::Platform,
            "platform",
            "Construction platform",
        ),
    ] {
        let mut entry = descriptor(
            &format!("prop/{slug}"),
            name,
            "World objects",
            "apps/client/src/world_visuals.rs::surface_prop_parts",
        );
        entry.spec_dimensions_m = Some(kind.size());
        entry.notes.push("Actual body and top marker. Source size describes the body; measured bounds also include the marker.".into());
        entries.push(entry);
    }
    for (id, name) in [
        ("world/asteroid_ore", "Ore asteroid"),
        ("world/asteroid_depleted", "Depleted asteroid"),
        ("world/gate", "Stargate paired rings"),
        ("world/planet", "Verdant planet geometry"),
        ("world/terrain", "Verdant ground patch geometry"),
    ] {
        let mut entry = descriptor(
            id,
            name,
            "World objects",
            "apps/client/src/flight3d.rs / world_visuals.rs",
        );
        match id {
            "world/asteroid_ore" | "world/asteroid_depleted" => entry.notes.push("Shared runtime rock at the first catalog asteroid's 80 m radius and time 0 pose. The 108 instances use this mesh at five catalog radii; these are not 108 distinct assets.".into()),
            "world/gate" => entry.notes.push("Actual two-ring design at the first gate's catalog radius; the four gate identities reuse this geometry.".into()),
            _ => entry.notes.push("Exact uploaded geometry with a plain inspection material. Live planet/terrain procedural shader, atmosphere, world projection and camera-relative clipping are not reproduced in isolated inspection.".into()),
        }
        entries.push(entry);
    }
    for (slug, name) in PRIMITIVES {
        let mut entry = descriptor(
            &format!("geometry/{slug}"),
            name,
            "Geometry",
            "apps/client/src/flight3d.rs::flight_geometry",
        );
        entry.notes.push("Exact reusable GPU mesh recipe before instance transforms. Measured size follows its source coordinates; this is not a physical model specification.".into());
        entries.push(entry);
    }
    if let Ok(primitives) = pioneer_asset() {
        for (index, primitive) in primitives.iter().enumerate() {
            let mut entry = descriptor(
                &format!("pioneer/material/{index}"),
                &format!("Pioneer · {}", primitive.material_name),
                "Hull material groups",
                "assets/ships/pioneer/pioneer.glb",
            );
            entry.notes.push(format!("Exact runtime primitive {index}, source material index {:?}. A material batch can contain disconnected source objects; it is not an editable source-part identity.", primitive.material_index));
            entries.push(entry);
        }
    }
    for fixture in arkline_protocol::default_pioneer_exterior().fixtures {
        let mut entry = descriptor(
            &format!("exterior/{}", fixture.id),
            &fixture.id,
            "Exterior fixtures",
            "crates/arkline-protocol/src/exterior_effects.rs::default_pioneer_exterior",
        );
        entry.notes.push(format!("{:?} fixture at {:?} body meters. Separate geometry uses the same runtime recipe at output 1; no powered state or pulse clock is changed.", fixture.kind, fixture.position_m));
        if fixture.kind == ExteriorFixtureKind::Window {
            entry.notes.clear();
            entry.notes.push("Authored window pane only, selected from the existing Navigation glass GPU batch. Exact GLB hash and checked-in source/mount mappings are validated before preview; no fixture emission or gameplay state is applied.".into());
            entry.notes.push("Source mapping is a checked-in review candidate for this exact asset snapshot, not an engineering approval or a durable identity across asset revisions.".into());
        }
        entries.push(entry);
    }
    for room in arkline_protocol::default_pioneer().rooms {
        let identity = room.source_room_id.as_deref().unwrap_or("local");
        let mut entry = descriptor(
            &format!("room/{identity}"),
            &format!("Deck {} · {}", room.deck, room.name),
            "Rooms",
            "apps/client/src/pioneer_interior.rs::inspection_room_parts",
        );
        entry.spec_dimensions_m = Some([
            room.clear_lwh_m[0],
            room.clear_lwh_m[2],
            room.clear_lwh_m[1],
        ]);
        entry.notes.push(format!(
            "Default room local ID {}; source ID {:?}; runtime ID {:?}.",
            room.id, room.source_room_id, room.runtime_room_id
        ));
        entry.notes.push("Room cutaway: existing walls, doorway surrounds, room light and already-prototyped storage. Shared deck floors, ceilings, perimeter walls, corridor ribs/markings, lift, entry pads and access-core surfaces are omitted.".into());
        entry.notes.push("Layout role names identify existing room shells; no additional utility or furnishing models are invented for those roles.".into());
        if room.runtime_room_id.as_deref() == Some(arkline_protocol::PIONEER_CONNECTED_ROOM.room_id)
        {
            entry.notes.push("Connected airlock also includes its existing breaker/vent bodies and service conduits, with commissioned mains/light on and door closed for inspection. No live room state is changed.".into());
        }
        entries.push(entry);
    }
    entries.extend(hangar::inspection_catalog::descriptors());
    entries
}

fn draws(parts: impl IntoIterator<Item = ModelPart>, material: Material) -> Vec<InspectionDraw> {
    parts
        .into_iter()
        .map(|part| InspectionDraw {
            part,
            material,
            range: None,
        })
        .collect()
}

fn recipe(id: &str) -> Result<(Vec<InspectionDraw>, f32), String> {
    if let Some(source_id) = id.strip_prefix("room/") {
        return Ok((
            draws(
                hangar::inspection_catalog::room_parts(source_id)?,
                Material::PaintedHull,
            ),
            1.0,
        ));
    }
    if let Some(result) = hangar::inspection_catalog::load(id) {
        return result;
    }
    if let Some(class) = id.strip_prefix("ship/").and_then(ship_class) {
        return Ok((
            draws(
                cached_ship_parts(class).iter().copied(),
                Material::BareMetal,
            ),
            100.0,
        ));
    }
    if let Some(class) = id.strip_prefix("turret/").and_then(ship_class) {
        return Ok((
            draws(
                turret_parts(class, Quat::IDENTITY, 0),
                Material::PaintedHull,
            ),
            100.0,
        ));
    }
    if let Some(class) = id.strip_prefix("cockpit/").and_then(ship_class) {
        // Translation into the ship is retained. Display fitting recenters only
        // the inspection camera; physical size follows the existing mount.
        let base = cockpit_fit::mount(Vec3::ZERO, Quat::IDENTITY, class);
        let parts = cached_cockpit_parts(class)
            .iter()
            .map(
                |&((mesh, model, color, emission), material)| InspectionDraw {
                    part: (mesh, base * model, color, emission),
                    material,
                    range: None,
                },
            )
            .collect();
        return Ok((parts, 100.0));
    }
    if let Some(slug) = id.strip_prefix("drone/") {
        let class = match slug {
            "recon" => DroneClass::Recon,
            "mining" => DroneClass::Mining,
            "attack" => DroneClass::Attack,
            _ => return Err("Unknown drone class".into()),
        };
        return Ok((
            draws(
                cached_drone_parts(class)?.iter().copied(),
                Material::PaintedHull,
            ),
            100.0,
        ));
    }
    if id == "station/exterior" {
        let geometry = flight_geometry()?;
        return Ok((
            draws(
                geometry
                    .station_batches
                    .iter()
                    .map(|&(mesh, color, emission)| (mesh, Mat4::IDENTITY, color, emission)),
                Material::BareMetal,
            ),
            100.0,
        ));
    }
    if let Some(slug) = id.strip_prefix("prop/") {
        let kind = match slug {
            "crate" => SurfacePropKind::Crate,
            "pylon" => SurfacePropKind::Pylon,
            "platform" => SurfacePropKind::Platform,
            _ => return Err("Unknown construction prop".into()),
        };
        let model = Mat4::from_scale(Vec3::from_array(kind.size().map(|v| v as f32 / 200.0)));
        return Ok((
            draws(
                world_visuals::surface_prop_parts(model),
                Material::PaintedHull,
            ),
            100.0,
        ));
    }
    if let Some(slug) = id.strip_prefix("geometry/") {
        let index = PRIMITIVES
            .iter()
            .position(|(name, _)| *name == slug)
            .ok_or("Unknown geometry recipe")?;
        return Ok((
            draws(
                [(index, Mat4::IDENTITY, Vec3::new(0.25, 0.34, 0.32), 0.0)],
                Material::PaintedHull,
            ),
            100.0,
        ));
    }
    if let Some(index) = id.strip_prefix("pioneer/material/") {
        let index = index
            .parse::<usize>()
            .map_err(|_| "Invalid Pioneer primitive")?;
        let p = pioneer_asset()?
            .get(index)
            .ok_or("Unknown Pioneer primitive")?;
        return Ok((
            draws(
                [(10 + index, Mat4::IDENTITY, p.color, p.emission)],
                Material::BareMetal,
            ),
            100.0,
        ));
    }
    if let Some(identity) = id.strip_prefix("exterior/") {
        let definition = arkline_protocol::default_pioneer_exterior();
        let fixture = definition
            .fixtures
            .iter()
            .find(|f| f.id == identity)
            .ok_or("Unknown exterior fixture")?;
        if fixture.kind == ExteriorFixtureKind::Window {
            return Ok((vec![windows::draw(fixture)?], 100.0));
        }
        let part = exterior_visuals::fixture_part(fixture, Mat4::IDENTITY, 1.0)
            .ok_or("Fixture has no separate runtime geometry")?;
        return Ok((draws([part], Material::PaintedHull), 100.0));
    }
    match id {
        "world/asteroid_ore" | "world/asteroid_depleted" => {
            let asteroid = arkline_protocol::asteroid_catalog(108).remove(0);
            let space = Space {
                origin: asteroid.position,
            };
            let ore = id.ends_with("_ore");
            let color = if ore {
                Vec3::new(0.36, 0.29, 0.18)
            } else {
                Vec3::splat(0.10)
            };
            Ok((
                draws(
                    [(3, asteroid_model(&asteroid, space, 0), color, 0.0)],
                    if ore { Material::Ore } else { Material::Rock },
                ),
                100.0,
            ))
        }
        "world/gate" => {
            let gate = arkline_protocol::gates().remove(0);
            let space = Space {
                origin: gate.position,
            };
            Ok((
                draws(
                    gate_models(&gate, space)
                        .map(|model| (1, model, Vec3::new(0.16, 0.43, 0.65), 0.7)),
                    Material::BareMetal,
                ),
                100.0,
            ))
        }
        "world/planet" => {
            let layout = arkline_protocol::legacy_world_layout();
            let planet = &layout.planets[0];
            Ok((
                draws(
                    [(
                        5,
                        Mat4::from_scale(Vec3::splat(planet.radius_m as f32 / 100.0)),
                        Vec3::ONE,
                        0.0,
                    )],
                    Material::PaintedHull,
                ),
                100.0,
            ))
        }
        "world/terrain" => Ok((
            draws(
                [(7, Mat4::IDENTITY, Vec3::new(0.24, 0.34, 0.22), 0.0)],
                Material::PaintedHull,
            ),
            100.0,
        )),
        _ => Err("No geometry recipe for this runtime entry".into()),
    }
}

fn measure(
    parts: Vec<InspectionDraw>,
    meters_per_unit: f32,
) -> Result<RuntimeInspectionModel, String> {
    if parts.is_empty() || !meters_per_unit.is_finite() || meters_per_unit <= 0.0 {
        return Err("Runtime recipe has no valid geometry units".into());
    }
    let geometry = flight_geometry()?;
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut triangles = 0usize;
    for draw in &parts {
        let (mesh, matrix, color, emission) = draw.part;
        if !matrix.is_finite() || !color.is_finite() || !emission.is_finite() {
            return Err("Runtime recipe has a non-finite draw".into());
        }
        let vertices = geometry
            .catalog
            .uploads()
            .get(mesh_index(mesh))
            .ok_or("Runtime recipe refers to an unknown GPU mesh")?;
        let vertices = draw.selected_vertices(vertices)?;
        triangles = triangles
            .checked_add(vertices.len() / 3)
            .ok_or("Triangle count overflow")?;
        for vertex in vertices.iter() {
            let point = matrix.transform_point3(vertex.p);
            if !point.is_finite() || !vertex.n.is_finite() {
                return Err("Runtime recipe contains non-finite vertices".into());
            }
            min = min.min(point);
            max = max.max(point);
        }
    }
    if !min.is_finite() || !max.is_finite() || (max - min).max_element() <= 0.0 {
        return Err("Runtime recipe has invalid measured bounds".into());
    }
    let min_m = min * meters_per_unit;
    let max_m = max * meters_per_unit;
    if !min_m.is_finite() || !max_m.is_finite() {
        return Err("Runtime recipe meter bounds overflow".into());
    }
    Ok(RuntimeInspectionModel {
        info: ModelGeometryInfo {
            min_m: min_m.to_array(),
            max_m: max_m.to_array(),
            triangles,
            batches: parts.len(),
        },
        parts,
        min,
        max,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_unique_source_ids_and_loads_every_available_recipe() {
        let catalog = runtime_model_catalog();
        let mut ids = std::collections::BTreeSet::new();
        assert_eq!(catalog.iter().filter(|e| e.category == "Ships").count(), 7);
        assert_eq!(catalog.iter().filter(|e| e.category == "Drones").count(), 3);
        assert_eq!(
            catalog
                .iter()
                .filter(|e| e.category == "Exterior fixtures")
                .count(),
            13
        );
        assert_eq!(
            catalog.iter().filter(|e| e.category == "Geometry").count(),
            10
        );
        for entry in catalog {
            assert!(
                ids.insert(entry.id.clone()),
                "Duplicate catalog ID {}",
                entry.id
            );
            assert!(!entry.name.is_empty() && !entry.source.is_empty());
            if entry.unavailable.is_some() {
                assert!(load_runtime_model(&entry.id).is_err());
                continue;
            }
            let model =
                load_runtime_model(&entry.id).unwrap_or_else(|e| panic!("{}: {e}", entry.id));
            assert!(model.min.is_finite() && model.max.is_finite());
            assert!((model.max - model.min).max_element() > 0.0);
            assert!(model.info.triangles > 0 && model.info.batches == model.parts.len());
            assert!(Arc::ptr_eq(&model, &load_runtime_model(&entry.id).unwrap()));
        }
        assert!(load_runtime_model("unknown/model").is_err());
    }

    #[test]
    fn all_geometric_entries_are_previewable_and_room_cutaways_keep_shared_draws() {
        let catalog = runtime_model_catalog();
        assert_eq!(catalog.len(), 111);
        let unavailable = catalog
            .iter()
            .filter(|entry| entry.unavailable.is_some())
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            unavailable,
            vec![
                "equipment/pioneer.deck1.airlock.supply",
                "equipment/pioneer.deck1.airlock.controller",
                "equipment/pioneer.deck1.airlock.pressure",
            ]
        );
        assert_eq!(
            catalog
                .iter()
                .filter(|entry| entry.unavailable.is_none())
                .count(),
            108
        );
        for room in arkline_protocol::default_pioneer().rooms {
            let source_id = room.source_room_id.as_deref().unwrap();
            let id = format!("room/{source_id}");
            let entry = catalog.iter().find(|entry| entry.id == id).unwrap();
            assert!(entry
                .notes
                .iter()
                .any(|note| note.contains("floors") && note.contains("omitted")));
            let model = load_runtime_model(&id).unwrap();
            let expected = hangar::inspection_catalog::room_parts(source_id).unwrap();
            assert_eq!(
                model.parts.iter().map(|draw| draw.part).collect::<Vec<_>>(),
                expected
            );
            assert!(model
                .parts
                .iter()
                .all(|draw| draw.range.is_none() && draw.material == Material::PaintedHull));
            assert_eq!(model.info.min_m, model.min.to_array());
            assert_eq!(model.info.max_m, model.max.to_array());
        }
        assert!(hangar::inspection_catalog::room_parts("D3-P1").is_err());
    }

    #[test]
    fn ship_drone_and_cockpit_previews_retain_exact_shared_draws_and_units() {
        for class in ALL_SHIP_CLASSES {
            let hull = load_runtime_model(&format!("ship/{}", ship_slug(class))).unwrap();
            let source = cached_ship_parts(class);
            assert_eq!(hull.parts.len(), source.len());
            for (actual, expected) in hull.parts.iter().zip(source) {
                assert_eq!(actual.part, *expected);
                assert_eq!(actual.material, Material::BareMetal);
            }
            let cockpit = load_runtime_model(&format!("cockpit/{}", ship_slug(class))).unwrap();
            let base = cockpit_fit::mount(Vec3::ZERO, Quat::IDENTITY, class);
            for (actual, &((mesh, model, color, emission), material)) in
                cockpit.parts.iter().zip(cached_cockpit_parts(class))
            {
                assert_eq!(actual.part, (mesh, base * model, color, emission));
                assert_eq!(actual.material, material);
            }
        }
        for slug in ["recon", "mining", "attack"] {
            let model = load_runtime_model(&format!("drone/{slug}")).unwrap();
            let size = Vec3::from_array(model.info.max_m) - Vec3::from_array(model.info.min_m);
            assert!(size.abs_diff_eq(Vec3::new(5.0, 1.8, 6.0), 1e-4));
            assert_eq!(model.parts.len(), 5);
        }
    }

    #[test]
    fn catalog_reads_real_appended_meshes_and_preserves_material_response() {
        let station = load_runtime_model("station/exterior").unwrap();
        let geometry = flight_geometry().unwrap();
        assert_eq!(station.parts.len(), geometry.station_batches.len());
        for (draw, &(mesh, color, emission)) in station.parts.iter().zip(&geometry.station_batches)
        {
            assert_eq!(draw.part, (mesh, Mat4::IDENTITY, color, emission));
            assert_eq!(draw.material, Material::BareMetal);
        }
        assert_eq!(
            load_runtime_model("world/asteroid_ore").unwrap().parts[0].material,
            Material::Ore
        );
        assert_eq!(
            load_runtime_model("world/asteroid_depleted").unwrap().parts[0].material,
            Material::Rock
        );
        assert!(load_runtime_model("world/gate")
            .unwrap()
            .parts
            .iter()
            .all(|part| part.material == Material::BareMetal));
        for slug in ["crate", "pylon", "platform"] {
            assert_eq!(
                load_runtime_model(&format!("prop/{slug}"))
                    .unwrap()
                    .parts
                    .len(),
                2
            );
        }
    }
}
