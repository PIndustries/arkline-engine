//! In-process editor authority. No transport request can control this clock.
use crate::*;

pub const EDITOR_TICK_MS: u64 = 50;
pub const MAX_EDITOR_ADVANCE_MS: u64 = 250;

/// Owns a local-development world and always reopens paused. Play time comes only
/// from explicit bounded deltas, never elapsed wall time. Server is deliberately
/// private: callers cannot accidentally invoke its wall-clock tick/handle APIs.
pub struct EditorWorld {
    server: Server,
    paused: bool,
    remainder_ms: u64,
}
impl EditorWorld {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let path = local_development::database_path(directory.as_ref())?;
        let server = Server::open_realm(path, true, true)?;
        Ok(Self {
            server,
            paused: true,
            remainder_ms: 0,
        })
    }
    pub fn play(&mut self) {
        self.paused = false;
    }
    pub fn pause(&mut self) {
        self.paused = true;
        self.remainder_ms = 0;
    }
    pub fn is_paused(&self) -> bool {
        self.paused
    }
    pub fn time(&self) -> u64 {
        self.server.editor_time.expect("editor clock initialized")
    }
    /// At most five fixed ticks per call; excess frame delay is discarded rather
    /// than replaying a backlog that would freeze the editor after a stall.
    pub fn advance(&mut self, delta_ms: u64) -> Result<()> {
        if !self.paused {
            self.run_ticks(delta_ms)?;
        }
        Ok(())
    }
    /// Explicit single-step while paused, with the same fixed-tick/cost bound.
    pub fn step(&mut self, delta_ms: u64) -> Result<()> {
        ensure!(self.paused, "pause the editor before stepping");
        self.run_ticks(delta_ms)
    }
    fn run_ticks(&mut self, delta_ms: u64) -> Result<()> {
        self.remainder_ms += delta_ms.min(MAX_EDITOR_ADVANCE_MS);
        while self.remainder_ms >= EDITOR_TICK_MS {
            let next = self
                .time()
                .checked_add(EDITOR_TICK_MS)
                .context("editor clock exhausted")?;
            if let Err(error) = self.server.tick_at(next) {
                self.pause();
                return Err(error);
            }
            self.remainder_ms -= EDITOR_TICK_MS;
        }
        Ok(())
    }
    pub fn handle(&mut self, session: &mut Option<String>, request: Request) -> Response {
        self.server.handle_at(session, request, self.time())
    }
    pub fn disconnect(&mut self, session: &mut Option<String>) {
        self.server.disconnect(session);
    }
}

/// Read before startup advancement: paused reopen must never recharge reactors
/// or progress room systems by the time elapsed on the host clock.
pub(super) fn open_clock(db: &mut Connection, editor: bool) -> Result<Option<u64>> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='editor_clock')",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        editor || !exists,
        "editor-clock world requires the embedded editor"
    );
    if !editor {
        return Ok(None);
    }
    if exists {
        let time: String =
            db.query_row("SELECT time_ms FROM editor_clock WHERE id=1", [], |r| {
                r.get(0)
            })?;
        return Ok(Some(
            time.parse().context("invalid persisted editor clock")?,
        ));
    }
    // Adoption of an existing local world happens once; subsequent opens use only
    // this value. The existing world remains subject to normal restart cleanup.
    let time = now_ms();
    let tx = db.transaction()?;
    tx.execute_batch(
        "CREATE TABLE editor_clock(id INTEGER PRIMARY KEY CHECK(id=1),time_ms TEXT NOT NULL)",
    )?;
    tx.execute(
        "INSERT INTO editor_clock(id,time_ms) VALUES(1,?1)",
        [time.to_string()],
    )?;
    tx.commit()?;
    Ok(Some(time))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("arkline-editor-clock-{}", rand::random::<u64>()))
    }
    fn connect(token: Option<String>) -> Request {
        serde_json::from_value(serde_json::json!({"command":"connect","protocol_version":PROTOCOL_VERSION,"pilot":"EditorPilot","resume_token":token})).unwrap()
    }
    fn call(w: &mut EditorWorld, s: &mut Option<String>, r: Request) -> Response {
        let response = w.handle(s, r);
        assert!(response.ok, "{}", response.message);
        response
    }
    fn snapshot(w: &mut EditorWorld) -> Snapshot {
        call(w, &mut None, Request::Snapshot).snapshot.unwrap()
    }
    #[test]
    fn paused_movement_and_mining_freeze_and_resume_only_explicit_ticks() {
        let path = dir();
        let mut w = EditorWorld::open(&path).unwrap();
        let mut session = None;
        call(&mut w, &mut session, connect(None));
        call(&mut w, &mut session, Request::Undock);
        call(
            &mut w,
            &mut session,
            Request::Navigate {
                mode: NavigationMode::FlyTo {
                    position: [5000.0, 0.0, 1000.0],
                },
            },
        );
        let start = snapshot(&mut w);
        w.advance(u64::MAX).unwrap();
        assert_eq!(
            serde_json::to_value(&start).unwrap(),
            serde_json::to_value(snapshot(&mut w)).unwrap()
        );
        let time = w.time();
        w.step(250).unwrap();
        assert_eq!(w.time(), time + 250);
        assert_ne!(
            snapshot(&mut w).pilots[0].flight.as_ref().unwrap().position,
            start.pilots[0].flight.as_ref().unwrap().position
        );
        // Let the ordinary autonomous controller travel, target and start miners.
        call(
            &mut w,
            &mut session,
            Request::StartResourceJob {
                pilot: None,
                target_units: 100,
            },
        );
        w.play();
        for _ in 0..2400 {
            w.advance(250).unwrap();
            if snapshot(&mut w).pilots[0]
                .mining_modules
                .iter()
                .any(|m| m.target_id.is_some())
            {
                break;
            }
        }
        let before = snapshot(&mut w);
        assert!(before.pilots[0]
            .mining_modules
            .iter()
            .any(|m| m.target_id.is_some()));
        w.pause();
        let time = w.time();
        std::thread::sleep(std::time::Duration::from_millis(60));
        w.advance(u64::MAX).unwrap();
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(snapshot(&mut w)).unwrap()
        );
        assert_eq!(time, w.time());
        w.play();
        w.advance(u64::MAX).unwrap();
        assert_eq!(w.time(), time + 250);
        for _ in 0..28 {
            w.advance(250).unwrap();
        }
        assert!(snapshot(&mut w).pilots[0].ore > before.pilots[0].ore);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn restart_preserves_clock_and_charge_and_cannot_open_as_network_world() {
        let path = dir();
        let mut w = EditorWorld::open(&path).unwrap();
        let mut session = None;
        let token = call(&mut w, &mut session, connect(None))
            .resume_token
            .unwrap();
        call(&mut w, &mut session, Request::Undock);
        call(
            &mut w,
            &mut session,
            Request::Warp {
                destination: Destination::Belt,
            },
        );
        w.step(250).unwrap();
        let before = snapshot(&mut w);
        let time = w.time();
        drop(w);
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(Server::open_local_development(&path).is_err());
        let mut w = EditorWorld::open(&path).unwrap();
        assert!(w.is_paused());
        assert_eq!(w.time(), time);
        let after = snapshot(&mut w);
        assert_eq!(before.pilots[0].power, after.pilots[0].power);
        assert_eq!(before.pilots[0].flight, after.pilots[0].flight);
        assert!(!w.handle(&mut None, connect(None)).ok);
        call(&mut w, &mut session, connect(Some(token)));
        w.play();
        assert!(w.step(50).is_err());
        w.advance(49).unwrap();
        assert_eq!(w.time(), time);
        w.advance(1).unwrap();
        assert_eq!(w.time(), time + 50);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn failed_clock_commit_rolls_back_world_and_pauses_without_backlog() {
        let path = dir();
        let mut w = EditorWorld::open(&path).unwrap();
        let mut session = None;
        call(&mut w, &mut session, connect(None));
        call(&mut w, &mut session, Request::Undock);
        call(
            &mut w,
            &mut session,
            Request::Navigate {
                mode: NavigationMode::FlyTo {
                    position: [5000.0, 0.0, 1000.0],
                },
            },
        );
        let before = snapshot(&mut w);
        let time = w.time();
        w.server.db.execute_batch("CREATE TRIGGER reject_clock BEFORE UPDATE ON editor_clock BEGIN SELECT RAISE(ABORT, 'test clock write failure'); END;").unwrap();
        w.play();
        assert!(w.advance(250).is_err());
        assert!(w.is_paused());
        assert_eq!(w.time(), time);
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(snapshot(&mut w)).unwrap()
        );
        w.server
            .db
            .execute_batch("DROP TRIGGER reject_clock")
            .unwrap();
        w.play();
        w.advance(50).unwrap();
        assert_eq!(w.time(), time + 50);
        drop(w);
        let w = EditorWorld::open(&path).unwrap();
        assert_eq!(w.time(), time + 50);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
}
