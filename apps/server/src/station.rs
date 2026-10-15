//! Server-authoritative walking inside the pilot's docked private hangar.
use super::*;
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct WalkInput {
    forward: f64,
    strafe: f64,
    turn: f64,
    accepted_ms: u64,
}
pub(super) fn stop(p: &mut Pilot) {
    p.walk_input = None;
    if let Some(a) = &mut p.view.station_avatar {
        a.moving = false;
        a.jump_started_ms = None;
        a.position[1] = 0.0;
    }
}
pub(super) fn execute(w: &mut World, name: &str, r: Request, now: u64) -> Result<()> {
    let fleet = w.accounts.get(name).is_some_and(|a| a.fleet_hangar);
    let p = w.pilots.get_mut(name).context("pilot missing")?;
    ensure!(
        p.view.location == Location::Docked,
        "station walking requires docking"
    );
    let layout = layout_for(p.view.ship_class, fleet);
    match r {
        Request::EnterInterior => {
            ensure!(
                p.view.ship_class == ShipClass::Pioneer,
                "only Pioneer interiors are walkable"
            );
            if let Some(a) = &p.view.station_avatar {
                ensure!(
                    !a.onboard_interior
                        && a.jump_started_ms.is_none()
                        && flight::distance(a.position, layout.boarding_point)
                            <= STATION_BOARD_RANGE_M,
                    "return to the boarding marker before entering the interior"
                );
            }
            p.walk_input = None;
            p.view.station_avatar = Some(StationAvatar {
                onboard_interior: true,
                interior_deck: 1,
                jump_started_ms: None,
                seated: false,
                position: PIONEER_INTERIOR_ENTRY,
                heading: std::f64::consts::FRAC_PI_2,
                moving: false,
                updated_at_ms: now,
            });
        }
        Request::InteriorLift { deck } => {
            let a = p
                .view
                .station_avatar
                .as_mut()
                .context("enter the interior before using its lift")?;
            ensure!(
                p.view.ship_class == ShipClass::Pioneer && a.onboard_interior,
                "the lift belongs to the Pioneer interior"
            );
            ensure!(
                (1..=2).contains(&deck) && deck != a.interior_deck,
                "choose the other interior deck"
            );
            ensure!(
                a.jump_started_ms.is_none()
                    && !a.seated
                    && flight::distance(a.position, PIONEER_LIFT_POINT) <= PIONEER_LIFT_RANGE,
                "stand on the marked access-core lift landing first"
            );
            a.interior_deck = deck;
            a.position = PIONEER_LIFT_POINT;
            a.moving = false;
            a.updated_at_ms = now;
            p.walk_input = None;
        }
        Request::ExitInterior => {
            let a = p
                .view
                .station_avatar
                .as_ref()
                .context("not inside the ship")?;
            ensure!(
                a.onboard_interior
                    && a.interior_deck == 1
                    && a.jump_started_ms.is_none()
                    && flight::distance(a.position, PIONEER_INTERIOR_ENTRY)
                        <= STATION_BOARD_RANGE_M,
                "return to the interior airlock before leaving"
            );
            p.walk_input = None;
            p.view.station_avatar = Some(StationAvatar {
                onboard_interior: false,
                interior_deck: 1,
                jump_started_ms: None,
                seated: false,
                position: layout.boarding_point,
                heading: 0.0,
                moving: false,
                updated_at_ms: now,
            });
        }

        Request::DisembarkShip => {
            ensure!(p.view.station_avatar.is_none(), "already on foot");
            p.controls = None;
            p.view.station_avatar = Some(StationAvatar {
                onboard_interior: false,
                interior_deck: 1,
                jump_started_ms: None,
                seated: false,
                position: layout.boarding_point,
                heading: 0.0,
                moving: false,
                updated_at_ms: now,
            });
        }
        Request::EnterShip => {
            let a = p.view.station_avatar.as_ref().context("already aboard")?;
            ensure!(
                !a.onboard_interior,
                "leave the interior through its airlock before boarding"
            );
            ensure!(
                a.jump_started_ms.is_none(),
                "land before boarding your ship"
            );
            ensure!(
                flight::distance(a.position, layout.boarding_point) <= STATION_BOARD_RANGE_M,
                "walk to your ship boarding point first"
            );
            p.view.station_avatar = None;
            p.walk_input = None;
        }
        Request::StationSit { seated } => {
            let a = p
                .view
                .station_avatar
                .as_mut()
                .context("disembark before using quarters seating")?;
            ensure!(!a.onboard_interior, "interior seats are not operational");
            ensure!(a.jump_started_ms.is_none(), "land before using seating");
            if seated {
                let seat = if fleet {
                    fleet_quarters_layout()
                } else {
                    quarters_layout(p.view.ship_class)
                }
                .seat_point;
                ensure!(
                    flight::distance(a.position, seat) <= 2.0,
                    "walk closer to the quarters sofa first"
                );
                a.position = seat;
                a.heading = std::f64::consts::FRAC_PI_2;
            }
            a.seated = seated;
            a.moving = false;
            a.updated_at_ms = a.updated_at_ms.max(now);
            p.walk_input = None;
        }
        Request::StationJump => {
            let a = p
                .view
                .station_avatar
                .as_mut()
                .context("disembark before jumping")?;
            ensure!(!a.seated, "stand up before jumping");
            ensure!(a.jump_started_ms.is_none(), "already airborne");
            a.jump_started_ms = Some(now);
            a.updated_at_ms = a.updated_at_ms.max(now);
        }
        Request::StationWalk {
            forward,
            strafe,
            turn,
        } => {
            ensure!(
                [forward, strafe, turn]
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= 1.0),
                "walk inputs must be finite within -1 through 1"
            );
            let a = p
                .view
                .station_avatar
                .as_mut()
                .context("disembark before walking")?;
            if forward != 0.0 || strafe != 0.0 || turn != 0.0 {
                a.seated = false;
            }
            a.updated_at_ms = a.updated_at_ms.max(now);
            a.moving = forward != 0.0 || strafe != 0.0 || turn != 0.0;
            p.walk_input = Some(WalkInput {
                forward,
                strafe,
                turn,
                accepted_ms: now,
            });
        }
        _ => bail!("unsupported station command"),
    }
    Ok(())
}
fn in_obstacle(position: [f64; 3], obstacle: &StationObstacle) -> bool {
    let r = STATION_AVATAR_RADIUS_M;
    position[0] > obstacle.min[0] - r
        && position[0] < obstacle.max[0] + r
        && position[2] > obstacle.min[2] - r
        && position[2] < obstacle.max[2] + r
}
/// Existing saves may place an avatar where new walls or furnishings now stand.
pub(super) fn migrate(w: &mut World) {
    let doors: std::collections::BTreeMap<_, _> = w
        .pilots
        .keys()
        .filter_map(|name| room_controls::door_obstacle(w, name).map(|o| (name.clone(), o)))
        .collect();
    for (name, p) in &mut w.pilots {
        let fleet = w.accounts.get(name).is_some_and(|a| a.fleet_hangar);
        if let Some(a) = &mut p.view.station_avatar {
            if obstacles_for(p.view.ship_class, a.onboard_interior, fleet)
                .iter()
                .chain(doors.get(name))
                .any(|o| in_obstacle(a.position, o))
            {
                a.position = if a.onboard_interior {
                    PIONEER_INTERIOR_ENTRY
                } else {
                    layout_for(p.view.ship_class, fleet).boarding_point
                };
                a.seated = false;
                a.moving = false;
                a.jump_started_ms = None;
                a.position[1] = 0.0;
                p.walk_input = None;
            }
        }
    }
}
pub(super) fn advance(w: &mut World, now: u64) {
    let doors: std::collections::BTreeMap<_, _> = w
        .pilots
        .keys()
        .filter_map(|name| room_controls::door_obstacle(w, name).map(|o| (name.clone(), o)))
        .collect();
    for (name, p) in &mut w.pilots {
        let fleet = w.accounts.get(name).is_some_and(|a| a.fleet_hangar);
        let Some(a) = &mut p.view.station_avatar else {
            continue;
        };
        if p.view.location != Location::Docked {
            p.view.station_avatar = None;
            p.walk_input = None;
            continue;
        }
        if let Some(start) = a.jump_started_ms {
            let elapsed = now.saturating_sub(start) as f64 / 1000.0;
            if elapsed >= 2.0 * STATION_JUMP_SPEED_MPS / STATION_GRAVITY_MPS2 {
                a.jump_started_ms = None;
                a.position[1] = 0.0;
            } else {
                a.position[1] = STATION_JUMP_SPEED_MPS * elapsed
                    - 0.5 * STATION_GRAVITY_MPS2 * elapsed * elapsed;
            }
        }
        let Some(input) = &p.walk_input else {
            a.moving = false;
            a.updated_at_ms = a.updated_at_ms.max(now);
            continue;
        };
        let expires = input.accepted_ms.saturating_add(STATION_WALK_LEASE_MS);
        let until = now.min(expires);
        let mut remaining = until
            .saturating_sub(a.updated_at_ms)
            .min(STATION_WALK_LEASE_MS) as f64
            / 1000.0;
        let layout = layout_for(p.view.ship_class, fleet);
        let hull = p.view.ship_class.spec();
        let mut obstacles = obstacles_for(p.view.ship_class, a.onboard_interior, fleet);
        if let Some(door) = doors.get(name) {
            obstacles.push(*door);
        }
        let (min_x, max_x, min_z, max_z) = if a.onboard_interior {
            (0.3, 31.7, 0.3, 13.7)
        } else {
            (
                -layout.half_length + 0.3,
                layout.half_length - 0.3,
                -layout.half_width + 0.3,
                if fleet { 3.0 } else { layout.half_width - 0.3 },
            )
        };
        let norm = input.forward.hypot(input.strafe).max(1.0);
        while remaining > 0.0 {
            let dt = remaining.min(0.025);
            remaining -= dt;
            a.heading = (a.heading + input.turn * 2.0 * dt).rem_euclid(std::f64::consts::TAU);
            let (sin, cos) = a.heading.sin_cos();
            let x = (a.position[0]
                + (cos * input.forward - sin * input.strafe) / norm * WALK_SPEED_MPS * dt)
                .clamp(min_x, max_x);
            let z = (a.position[2]
                + (sin * input.forward + cos * input.strafe) / norm * WALK_SPEED_MPS * dt)
                .clamp(min_z, max_z);
            let clear = |position: [f64; 3]| {
                (a.onboard_interior
                    || fleet
                    || position[0].abs() >= hull.length_m / 2.0 + STATION_AVATAR_RADIUS_M
                    || position[2].abs() >= hull.width_m / 2.0 + STATION_AVATAR_RADIUS_M)
                    && !obstacles.iter().any(|o| in_obstacle(position, o))
            };
            // Slide along boundaries instead of freezing diagonal movement.
            if clear([x, 0.0, a.position[2]]) {
                a.position[0] = x;
            }
            if clear([a.position[0], 0.0, z]) {
                a.position[2] = z;
            }
        }
        a.updated_at_ms = a.updated_at_ms.max(now);
        a.moving =
            now < expires && (input.forward != 0.0 || input.strafe != 0.0 || input.turn != 0.0);
        if now >= expires {
            p.walk_input = None;
        }
    }
}
pub(super) fn validate(w: &World) -> Result<()> {
    for (name, p) in &w.pilots {
        let fleet = w.accounts.get(name).is_some_and(|a| a.fleet_hangar);
        if let Some(a) = &p.view.station_avatar {
            let l = layout_for(p.view.ship_class, fleet);
            ensure!(
                p.view.location == Location::Docked
                    && a.heading.is_finite()
                    && a.position.iter().all(|v| v.is_finite())
                    && a.position[1] >= 0.0
                    && a.position[1]
                        <= STATION_JUMP_SPEED_MPS.powi(2) / (2.0 * STATION_GRAVITY_MPS2)
                    && (a.jump_started_ms.is_some() || a.position[1] == 0.0)
                    && !(a.seated && a.jump_started_ms.is_some())
                    && a.position[0].abs() <= l.half_length
                    && a.position[2].abs() <= l.half_width
                    && (!fleet || a.onboard_interior || a.position[2] <= 3.0)
                    && (!a.onboard_interior
                        || (p.view.ship_class == ShipClass::Pioneer
                            && (1..=2).contains(&a.interior_deck)
                            && a.position[0] >= 0.3
                            && a.position[0] <= 31.7
                            && a.position[2] >= 0.3
                            && a.position[2] <= 13.7))
                    && !obstacles_for(p.view.ship_class, a.onboard_interior, fleet)
                        .iter()
                        .chain(room_controls::door_obstacle(w, name).as_ref())
                        .any(|o| in_obstacle(a.position, o)),
                "invalid station avatar"
            );
        }
    }
    Ok(())
}

fn obstacles_for(class: ShipClass, interior: bool, fleet: bool) -> Vec<StationObstacle> {
    if interior {
        pioneer_interior_obstacles()
    } else if fleet {
        fleet_station_obstacles()
    } else {
        station_obstacles(class)
    }
}

fn layout_for(class: ShipClass, fleet: bool) -> HangarLayout {
    if fleet {
        let mut layout = fleet_hangar_layout();
        let (center, boarding) = fleet_berth(class);
        layout.ship_center = center;
        layout.boarding_point = boarding;
        layout
    } else {
        hangar_layout(class)
    }
}
