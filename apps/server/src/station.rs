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
    let p = w.pilots.get_mut(name).context("pilot missing")?;
    ensure!(
        p.view.location == Location::Docked,
        "station walking requires docking"
    );
    let layout = hangar_layout(p.view.ship_class);
    match r {
        Request::DisembarkShip => {
            ensure!(p.view.station_avatar.is_none(), "already on foot");
            p.controls = None;
            p.view.station_avatar = Some(StationAvatar {
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
            ensure!(a.jump_started_ms.is_none(), "land before using seating");
            if seated {
                let seat = quarters_layout(p.view.ship_class).seat_point;
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
    for p in w.pilots.values_mut() {
        if let Some(a) = &mut p.view.station_avatar {
            if station_obstacles(p.view.ship_class)
                .iter()
                .any(|o| in_obstacle(a.position, o))
            {
                a.position = hangar_layout(p.view.ship_class).boarding_point;
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
    for p in w.pilots.values_mut() {
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
        let layout = hangar_layout(p.view.ship_class);
        let hull = p.view.ship_class.spec();
        let obstacles = station_obstacles(p.view.ship_class);
        let norm = input.forward.hypot(input.strafe).max(1.0);
        while remaining > 0.0 {
            let dt = remaining.min(0.025);
            remaining -= dt;
            a.heading = (a.heading + input.turn * 2.0 * dt).rem_euclid(std::f64::consts::TAU);
            let (sin, cos) = a.heading.sin_cos();
            let x = (a.position[0]
                + (cos * input.forward - sin * input.strafe) / norm * WALK_SPEED_MPS * dt)
                .clamp(-layout.half_length + 0.3, layout.half_length - 0.3);
            let z = (a.position[2]
                + (sin * input.forward + cos * input.strafe) / norm * WALK_SPEED_MPS * dt)
                .clamp(-layout.half_width + 0.3, layout.half_width - 0.3);
            let clear = |position: [f64; 3]| {
                (position[0].abs() >= hull.length_m / 2.0 + STATION_AVATAR_RADIUS_M
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
    for p in w.pilots.values() {
        if let Some(a) = &p.view.station_avatar {
            let l = hangar_layout(p.view.ship_class);
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
                    && !station_obstacles(p.view.ship_class)
                        .iter()
                        .any(|o| in_obstacle(a.position, o)),
                "invalid station avatar"
            );
        }
    }
    Ok(())
}
