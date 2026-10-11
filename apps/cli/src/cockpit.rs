use crate::transport::Login;
use anyhow::{bail, Context, Result};
use arkline_protocol::{
    Destination, DroneClass, NavigationMode, NavigationTarget, Request, Response,
};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::{
    io,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

enum Command {
    Send(Request),
    Reconnect,
}
enum Update {
    Connected(Response),
    Response(Response, bool),
    Failed(String),
}
fn worker(login: Login, rx: mpsc::Receiver<Command>, tx: mpsc::Sender<Update>) {
    let mut connection = None;
    let mut reconnect = true;
    loop {
        if reconnect {
            reconnect = false;
            match login.connect() {
                Ok((t, r)) => {
                    connection = Some(t);
                    if tx.send(Update::Connected(r)).is_err() {
                        return;
                    }
                }
                Err(e) => {
                    if tx
                        .send(Update::Failed(format!(
                            "Connection failed: {e}. Press R to resume."
                        )))
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
        let (request, mutation) = match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(Command::Reconnect) => {
                connection = None;
                reconnect = true;
                continue;
            }
            Ok(Command::Send(r)) => (r, true),
            Err(mpsc::RecvTimeoutError::Timeout) if connection.is_some() => {
                (Request::Snapshot, false)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return,
        };
        if let Some(t) = connection.as_mut() {
            match t.request(&request) {
                Ok(mut r) => {
                    r.resume_token = None;
                    if tx.send(Update::Response(r, mutation)).is_err() {
                        return;
                    }
                }
                Err(_) => {
                    connection = None;
                    // Drop queued actions. An uncertain mutation is never replayed.
                    while rx.try_recv().is_ok() {}
                    if tx.send(Update::Failed("Connection lost; last action outcome may be unknown. No actions replayed. R resumes and refreshes state.".into())).is_err() { return; }
                }
            }
        }
    }
}
struct ScreenGuard;
impl Drop for ScreenGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

/// Commands always map to existing server authority; the server validates prerequisites.
fn command(text: &str, selected: Option<u64>) -> Result<Request> {
    let words: Vec<_> = text.split_whitespace().collect();
    let n = |i: usize| -> Result<u64> {
        words
            .get(i)
            .context("missing integer")?
            .parse()
            .context("invalid integer")
    };
    let target = || selected.context("select an asteroid with Up/Down");
    let decimal = |i: usize| -> Result<f64> {
        let value: f64 = words
            .get(i)
            .context("missing number")?
            .parse()
            .context("invalid number")?;
        if !value.is_finite() {
            bail!("number must be finite");
        }
        Ok(value)
    };
    let name =
        |i: usize| -> Result<String> { Ok(words.get(i).context("missing name")?.to_string()) };

    Ok(match words.first().copied().unwrap_or("") {
        "undock" => Request::Undock,
        "dock" => Request::Dock,
        "belt" => Request::Warp {
            destination: Destination::Belt,
        },
        "station" => Request::Warp {
            destination: Destination::Station,
        },
        "stop" => Request::Navigate {
            mode: NavigationMode::Stop,
        },
        "approach" => Request::Navigate {
            mode: NavigationMode::Approach {
                target: NavigationTarget::Asteroid { id: target()? },
            },
        },
        "lock" => Request::LockAsteroid {
            id: words
                .get(1)
                .map(|_| n(1))
                .transpose()?
                .or(selected)
                .context("select an asteroid")?,
        },
        "unlock" => Request::UnlockAsteroid { id: target()? },
        "orbit" => Request::Navigate {
            mode: NavigationMode::Orbit {
                target: NavigationTarget::Asteroid { id: target()? },
                radius: decimal(1)?,
            },
        },
        "range" => Request::Navigate {
            mode: NavigationMode::KeepRange {
                target: NavigationTarget::Asteroid { id: target()? },
                distance: decimal(1)?,
            },
        },
        "fly" => Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: [decimal(1)?, decimal(2)?, decimal(3)?],
            },
        },
        "corp-create" => Request::CreateCorporation { name: name(1)? },
        "corp-invite" => Request::InviteCorporation { pilot: name(1)? },
        "corp-accept" => Request::AcceptCorporation {
            corporation: name(1)?,
        },
        "corp-deposit" => Request::DepositCorporationOre { units: n(1)? },
        "corp-withdraw" => Request::WithdrawCorporationOre { units: n(1)? },
        "drone-orbit" => Request::OrbitDrone {
            id: n(1)?,
            target: NavigationTarget::Asteroid { id: target()? },
        },
        "drone-attack" => Request::AssignAttackDrone {
            id: n(1)?,
            target_pilot: name(2)?,
            target_drone_id: n(3)?,
        },
        "mine" => Request::StartMining {
            module_id: u8::try_from(n(1)?).context("invalid module")?,
            target_id: target()?,
        },
        "stop-mining" => Request::StopMining {
            module_id: u8::try_from(n(1)?).context("invalid module")?,
        },
        "deposit" => Request::DepositOre { units: n(1)? },
        "withdraw" => Request::WithdrawOre { units: n(1)? },
        "launch" => Request::LaunchDroneClass {
            class: match words.get(1).copied() {
                Some("recon") => DroneClass::Recon,
                Some("mining") => DroneClass::Mining,
                Some("attack") => DroneClass::Attack,
                _ => bail!("launch recon|mining|attack"),
            },
        },
        "recall" => Request::RecallDroneById { id: n(1)? },
        "hold" => Request::DroneHoldById { id: n(1)? },
        "drone-mine" => Request::AssignMiningDrone {
            id: n(1)?,
            target_id: target()?,
        },
        "gather" => Request::StartResourceJob {
            target_units: n(1)?,
            pilot: None,
        },
        "cancel-job" => Request::CancelResourceJob { pilot: None },
        "gate" => Request::Warp {
            destination: Destination::Gate { id: n(1)? },
        },
        "jump" => Request::Jump { gate_id: n(1)? },
        "sell" => Request::PlaceSell {
            units: n(1)?,
            unit_price: n(2)?,
        },
        "buy" => Request::Buy {
            order_id: n(1)?,
            units: n(2)?,
        },
        "cancel-order" => Request::CancelSell { order_id: n(1)? },
        "leave-ship" => Request::DisembarkShip,
        "enter-ship" => Request::EnterShip,
        "jump-foot" => Request::StationJump,
        "board" => Request::SelectShip {
            ship_class: serde_json::from_value(serde_json::Value::String(
                words.get(1).context("board HULL")?.to_string(),
            ))?,
        },
        _ => bail!("unknown command; press ? for help"),
    })
}
const HELP: &str = "Up/Down select local asteroid • Tab page • : command • R reconnect • q quit\nu undock • b warp belt • s warp station • d dock • a approach • l lock • 1/2 start miner • !/@ stop miner • Space brake\nCommands: deposit N, withdraw N, board HULL, launch recon|mining|attack, recall ID, hold ID, drone-mine ID, gather N, cancel-job, gate GATE_ID, jump GATE_ID, unlock\nMarket: sell UNITS PRICE, buy ORDER_ID UNITS, cancel-order ID\nFlight: orbit METERS, range METERS, fly X Y Z\nDrones: drone-orbit ID, drone-attack ID PILOT TARGET_DRONE\nCorporation: corp-create NAME, corp-invite PILOT, corp-accept NAME, corp-deposit N, corp-withdraw N\nStation: leave-ship, enter-ship, jump-foot • PageUp/PageDown scroll\nMining needs a confirmed lock. Commands remain pending until a server response. Quit disconnects; server stops manual input/miners. No client-predicted resource changes.";

// Server strings are untrusted terminal content, including custom-world messages.
fn terminal_text(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect()
}

pub fn run(endpoint: String, pilot: String, path: Option<PathBuf>) -> Result<()> {
    if !std::io::IsTerminal::is_terminal(&io::stdin()) {
        bail!("tui requires an interactive terminal; use status for JSON");
    }
    let login = Login::new(endpoint.clone(), pilot.clone(), path)?;
    let (commands, rx) = mpsc::sync_channel::<Command>(1);
    // A bounded UI queue plus a single pending mutation prevents accidental repeated actions.
    let (updates, incoming) = mpsc::channel();
    std::thread::spawn(move || worker(login, rx, updates));
    enable_raw_mode()?;
    let _guard = ScreenGuard;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut response: Option<Response> = None;
    let mut connected = false;
    let mut pending = false;
    let mut message = "Connecting…".to_string();
    let mut last = Instant::now();
    let mut selection = 0usize;
    let mut page = 0;
    let mut scroll = 0u16;
    let mut help = false;
    let mut input: Option<String> = None;
    loop {
        while let Ok(update) = incoming.try_recv() {
            match update {
                Update::Connected(r) => {
                    connected = true;
                    pending = false;
                    message = "Connected; authoritative state refreshed.".into();
                    response = Some(r);
                    last = Instant::now();
                }
                Update::Response(r, mutation) => {
                    if mutation || !r.ok {
                        message = r.message.clone();
                    }
                    if mutation {
                        pending = false;
                    }
                    response = Some(r);
                    last = Instant::now();
                }
                Update::Failed(error) => {
                    connected = false;
                    pending = false;
                    message = error;
                }
            }
        }
        let snapshot = response.as_ref().and_then(|r| r.snapshot.as_ref());
        let own = snapshot.and_then(|s| s.pilots.iter().find(|p| p.name == pilot));
        let rocks: Vec<_> = snapshot
            .into_iter()
            .flat_map(|s| &s.asteroids)
            .filter(|a| own.is_some_and(|p| p.system_id == a.system_id))
            .collect();
        selection = selection.min(rocks.len().saturating_sub(1));
        let selected = rocks.get(selection).map(|a| a.id);
        let status = if !connected {
            "OFFLINE"
        } else if last.elapsed() > Duration::from_secs(2) {
            "STALE"
        } else {
            "LIVE"
        };
        terminal.draw(|f| {
            let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(5), Constraint::Length(5)]).split(f.area());
            f.render_widget(Paragraph::new(format!("ARKLINE {}  {pilot}  [{status}] {}\n{endpoint} | Tab: overview / inventory / fleet / market / navigation | ?: help", arkline_protocol::BUILD_VERSION.trim(), if pending { "ACTION PENDING" } else { "" })).style(Style::default().fg(Color::Green)).block(Block::default().borders(Borders::ALL)), layout[0]);
            let body = if help { HELP.into() } else { match (page, own) {
                (0, Some(p)) => {
                    let mut text = format!("{:?} · system {} · {:?}\nCargo {} / {} · credits {}\n", p.ship_class,p.system_id,p.location,p.ore,p.ship_class.spec().cargo_capacity,p.credits);
                    if let Some(power) = &p.power { text.push_str(&format!("Power {} / {} J · regeneration {} W\n",power.charge_j,power.capacity_j,power.regen_w)); }
                    if let Some(flight) = &p.flight { text.push_str(&format!("Speed {:.1} m/s\n",flight.velocity.iter().map(|v|v*v).sum::<f64>().sqrt())); }
                    text.push_str(&format!("Locked {:?} · acquiring {}\n", p.locked_asteroids,p.target_locks.len()));
                    for m in &p.mining_modules { text.push_str(&format!("Miner {} → {} · {}\n",m.module_id+1,m.target_id.map(|v|format!("asteroid #{v}")).unwrap_or_else(||"idle".into()),m.stop_reason.as_deref().unwrap_or("ready"))); }
                    text.push_str("\nLOCAL ASTEROIDS — select then approach, lock, mine\n");
                    for (i,a) in rocks.iter().enumerate().skip(selection.saturating_sub(5)).take(12) {
                        let distance=p.flight.as_ref().map(|f|f.position.iter().zip(a.position).map(|(x,y)|(x-y).powi(2)).sum::<f64>().sqrt()).unwrap_or(0.0);
                        text.push_str(&format!("{} #{} {} — {} ore · {:.1} km\n",if i==selection { ">" } else { " " },a.id,a.name,a.ore,distance/1000.0));
                    } text
                },
                (1, _) => response.as_ref().and_then(|r| r.assets.as_ref()).map(|a| {
                    let mut t="PERSONAL STATION STORAGE\n".to_string();
                    for v in &a.storage { t.push_str(&format!("System {}: {} ore\n",v.system_id,v.ore)); }
                    t.push_str("\nHANGAR\n");
                    for v in &a.ships {t.push_str(&format!("{:?} · system {} · {} ore aboard\n",v.ship_class,v.system_id,v.ore));}
                    if let Some(c)=&a.corporation {t.push_str(&format!("\nCORPORATION {} · owner {}\nMembers: {}\n",c.name,c.owner,c.members.join(", "))); for v in &c.storage {t.push_str(&format!("System {}: {} ore\n",v.system_id,v.ore));}}
                    t.push_str("\n:deposit N · :withdraw N · :board HULL"); t
                }).unwrap_or_else(|| "Inventory unavailable until authenticated".into()),
                (2, Some(p)) => {
                    let mut t="DRONES\n".to_string();
                    for d in &p.drones {t.push_str(&format!("#{} {:?} · health {} · signal {:.0}% · {:?}\n",d.id,d.class,d.hit_points,d.signal_strength*100.0,d.mode));}
                    t.push_str("\nRESOURCE JOBS\n");
                    if let Some(s)=snapshot {for j in s.resource_jobs.iter().filter(|j|j.pilot==pilot) {t.push_str(&format!("#{} {:?}: stored {} / {} · {}\n",j.id,j.phase,j.stored_units,j.target_units,j.message));}}
                    t
                },
                (3, _) => {
                    let mut t="MARKET — server-confirmed orders\nID · seller · remaining ore · credits/unit\n".to_string();
                    if let Some(s)=snapshot {for o in &s.orders {t.push_str(&format!("#{} · {} · {} · {}\n",o.id,o.seller,o.units,o.unit_price));}}
                    t.push_str("\n:sell UNITS PRICE · :buy ORDER_ID UNITS · :cancel-order ID");t
                },
                (4, Some(p)) => {
                    let mut t="LOCAL STARGATES\n".to_string();
                    for g in arkline_protocol::gates().iter().filter(|g|g.system_id==p.system_id) {t.push_str(&format!("#{} {} → system {}\n",g.id,g.name,g.destination_system_id));}
                    t.push_str("\n:gate ID to warp to a gate; :jump ID when within range.\n:belt / :station warp locally. Arrival takes actual server travel time.");t
                },
                _ => "Waiting for server snapshot…".into(),
            }};
            f.render_widget(Paragraph::new(terminal_text(&body)).wrap(Wrap{trim:false}).scroll((scroll,0)).block(Block::default().title(["Overview","Inventory","Fleet","Market","Navigation"][page]).borders(Borders::ALL)),layout[1]);
            f.render_widget(Paragraph::new(format!("{}\n{}",terminal_text(&message),input.as_ref().map(|s|format!(":{s}█")).unwrap_or_else(||"u undock · b belt · s station · d dock · l lock · 1/2 mine · : command · ? help".into()))).wrap(Wrap{trim:false}).block(Block::default().borders(Borders::ALL)),layout[2]);
        })?;
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            break;
        }
        let mut action = None;
        if let Some(line) = input.as_mut() {
            match key.code {
                KeyCode::Esc => input = None,
                KeyCode::Backspace => {
                    line.pop();
                }
                KeyCode::Char(c) if line.len() < 256 => line.push(c),
                KeyCode::Enter => {
                    action = Some(line.clone());
                    input = None;
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char(':') => input = Some(String::new()),
                KeyCode::Char('?') => help = !help,
                KeyCode::Tab => {
                    page = (page + 1) % 5;
                    scroll = 0;
                }
                KeyCode::PageDown => scroll = scroll.saturating_add(8),
                KeyCode::PageUp => scroll = scroll.saturating_sub(8),
                KeyCode::Up => selection = selection.saturating_sub(1),
                KeyCode::Down => selection = (selection + 1).min(rocks.len().saturating_sub(1)),
                KeyCode::Char('R') if !connected => {
                    let _ = commands.try_send(Command::Reconnect);
                    message = "Resuming saved identity…".into();
                }
                KeyCode::Char(c) => {
                    action = match c {
                        'u' => Some("undock"),
                        'b' => Some("belt"),
                        's' => Some("station"),
                        'd' => Some("dock"),
                        'a' => Some("approach"),
                        'l' => Some("lock"),
                        '1' => Some("mine 0"),
                        '2' => Some("mine 1"),
                        '!' => Some("stop-mining 0"),
                        '@' => Some("stop-mining 1"),
                        ' ' => Some("stop"),
                        _ => None,
                    }
                    .map(str::to_string)
                }
                _ => {}
            }
        }
        if let Some(action) = action {
            if !connected || pending || last.elapsed() > Duration::from_secs(2) {
                message = "Wait for a live connection and the pending response.".into();
                continue;
            }
            match command(&action, selected) {
                Ok(request) => {
                    if commands.try_send(Command::Send(request)).is_ok() {
                        pending = true;
                        message = format!("Sent: {action}");
                    }
                }
                Err(error) => message = error.to_string(),
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mining_uses_selected_target_and_fixed_module() {
        assert!(matches!(
            command("mine 1", Some(9)).unwrap(),
            Request::StartMining {
                module_id: 1,
                target_id: 9
            }
        ));
        assert!(command("mine 999", Some(9)).is_err());
        assert!(command("mine 0", None).is_err());
    }
    #[test]
    fn drone_assignment_does_not_change_other_modules() {
        assert!(matches!(
            command("drone-mine 42", Some(7)).unwrap(),
            Request::AssignMiningDrone {
                id: 42,
                target_id: 7
            }
        ));
    }
}
