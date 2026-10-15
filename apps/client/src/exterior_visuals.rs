//! Confirmed powered exterior presentation. Fixtures remain bound to one exact hull.
use super::*;
use arkline_protocol::{ExteriorFixtureKind, ExteriorVesselState, ExteriorWorldState};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct EmissionPower {
    drive: Vec3,
    panels: Vec3,
    windows: Vec3,
    controlled: bool,
}
impl EmissionPower {
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
        match embedded_primitive(mesh).and_then(|p| p.material_index) {
            Some(4) => Some(self.drive),
            Some(5) => Some(self.panels),
            Some(6) => Some(self.windows),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Binding<'a> {
    world: &'a ExteriorWorldState,
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
    Some(Binding {
        world,
        vessel,
        now,
        live,
    })
}
pub(super) fn for_pilot<'a>(frame: &'a Frame, pilot: &PilotView) -> Option<Binding<'a>> {
    for_pilot_state(
        frame.exterior.as_deref(),
        pilot,
        frame.snapshot_time,
        frame.connected && frame.now.saturating_sub(frame.snapshot_time) <= 1500,
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
            ..Default::default()
        };
        for fixture in &self.world.definition.document.fixtures {
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
        self.world
            .definition
            .document
            .fixtures
            .iter()
            .filter_map(|fixture| {
                let output = self.output(fixture);
                if output <= 0. {
                    return None;
                }
                let (mesh, half) = match fixture.kind {
                    ExteriorFixtureKind::Navigation => (2, Vec3::splat(0.18)),
                    ExteriorFixtureKind::Strobe => (2, Vec3::splat(0.24)),
                    ExteriorFixtureKind::Window => (0, Vec3::new(0.06, 0.2, 0.35)),
                    ExteriorFixtureKind::Panel => (0, Vec3::new(0.08, 0.15, 0.25)),
                    // The static GLB's opaque aft cap can hide its recessed throat.
                    // A bounded axial glow surface uses the accepted nozzle mount,
                    // retaining the authored hull and existing dynamic exhaust.
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
            })
            .collect()
    }
    pub(super) fn lights(self, pose: Mat4) -> Vec<local_lighting::Light> {
        let scale = pose.transform_vector3(Vec3::X).length() / 100.;
        self.world
            .definition
            .document
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

impl Renderer {
    pub(super) fn prepare_exterior(&mut self, frame: &Frame) {
        let live = frame.connected && frame.now.saturating_sub(frame.snapshot_time) <= 1500;
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
