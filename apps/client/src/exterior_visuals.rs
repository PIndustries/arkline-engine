//! Confirmed powered exterior presentation. Fixtures remain bound to one exact hull.
use super::*;
use arkline_protocol::{ExteriorFixtureKind, ExteriorVesselState, ExteriorWorldState};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct EmissionPower {
    drive: Vec3,
    panels: Vec3,
    windows: Vec3,
    controlled: bool,
    carrier: bool,
}
impl EmissionPower {
    pub(super) fn for_binding(binding: Option<Binding<'_>>, carrier_present: bool) -> Self {
        binding.map_or_else(
            || Self {
                controlled: carrier_present,
                carrier: carrier_present,
                ..Default::default()
            },
            |b| b.emission_power(),
        )
    }

    pub(super) fn for_mesh(self, mesh: usize) -> f32 {
        if self
            .override_for_mesh(mesh)
            .is_some_and(|emitted| emitted.max_element() == 0.)
        {
            0.
        } else {
            1.
        }
    }
    pub(super) fn override_for_mesh(self, mesh: usize) -> Option<Vec3> {
        if !self.controlled {
            return None;
        }
        let role = if self.carrier {
            carrier_exterior_material_role(mesh)
        } else {
            embedded_primitive(mesh).and_then(|p| p.material_index)
        };
        match role {
            Some(4) => Some(self.drive),
            Some(5) => Some(self.panels),
            Some(6) => Some(self.windows),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Binding<'a> {
    definition: &'a arkline_protocol::ExteriorDefinition,
    carrier: bool,
    vessel: &'a ExteriorVesselState,
    now: u64,
    live: bool,
}
pub(super) fn for_vessel<'a>(
    world: Option<&'a ExteriorWorldState>,
    vessel_id: &str,
    now: u64,
    live: bool,
) -> Option<Binding<'a>> {
    let world = world?;
    // Neither names nor retained material indices authorize a different hull.
    if world.definition.document.hull_asset_sha256 != pioneer_asset_sha256()
        || world.definition.document.validate().is_err()
    {
        return None;
    }
    let vessel = world
        .vessels
        .iter()
        .find(|v| v.vessel_id == vessel_id && v.ship_class == ShipClass::Pioneer)?;
    if vessel.source_definition.is_some() {
        // Source-bound vessels need their accepted PilotView, never a legacy fallback.
        return None;
    }
    Some(Binding {
        definition: &world.definition.document,
        carrier: false,
        vessel,
        now,
        live,
    })
}
pub(super) fn for_pilot<'a>(frame: &'a Frame, pilot: &PilotView) -> Option<Binding<'a>> {
    if frame.carrier_advertised(pilot) && frame.ship_mount(pilot).is_none() {
        return None;
    }
    for_pilot_state(
        frame.exterior.as_deref(),
        pilot,
        frame.snapshot_time,
        if frame.carrier_advertised(pilot) {
            frame.presentation_live()
        } else {
            frame.connected && frame.now.saturating_sub(frame.snapshot_time) <= 1500
        },
    )
}
pub(super) fn for_pilot_state<'a>(
    world: Option<&'a ExteriorWorldState>,
    pilot: &PilotView,
    now: u64,
    live: bool,
) -> Option<Binding<'a>> {
    let world = world?;
    let vessel = world
        .vessels
        .iter()
        .find(|v| v.pilot == pilot.name && v.ship_class == pilot.ship_class)?;
    if let Some(public) = &pilot.carrier_inventory {
        let binding = public.binding();
        if public.sample_time_ms != now
            || binding.vessel_id != vessel.vessel_id
            || world
                .vessels
                .iter()
                .filter(|v| v.vessel_id == binding.vessel_id)
                .count()
                != 1
            || world
                .vessels
                .iter()
                .filter(|v| v.pilot == pilot.name)
                .count()
                != 1
        {
            return None;
        }
        public.validate_projection(pilot).ok()?;
        let installed = flight_geometry().ok()?;
        installed
            .carriers
            .iter()
            .filter_map(|v| v.as_ref().ok())
            .find(|c| c.registration == binding.registration)?;
        let definition = vessel.source_definition.as_ref()?;
        if definition.assembly_revision != binding.registration.assembly_revision
            || definition.source_sha256 != binding.registration.assembly_source_sha256
        {
            return None;
        }
        arkline_protocol::parallel_exterior::validate_against(definition, &world.definition)
            .ok()?;
        return Some(Binding {
            definition: &definition.document,
            carrier: true,
            vessel,
            now,
            live,
        });
    }
    for_vessel(Some(world), &vessel.vessel_id, now, live)
}
pub(super) struct Instance {
    pub(super) emission: EmissionPower,
    pub(super) hull: Vec<ModelPart>,
    pub(super) fixtures: Vec<ModelPart>,
    pub(super) lights: Vec<local_lighting::Light>,
}
impl Binding<'_> {
    pub(super) fn instance(self, pose: Mat4, hull: Vec<ModelPart>) -> Instance {
        Instance {
            emission: self.emission_power(),
            hull,
            fixtures: self.parts(pose),
            lights: self.lights(pose),
        }
    }
    fn output(self, fixture: &arkline_protocol::ExteriorFixture) -> f32 {
        if self.live {
            fixture.output(self.vessel, self.now)
        } else {
            0.
        }
    }
    pub(super) fn emission_power(self) -> EmissionPower {
        let mut result = EmissionPower {
            controlled: true,
            carrier: self.carrier,
            ..Default::default()
        };
        for fixture in &self.definition.fixtures {
            let emitted =
                Vec3::from_array(fixture.color) * (fixture.emission * self.output(fixture));
            // The published GLB batches these source parts by material. The
            // maximum channel energy drives that shared batch; individual
            // fixture lenses and illumination retain their own values.
            match fixture.kind {
                ExteriorFixtureKind::EngineGlow => result.drive = result.drive.max(emitted),
                ExteriorFixtureKind::Panel => result.panels = result.panels.max(emitted),
                ExteriorFixtureKind::Window => result.windows = result.windows.max(emitted),
                _ => {}
            }
        }
        result
    }
    pub(super) fn parts(self, pose: Mat4) -> Vec<ModelPart> {
        self.definition
            .fixtures
            .iter()
            .filter_map(|fixture| fixture_part(fixture, pose, self.output(fixture)))
            .collect()
    }
    pub(super) fn lights(self, pose: Mat4) -> Vec<local_lighting::Light> {
        let scale = pose.transform_vector3(Vec3::X).length() / 100.;
        self.definition
            .fixtures
            .iter()
            .filter_map(|fixture| {
                let output = self.output(fixture);
                (output > 0. && fixture.light_range_m > 0. && fixture.light_intensity > 0.).then(
                    || local_lighting::Light {
                        position: pose.transform_point3(
                            Vec3::from_array(fixture.position_m.map(|v| v as f32)) / 100.,
                        ),
                        range: fixture.light_range_m * scale,
                        color: Vec3::from_array(fixture.color),
                        power: fixture.light_intensity * output,
                    },
                )
            })
            .collect()
    }
}

/// The exact separate lens/body recipe. Window surfaces remain authored GLB panes.
pub(super) fn fixture_part(
    fixture: &arkline_protocol::ExteriorFixture,
    pose: Mat4,
    output: f32,
) -> Option<ModelPart> {
    if output <= 0. {
        return None;
    }
    let (mesh, half) = match fixture.kind {
        ExteriorFixtureKind::Navigation => (2, Vec3::splat(0.18)),
        ExteriorFixtureKind::Strobe => (2, Vec3::splat(0.24)),
        // The authored recessed panes provide the Window surface; a
        // second axis-aligned cube would sit proud of wrapped glass.
        ExteriorFixtureKind::Window => return None,
        ExteriorFixtureKind::Panel => (0, Vec3::new(0.08, 0.15, 0.25)),
        // The accepted mount sits at the actual recessed throat,
        // inside the authored opening. Existing dynamic exhaust remains.
        ExteriorFixtureKind::EngineGlow => (2, Vec3::new(0.18, 1.1, 1.1)),
    };
    Some((
        mesh,
        pose * transform(
            Vec3::from_array(fixture.position_m.map(|v| v as f32)) / 100.,
            half / 100.,
            Quat::IDENTITY,
        ),
        Vec3::from_array(fixture.color),
        fixture.emission * output,
    ))
}

impl Renderer {
    pub(super) fn prepare_exterior(&mut self, frame: &Frame) {
        let live = if frame
            .scene
            .pilots
            .iter()
            .any(|p| p.carrier_inventory.is_some())
        {
            frame.presentation_live()
        } else {
            frame.connected && frame.now.saturating_sub(frame.snapshot_time) <= 1500
        };
        let next = (frame.exterior.clone(), live);
        if self.exterior_cache.as_ref() != Some(&next) {
            for feed in &mut self.feed_cache {
                feed.painted_ms = None;
            }
            self.exterior_cache = Some(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn world() -> ExteriorWorldState {
        ExteriorWorldState {
            definition: arkline_protocol::ExteriorDefinitionState {
                revision: 1,
                document: arkline_protocol::default_pioneer_exterior(),
            },
            vessels: vec![ExteriorVesselState {
                pilot: "fixture".into(),
                ship_class: ShipClass::Pioneer,
                vessel_id: "v1:fixture:Pioneer".into(),
                enabled: false,
                powered: false,
                drive: arkline_protocol::ExteriorDriveState::Off,
                load_w: 0,
                source_definition: None,
            }],
        }
    }
    #[test]
    fn exterior_requires_exact_hull_and_stable_vessel_identity() {
        let mut world = world();
        assert!(for_vessel(Some(&world), "wrong", 1000, true).is_none());
        let binding = for_vessel(Some(&world), "v1:fixture:Pioneer", 1000, true).unwrap();
        assert!(binding.parts(Mat4::IDENTITY).is_empty());
        assert!(binding.lights(Mat4::IDENTITY).is_empty());
        for mesh in 10..19 {
            if matches!(
                embedded_primitive(mesh).unwrap().material_index,
                Some(4..=6)
            ) {
                assert_eq!(binding.emission_power().for_mesh(mesh), 0.);
            }
        }
        world.definition.document.hull_asset_sha256 = "0".repeat(64);
        assert!(for_vessel(Some(&world), "v1:fixture:Pioneer", 1000, true).is_none());
    }
    #[test]
    fn exterior_drive_active_survives_branch_off_and_stale_state_goes_dark() {
        let mut world = world();
        world.vessels[0].drive = arkline_protocol::ExteriorDriveState::Active;
        let binding = for_vessel(Some(&world), "v1:fixture:Pioneer", 1000, true).unwrap();
        assert!(binding.emission_power().drive.max_element() > 0.);
        assert_eq!(binding.emission_power().windows, Vec3::ZERO);
        let stale = for_vessel(Some(&world), "v1:fixture:Pioneer", 1000, false).unwrap();
        assert_eq!(stale.emission_power().drive, Vec3::ZERO);
        assert!(stale.lights(Mat4::IDENTITY).is_empty());
    }
}

#[cfg(test)]
mod carrier_tests {
    use super::*;
    #[test]
    fn missing_carrier_binding_is_dark_and_per_vessel_glass_state_is_independent() {
        let g = flight_geometry().unwrap();
        let c = g
            .carriers
            .iter()
            .filter_map(|c| c.as_ref().ok())
            .find(|c| c.registration.assembly_revision == 1703)
            .unwrap();
        let glass = c
            .exterior_roles
            .iter()
            .find(|(_, role)| *role == 6)
            .unwrap()
            .0;
        let dark = EmissionPower::for_binding(None, true);
        let lit = EmissionPower {
            controlled: true,
            carrier: true,
            windows: Vec3::new(0.1, 0.8, 0.2),
            ..Default::default()
        };
        let queued = [(glass, dark), (glass, lit)];
        assert_eq!(queued[0].1.override_for_mesh(glass), Some(Vec3::ZERO));
        assert_eq!(queued[1].1.override_for_mesh(glass), Some(lit.windows));
        for &(mesh, _) in &c.warning_beacons {
            assert_eq!(dark.override_for_mesh(mesh), None);
        }
        assert_eq!(
            EmissionPower::for_binding(None, false),
            EmissionPower::default()
        );
    }
    #[test]
    fn exact_material_roles_do_not_reuse_legacy_or_warning_slots() {
        let g = flight_geometry().unwrap();
        let c = g
            .carriers
            .iter()
            .filter_map(|c| c.as_ref().ok())
            .find(|c| c.registration.assembly_revision == 1703)
            .unwrap();
        assert!(!c.exterior_roles.is_empty());
        for &(mesh, role) in &c.exterior_roles {
            assert_eq!(carrier_exterior_material_role(mesh), Some(role));
            assert!(matches!(role, 4..=6));
            assert!(!c.warning_beacons.iter().any(|(m, _)| *m == mesh));
        }
        for &(mesh, _) in &c.warning_beacons {
            assert_eq!(carrier_exterior_material_role(mesh), None);
        }
        for mesh in 0..10 {
            assert_eq!(carrier_exterior_material_role(mesh), None);
        }
    }
    #[test]
    #[ignore = "requires pinned actual847b Response; source-definition overlay is a test fixture, not authority evidence"]
    fn actual_carrier_projection_requires_matching_exterior_source_vessel_and_clock() {
        use sha2::{Digest, Sha256};
        let root =
            std::path::PathBuf::from(std::env::var("ARKLINE_CARRIER_EVIDENCE_ROOT").unwrap());
        let bytes =
            std::fs::read(root.join("parallel-camera-onfoot-847b/response-1793007142435.json"))
                .unwrap();
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            "4a5bc5afdfafbc31c411f17ccb385b28614bf0997a4a5e86c26babdcfcc932bb"
        );
        let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
        let snapshot = response.snapshot.unwrap();
        let pilot = snapshot
            .pilots
            .iter()
            .find(|p| p.carrier_inventory.is_some())
            .unwrap();
        let public = pilot.carrier_inventory.as_ref().unwrap();
        let mut frame = super::super::tests::frame_for_scene_clone_test();
        frame.own = pilot.name.clone();
        frame.snapshot_time = snapshot.server_time_ms;
        frame.now = snapshot.server_time_ms;
        let scene = Arc::make_mut(&mut frame.scene);
        scene.pilots = vec![pilot.clone()];
        scene.carrier_inventory_registration = Some(public.binding().registration.clone());
        assert_eq!(frame.carrier_instances(pilot).unwrap().len(), 20);
        Arc::make_mut(&mut frame.scene)
            .carrier_inventory_registration
            .as_mut()
            .unwrap()
            .assembly_source_sha256 = "0".repeat(64);
        assert!(frame.ship_mount(pilot).is_none());
        assert!(frame.carrier_instances(pilot).is_none());
        assert!(frame.canonical_pioneer_parts().is_none());
        let mut world = ExteriorWorldState {
            definition: Default::default(),
            vessels: vec![],
        };
        let definition = arkline_protocol::parallel_exterior::for_exact_source(
            1703,
            &public.binding().registration.assembly_source_sha256,
            &world.definition,
        )
        .unwrap();
        world.vessels.push(ExteriorVesselState {
            pilot: pilot.name.clone(),
            ship_class: pilot.ship_class,
            vessel_id: public.binding().vessel_id.clone(),
            enabled: true,
            powered: true,
            drive: arkline_protocol::ExteriorDriveState::Idle,
            load_w: 0,
            source_definition: Some(definition),
        });
        let now = snapshot.server_time_ms;
        assert!(for_pilot_state(Some(&world), pilot, now, true).is_some());
        assert!(for_pilot_state(Some(&world), pilot, now + 1, true).is_none());
        assert!(for_vessel(Some(&world), &public.binding().vessel_id, now, true).is_none());
        world.vessels[0]
            .source_definition
            .as_mut()
            .unwrap()
            .device_revision += 1;
        assert!(for_pilot_state(Some(&world), pilot, now, true).is_none());
        world.vessels[0]
            .source_definition
            .as_mut()
            .unwrap()
            .device_revision -= 1;
        world.vessels[0].vessel_id.push_str("foreign");
        assert!(for_pilot_state(Some(&world), pilot, now, true).is_none());
        world.vessels[0].vessel_id = public.binding().vessel_id.clone();
        world.vessels[0].source_definition = None;
        assert!(for_pilot_state(Some(&world), pilot, now, true).is_none());
    }
}
