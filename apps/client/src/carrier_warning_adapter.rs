//! Exact1703 source-bound beacon presentation. No geometry or gameplay mutation.
use crate::carrier_warning_geometry::{
    self as warning, ConfirmedWarning, Definition, WarningVisual,
};
use arkline_protocol::{
    carrier_camera_devices::CameraAvailability, carrier_inventory_geometry,
    drone_bay_geometry::BayPose, CarrierInventoryAction, CarrierInventoryPublicState,
    CarrierOperationPhase, CarrierPresentationRoute, CarrierRouteKind, DroneClass, PilotView,
};
const SOURCE: &str = "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a";
#[derive(Clone, Copy)]
struct DoorTravel {
    closed: BayPose,
    delta: [f64; 3],
}
fn fraction(travel: DoorTravel, current: BayPose) -> Option<f64> {
    if !current.valid()
        || !(current.orientation_xyzw == travel.closed.orientation_xyzw
            || current.orientation_xyzw == travel.closed.orientation_xyzw.map(|v| -v))
    {
        return None;
    }
    let actual: [f64; 3] =
        std::array::from_fn(|i| current.position_m[i] - travel.closed.position_m[i]);
    let length2 = travel.delta.iter().map(|v| v * v).sum::<f64>();
    if length2 <= 1e-10 {
        return None;
    }
    let t = actual
        .iter()
        .zip(travel.delta)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        / length2;
    if !(-1e-6..=1. + 1e-6).contains(&t)
        || (0..3).any(|i| (actual[i] - travel.delta[i] * t).abs() > 1e-5)
    {
        return None;
    }
    Some(t.clamp(0., 1.))
}
fn owner(route: &CarrierPresentationRoute) -> u8 {
    match route {
        CarrierPresentationRoute::Hardware {
            route: CarrierRouteKind::Stage {
                destination_berth, ..
            },
            ..
        } => *destination_berth,
        CarrierPresentationRoute::Hardware {
            route: CarrierRouteKind::Bay { berth_id, .. },
            ..
        }
        | CarrierPresentationRoute::Approach { berth_id, .. }
        | CarrierPresentationRoute::Departure { berth_id, .. } => *berth_id,
    }
}
fn source_travel(public: &CarrierInventoryPublicState) -> Option<&'static [[DoorTravel; 2]; 10]> {
    static TRAVEL: std::sync::OnceLock<Option<[[DoorTravel; 2]; 10]>> = std::sync::OnceLock::new();
    TRAVEL
        .get_or_init(|| {
            let def = carrier_inventory_geometry::stationary_fixture_for_registration(
                &public.binding().registration,
            )?;
            let initial: Vec<_> = def
                .cassettes
                .iter()
                .map(|c| (c.cassette_id, c.reference_pose))
                .collect();
            let idle = def.idle_roots(&initial)?;
            let mut cells: Vec<[DoorTravel; 2]> = Vec::new();
            for cell in 0..10u8 {
                let route = def.bay_route(
                    cell,
                    DroneClass::Mining,
                    u64::from(cell),
                    CarrierInventoryAction::Launch,
                )?;
                let mut doors = Vec::new();
                for leaf in ["lower", "upper"] {
                    let id = format!("b{cell:02}/door/{leaf}");
                    let closed = idle.iter().find(|(key, _)| key == &id)?.1;
                    let delta = route
                        .iter()
                        .filter_map(|f| f.machinery.iter().find(|(key, _)| key == &id))
                        .map(|(_, p)| {
                            std::array::from_fn::<_, 3, _>(|i| {
                                p.position_m[i] - closed.position_m[i]
                            })
                        })
                        .max_by(|a, b| {
                            a.iter()
                                .map(|v| v * v)
                                .sum::<f64>()
                                .total_cmp(&b.iter().map(|v| v * v).sum::<f64>())
                        })?;
                    if delta.iter().map(|v| v * v).sum::<f64>() <= 1e-10 {
                        return None;
                    }
                    doors.push(DoorTravel { closed, delta });
                }
                cells.push(doors.try_into().ok()?);
            }
            cells.try_into().ok()
        })
        .as_ref()
}
/// Call on the same accepted sample used for source/body rendering. Emission is
/// clocked only by that authority sample: pause/stale views cannot advance flash.
/// None means unavailable/dark, not a fabricated secured/healthy state.
pub(crate) fn visuals(
    pilot: &PilotView,
    public: &CarrierInventoryPublicState,
) -> [Option<WarningVisual>; 10] {
    let unavailable = [None; 10];
    let reg = &public.binding().registration;
    if reg.assembly_revision != 1703
        || reg.assembly_source_sha256 != SOURCE
        || public.validate_projection(pilot).is_err()
    {
        return unavailable;
    }
    let Some(travel) = source_travel(public) else {
        return unavailable;
    };
    let Some(cameras) = public.cameras.as_ref() else {
        return unavailable;
    };
    std::array::from_fn(|cell| {
        let side = if cell < 5 { "port" } else { "starboard" };
        let camera_id = format!("pioneer.carrier.camera.loader.{side}.{}", cell % 5);
        let powered = cameras
            .devices
            .iter()
            .find(|d| d.device_id == camera_id)?
            .availability
            == CameraAvailability::Available;
        let mut door_fraction: f64 = 0.;
        for (leaf, travel) in ["lower", "upper"].into_iter().zip(travel[cell]) {
            let id = format!("b{cell:02}/door/{leaf}");
            let current = public.machinery.iter().find(|(key, _)| key == &id)?.1;
            door_fraction = door_fraction.max(fraction(travel, current)?);
        }
        let active = public
            .motions
            .iter()
            .any(|m| usize::from(owner(&m.route)) == cell);
        let held = public
            .motions
            .iter()
            .any(|m| usize::from(owner(&m.route)) == cell && m.held)
            || public.inventory.operations.iter().any(|op| {
                op.berth_id == Some(cell as u8) && op.phase == CarrierOperationPhase::Held
            });
        let device_id = format!("pioneer.carrier.warning.loader.{side}.{}", cell % 5);
        let parent_id = format!("cell{cell:02}/loader/trolley");
        let definition = Definition {
            device_id: &device_id,
            source_sha256: SOURCE,
            revision: 1,
            parent_id: &parent_id,
        };
        let state = ConfirmedWarning {
            vessel_id: &public.binding().vessel_id,
            device_id: &device_id,
            source_sha256: SOURCE,
            revision: 1,
            parent_id: &parent_id,
            sampled_at_ms: public.sample_time_ms,
            supply_available: powered,
            door_fraction,
            // No inferred door velocity: actual motion ownership and measured
            // door displacement already activate the warning.
            door_moving: false,
            loader_active: active,
            held_fault: held,
        };
        warning::visual(
            &definition,
            &state,
            &public.binding().vessel_id,
            public.sample_time_ms,
            true,
        )
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pose(y: f64) -> BayPose {
        BayPose {
            position_m: [0., y, 0.],
            orientation_xyzw: [0., 0., 0., 1.],
        }
    }
    #[test]
    fn door_fraction_is_actual_source_travel_not_elapsed_time() {
        let travel = DoorTravel {
            closed: pose(0.),
            delta: [0., 2., 0.],
        };
        assert_eq!(fraction(travel, pose(0.)), Some(0.));
        assert_eq!(fraction(travel, pose(1.)), Some(0.5));
        assert_eq!(fraction(travel, pose(2.)), Some(1.));
        assert_eq!(fraction(travel, pose(3.)), None);
        let mut wrong = pose(1.);
        wrong.position_m[0] = 0.01;
        assert_eq!(fraction(travel, wrong), None);
    }
    #[test]
    fn actual_route_owner_selects_only_its_berth() {
        assert_eq!(
            owner(&CarrierPresentationRoute::Departure {
                berth_id: 7,
                cassette_id: 17
            }),
            7
        );
        assert_eq!(
            owner(&CarrierPresentationRoute::Hardware {
                route: CarrierRouteKind::Bay {
                    berth_id: 2,
                    cassette_id: 2,
                    action: CarrierInventoryAction::Launch
                },
                target_index: 4
            }),
            2
        );
    }
    #[test]
    fn idle_loading_launch_held_and_power_loss_are_presentation_only() {
        let definition = Definition {
            device_id: "beacon",
            source_sha256: SOURCE,
            revision: 1,
            parent_id: "cell00/loader/trolley",
        };
        let mut state = ConfirmedWarning {
            vessel_id: "owned",
            device_id: "beacon",
            source_sha256: SOURCE,
            revision: 1,
            parent_id: "cell00/loader/trolley",
            sampled_at_ms: 1000,
            supply_available: true,
            door_fraction: 0.,
            door_moving: false,
            loader_active: false,
            held_fault: false,
        };
        let sample =
            |s: &ConfirmedWarning<'_>| warning::visual(&definition, s, "owned", 1000, true);
        assert_eq!(sample(&state).unwrap().mode, warning::WarningMode::Secured);
        state.loader_active = true;
        assert_eq!(
            sample(&state).unwrap().mode,
            warning::WarningMode::Operating
        );
        assert_eq!(sample(&state), sample(&state));
        state.loader_active = false;
        state.door_fraction = 0.5;
        assert_eq!(
            sample(&state).unwrap().mode,
            warning::WarningMode::Operating
        );
        state.held_fault = true;
        assert_eq!(
            sample(&state).unwrap().mode,
            warning::WarningMode::HeldFault
        );
        state.supply_available = false;
        assert!(sample(&state).is_none());
    }
}
