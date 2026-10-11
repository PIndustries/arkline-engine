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
                position: layout.boarding_point,
                heading: 0.0,
                moving: false,
                updated_at_ms: now,
            });
        }
        Request::EnterShip => {
            let a = p.view.station_avatar.as_ref().context("already aboard")?;
            ensure!(
                flight::distance(a.position, layout.boarding_point) <= STATION_BOARD_RANGE_M,
                "walk to your ship boarding point first"
            );
            p.view.station_avatar = None;
            p.walk_input = None;
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
            if x.abs() >= hull.length_m / 2.0 + 0.3 || z.abs() >= hull.width_m / 2.0 + 0.3 {
                a.position = [x, 0.0, z];
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
                    && a.position[1] == 0.0
                    && a.position[0].abs() <= l.half_length
                    && a.position[2].abs() <= l.half_width,
                "invalid station avatar"
            );
        }
    }
    Ok(())
}
