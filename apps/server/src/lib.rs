use anyhow::{bail, ensure, Context, Result};
use arkline_protocol::*;
use fs2::FileExt;
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
struct Pilot {
    view: PilotView,
    token_hash: String,
    last_mine_ms: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
struct World {
    pilots: BTreeMap<String, Pilot>,
    orders: BTreeMap<u64, SellOrder>,
    belt_ore: u64,
    next_order: u64,
}
impl Default for World {
    fn default() -> Self {
        Self {
            pilots: BTreeMap::new(),
            orders: BTreeMap::new(),
            belt_ore: INITIAL_BELT_ORE,
            next_order: 1,
        }
    }
}
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// One connection, protected by the transport's mutex, serializes all writes.
/// The durable state and its economic event commit in one SQLite transaction.
pub struct Server {
    db: Connection,
    online_sessions: BTreeMap<String, usize>,
    // Kept after db so SQLite closes before authority is released. Never unlink
    // the sidecar: replacing its inode would allow competing locks.
    _ownership_lock: Option<std::fs::File>,
}
impl Server {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let ownership_lock = if path == Path::new(":memory:") {
            None
        } else {
            // Resolve directory and existing-file symlinks to share a lock for
            // ordinary alternate spellings of the same database path.
            let resolved = if path.exists() {
                std::fs::canonicalize(path)?
            } else {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                std::fs::canonicalize(parent)?.join(
                    path.file_name()
                        .context("database path requires a filename")?,
                )
            };
            let mut sidecar = resolved.as_os_str().to_os_string();
            sidecar.push(".lock");
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&sidecar)
                .context("cannot open database ownership lock")?;
            file.try_lock_exclusive().context("world database is already owned by another server (or ownership lock is unavailable)")?;
            Some(file)
        };
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS world (id INTEGER PRIMARY KEY CHECK(id=1), json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS ledger (sequence INTEGER PRIMARY KEY, json TEXT NOT NULL);")?;
        db.execute(
            "INSERT OR IGNORE INTO world(id,json) VALUES(1,?1)",
            [serde_json::to_string(&World::default())?],
        )?;
        Ok(Self {
            db,
            online_sessions: BTreeMap::new(),
            _ownership_lock: ownership_lock,
        })
    }
    pub fn handle(&mut self, session: &mut Option<String>, request: Request) -> Response {
        self.handle_at(session, request, now_ms())
    }
    pub fn handle_at(
        &mut self,
        session: &mut Option<String>,
        request: Request,
        now: u64,
    ) -> Response {
        let previous = session.clone();
        match self.execute(session, request, now) {
            Ok(mut response) => {
                // execute changes the session only after a successful durable
                // commit. Repeated Connect on one socket must not double-count.
                if previous != *session {
                    if let Some(previous) = previous {
                        self.remove_presence(&previous);
                    }
                    if let Some(current) = session.as_ref() {
                        *self.online_sessions.entry(current.clone()).or_default() += 1;
                    }
                }
                let online = self.online_sessions.keys().cloned().collect::<Vec<_>>();
                if let Some(snapshot) = &mut response.snapshot {
                    snapshot.online_pilots = online.clone();
                }
                if let Some(export) = &mut response.ledger {
                    export.snapshot.online_pilots = online;
                }
                response
            }
            Err(error) => Response::error(error.to_string()),
        }
    }
    /// Called exactly once by the transport session guard, on all exit paths.
    /// Presence is in-memory only; persistent ships and economy are untouched.
    pub fn disconnect(&mut self, session: &mut Option<String>) {
        if let Some(pilot) = session.take() {
            self.remove_presence(&pilot);
        }
    }
    fn remove_presence(&mut self, pilot: &str) {
        if let Some(count) = self.online_sessions.get_mut(pilot) {
            *count -= 1;
            if *count == 0 {
                self.online_sessions.remove(pilot);
            }
        }
    }
    fn execute(
        &mut self,
        session: &mut Option<String>,
        request: Request,
        now: u64,
    ) -> Result<Response> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let json: String = tx.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        let mut world: World = serde_json::from_str(&json).context("invalid durable world")?;
        for pilot in world.pilots.values_mut() {
            if let Location::InTransit {
                destination,
                arrives_at_ms,
            } = pilot.view.location
            {
                if now >= arrives_at_ms {
                    pilot.view.location = match destination {
                        Destination::Station => Location::Station,
                        Destination::Belt => Location::Belt,
                    };
                }
            }
        }
        let mut token = None;
        let mut event = None;
        let mut connected = None;
        let export = matches!(request, Request::ExportLedger);
        let message;
        match request {
            Request::Connect {
                protocol_version,
                pilot,
                resume_token,
            } => {
                ensure!(
                    protocol_version == PROTOCOL_VERSION,
                    "unsupported protocol version"
                );
                ensure!(
                    !pilot.is_empty()
                        && pilot.len() <= 24
                        && pilot
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
                    "pilot name must be 1-24 ASCII letters, digits, underscore or hyphen"
                );
                if let Some(existing) = world.pilots.get(&pilot) {
                    let supplied =
                        resume_token.context("resume token required for existing pilot")?;
                    ensure!(
                        token_hash(&supplied) == existing.token_hash,
                        "invalid resume token"
                    );
                } else {
                    ensure!(
                        resume_token.is_none(),
                        "unknown pilot; omit resume token to create"
                    );
                    let mut bytes = [0u8; 32];
                    OsRng
                        .try_fill_bytes(&mut bytes)
                        .context("secure random generator unavailable")?;
                    let issued = hex::encode(bytes);
                    world.pilots.insert(
                        pilot.clone(),
                        Pilot {
                            view: PilotView {
                                name: pilot.clone(),
                                credits: STARTER_CREDITS,
                                ore: 0,
                                location: Location::Docked,
                            },
                            token_hash: token_hash(&issued),
                            last_mine_ms: None,
                        },
                    );
                    token = Some(issued);
                    event = Some(EconomicEvent::PilotCreated {
                        pilot: pilot.clone(),
                    });
                }
                connected = Some(pilot);
                message = "Connected";
            }
            Request::Snapshot | Request::ExportLedger => {
                message = "Snapshot";
            }
            request => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?
                    .clone();
                let pilot = world.pilots.get_mut(&name).context("pilot missing")?;
                match request {
                    Request::Undock => {
                        ensure!(pilot.view.location == Location::Docked, "must be docked");
                        pilot.view.location = Location::Station;
                        message = "Undocked";
                    }
                    Request::Warp { destination } => {
                        ensure!(
                            matches!(pilot.view.location, Location::Station | Location::Belt),
                            "must be in space and not in transit"
                        );
                        ensure!(
                            !matches!(
                                (&pilot.view.location, destination),
                                (Location::Station, Destination::Station)
                                    | (Location::Belt, Destination::Belt)
                            ),
                            "already at destination"
                        );
                        pilot.view.location = Location::InTransit {
                            destination,
                            arrives_at_ms: now
                                .checked_add(WARP_DURATION_MS)
                                .context("clock overflow")?,
                        };
                        message = "Warp initiated";
                    }
                    Request::Dock => {
                        ensure!(
                            pilot.view.location == Location::Station,
                            "must be at station"
                        );
                        pilot.view.location = Location::Docked;
                        message = "Docked";
                    }
                    Request::Mine { units } => {
                        ensure!(pilot.view.location == Location::Belt, "must be at belt");
                        ensure!(
                            (1..=MAX_MINE_UNITS).contains(&units),
                            "mine quantity must be 1-100"
                        );
                        ensure!(
                            pilot
                                .last_mine_ms
                                .is_none_or(|last| now >= last.saturating_add(1_000)),
                            "mining cooldown: wait one second"
                        );
                        ensure!(world.belt_ore >= units, "insufficient belt ore");
                        pilot.view.ore = add_cargo(pilot.view.ore, units)?;
                        world.belt_ore -= units;
                        pilot.last_mine_ms = Some(now);
                        event = Some(EconomicEvent::OreMined { pilot: name, units });
                        message = "Ore mined";
                    }
                    Request::PlaceSell { units, unit_price } => {
                        ensure!(pilot.view.location == Location::Docked, "must be docked");
                        ensure!(
                            units > 0 && unit_price > 0,
                            "quantity and price must be positive"
                        );
                        units.checked_mul(unit_price).context("price overflow")?;
                        ensure!(pilot.view.ore >= units, "insufficient ore");
                        pilot.view.ore -= units;
                        let id = world.next_order;
                        world.next_order = id.checked_add(1).context("order id overflow")?;
                        world.orders.insert(
                            id,
                            SellOrder {
                                id,
                                seller: name.clone(),
                                units,
                                unit_price,
                            },
                        );
                        event = Some(EconomicEvent::SellPlaced {
                            order_id: id,
                            seller: name,
                            units,
                            unit_price,
                        });
                        message = "Sell order placed";
                    }
                    Request::Buy { order_id, units } => {
                        ensure!(pilot.view.location == Location::Docked, "must be docked");
                        let order = world
                            .orders
                            .get(&order_id)
                            .context("order not found")?
                            .clone();
                        ensure!(order.seller != name, "cannot buy own order");
                        ensure!(units > 0 && units <= order.units, "invalid buy quantity");
                        let price = units
                            .checked_mul(order.unit_price)
                            .context("price overflow")?;
                        ensure!(pilot.view.credits >= price, "insufficient credits");
                        pilot.view.ore = add_cargo(pilot.view.ore, units)?;
                        pilot.view.credits -= price;
                        let seller = world
                            .pilots
                            .get_mut(&order.seller)
                            .context("seller missing")?;
                        seller.view.credits = seller
                            .view
                            .credits
                            .checked_add(price)
                            .context("balance overflow")?;
                        if order.units == units {
                            world.orders.remove(&order_id);
                        } else {
                            world.orders.get_mut(&order_id).unwrap().units -= units;
                        }
                        event = Some(EconomicEvent::TradeExecuted {
                            order_id,
                            buyer: name,
                            units,
                        });
                        message = "Trade committed";
                    }
                    Request::CancelSell { order_id } => {
                        ensure!(pilot.view.location == Location::Docked, "must be docked");
                        let order = world.orders.get(&order_id).context("order not found")?;
                        ensure!(order.seller == name, "only seller can cancel");
                        pilot.view.ore = add_cargo(pilot.view.ore, order.units)?;
                        world.orders.remove(&order_id);
                        event = Some(EconomicEvent::SellCancelled {
                            order_id,
                            seller: name,
                        });
                        message = "Sell order cancelled";
                    }
                    _ => unreachable!(),
                }
            }
        }
        validate(&world)?;
        let mut entries = {
            let query = if export {
                "SELECT json FROM ledger ORDER BY sequence"
            } else {
                "SELECT json FROM ledger ORDER BY sequence DESC LIMIT 1"
            };
            let mut statement = tx.prepare(query)?;
            let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
            let mut entries = Vec::new();
            for row in rows {
                entries.push(serde_json::from_str::<LedgerEntry>(&row?)?);
            }
            entries
        };
        if let Some(event) = event {
            let last = entries.last();
            let mut entry = LedgerEntry {
                sequence: last
                    .map_or(0, |e| e.sequence)
                    .checked_add(1)
                    .context("ledger sequence overflow")?,
                ruleset_version: RULESET_VERSION,
                timestamp_ms: now.max(last.map_or(0, |e| e.timestamp_ms)),
                previous_hash: last.map_or(GENESIS_HASH, |e| e.hash.as_str()).into(),
                event,
                hash: String::new(),
            };
            entry.hash = entry.calculate_hash();
            tx.execute(
                "INSERT INTO ledger(sequence,json) VALUES(?1,?2)",
                params![
                    i64::try_from(entry.sequence)?,
                    serde_json::to_string(&entry)?
                ],
            )?;
            entries.push(entry);
        }
        // Snapshot polling must not rewrite and fsync the entire world unless
        // it resolves a due arrival. Economic updates still commit with their
        // ledger entry in this same transaction.
        let updated_json = serde_json::to_string(&world)?;
        if updated_json != json {
            tx.execute("UPDATE world SET json=?1 WHERE id=1", [updated_json])?;
        }
        let snapshot = Snapshot {
            server_time_ms: now,
            online_pilots: self.online_sessions.keys().cloned().collect(),
            pilots: world.pilots.values().map(|p| p.view.clone()).collect(),
            orders: world.orders.values().cloned().collect(),
            belt_ore: world.belt_ore,
            ledger_sequence: entries.last().map_or(0, |e| e.sequence),
            ledger_hash: entries
                .last()
                .map_or(GENESIS_HASH, |e| e.hash.as_str())
                .into(),
        };
        tx.commit()?;
        if let Some(pilot) = connected {
            *session = Some(pilot);
        }
        Ok(Response {
            ok: true,
            message: message.into(),
            resume_token: token,
            snapshot: Some(snapshot.clone()),
            ledger: if export {
                Some(LedgerExport {
                    protocol_version: PROTOCOL_VERSION,
                    entries,
                    snapshot,
                })
            } else {
                None
            },
        })
    }
}
fn add_cargo(current: u64, units: u64) -> Result<u64> {
    let next = current.checked_add(units).context("cargo overflow")?;
    ensure!(next <= CARGO_CAPACITY, "cargo capacity exceeded");
    Ok(next)
}
fn validate(world: &World) -> Result<()> {
    let credits: u128 = world.pilots.values().map(|p| p.view.credits as u128).sum();
    let ore = world.belt_ore as u128
        + world
            .pilots
            .values()
            .map(|p| p.view.ore as u128)
            .sum::<u128>()
        + world.orders.values().map(|o| o.units as u128).sum::<u128>();
    if credits != world.pilots.len() as u128 * STARTER_CREDITS as u128
        || ore != INITIAL_BELT_ORE as u128
    {
        bail!("economic conservation invariant failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_writes_only_when_a_persisted_arrival_changes_state() {
        let mut server = Server::open(":memory:").unwrap();
        let mut session = None;
        let connected = server.handle_at(
            &mut session,
            Request::Connect {
                protocol_version: PROTOCOL_VERSION,
                pilot: "Traveler".into(),
                resume_token: None,
            },
            100,
        );
        assert!(connected.ok);
        assert!(server.handle_at(&mut session, Request::Undock, 101).ok);
        assert!(
            server
                .handle_at(
                    &mut session,
                    Request::Warp {
                        destination: Destination::Belt
                    },
                    102
                )
                .ok
        );
        let before = server.db.total_changes();
        for time in [103, 1000, WARP_DURATION_MS + 101] {
            assert!(server.handle_at(&mut None, Request::Snapshot, time).ok);
            assert!(server.handle_at(&mut None, Request::ExportLedger, time).ok);
        }
        assert_eq!(
            server.db.total_changes(),
            before,
            "polling unchanged state must not write"
        );
        let arrived = server.handle_at(&mut None, Request::Snapshot, WARP_DURATION_MS + 102);
        assert_eq!(arrived.snapshot.unwrap().pilots[0].location, Location::Belt);
        assert_eq!(
            server.db.total_changes(),
            before + 1,
            "arrival must persist once"
        );
        assert!(
            server
                .handle_at(&mut None, Request::Snapshot, WARP_DURATION_MS + 103)
                .ok
        );
        assert_eq!(server.db.total_changes(), before + 1);
    }
}
