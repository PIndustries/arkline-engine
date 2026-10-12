//! Bounded local graph execution through the ordinary authoritative command path.
use crate::*;
use std::{collections::VecDeque, time::Instant};
const MAX_PENDING: usize = 256;
const MAX_TRACE: usize = 256;
const RUN_BUDGET: usize = 64;
#[derive(Clone)]
struct Pending {
    node: u64,
    due: u64,
    order: u64,
    cause: Option<u64>,
}
#[derive(Default)]
pub(super) struct GraphRuntime {
    graph: EditorEventGraph,
    revision: u64,
    queue: Vec<Pending>,
    trace: VecDeque<EditorGraphTrace>,
    sequence: u64,
    order: u64,
    breakpoint: Option<u64>,
    skip_breakpoint: bool,
}
impl GraphRuntime {
    pub(super) fn debug(&self, paused: bool) -> EditorGraphDebug {
        let mut pending: Vec<_> = self.queue.iter().collect();
        pending.sort_unstable_by_key(|p| (p.due, p.order));
        EditorGraphDebug {
            revision: self.revision,
            graph: self.graph.clone(),
            pending: self.queue.len(),
            pending_nodes: pending
                .into_iter()
                .map(|p| EditorPendingNode {
                    node_id: p.node,
                    due_at_ms: p.due,
                    cause: p.cause,
                })
                .collect(),
            breakpoint_node: self.breakpoint,
            trace: self.trace.iter().cloned().collect(),
            paused,
        }
    }
    pub(super) fn replace(
        &mut self,
        expected: u64,
        graph: EditorEventGraph,
        clear: bool,
        now: u64,
    ) -> Result<()> {
        ensure!(
            expected == self.revision,
            "graph revision changed; refresh before applying"
        );
        graph.validate().map_err(anyhow::Error::msg)?;
        ensure!(
            clear || self.queue.is_empty(),
            "clear pending graph work before replacing it"
        );
        let revision = self
            .revision
            .checked_add(1)
            .context("graph revision exhausted")?;
        self.queue.clear();
        self.breakpoint = None;
        self.skip_breakpoint = false;
        self.graph = graph;
        self.revision = revision;
        self.record(None, None, now, 0, "graph", "Graph replaced", true);
        Ok(())
    }
    pub(super) fn clear(&mut self, now: u64) {
        self.queue.clear();
        self.breakpoint = None;
        self.skip_breakpoint = false;
        self.record(
            None,
            None,
            now,
            0,
            "clear",
            "Pending graph work cleared",
            true,
        );
    }
    pub(super) fn resume(&mut self) {
        self.skip_breakpoint = self.breakpoint.is_some();
    }
    // Explicit trace fields keep causal IDs, simulation time and host duration distinct.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record(
        &mut self,
        cause: Option<u64>,
        node: Option<u64>,
        now: u64,
        duration: u64,
        kind: &str,
        message: &str,
        ok: bool,
    ) -> u64 {
        self.sequence = self.sequence.saturating_add(1);
        if self.trace.len() == MAX_TRACE {
            self.trace.pop_front();
        }
        self.trace.push_back(EditorGraphTrace {
            graph_revision: self.revision,
            sequence: self.sequence,
            cause,
            node_id: node,
            simulation_time_ms: now,
            host_duration_us: duration,
            kind: kind.into(),
            message: message
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect(),
            ok,
        });
        self.sequence
    }
    fn enqueue(&mut self, node: u64, due: u64, cause: Option<u64>) -> Result<()> {
        ensure!(
            self.queue.len() < MAX_PENDING,
            "graph pending queue limit reached"
        );
        let order = self
            .order
            .checked_add(1)
            .context("graph scheduling sequence exhausted")?;
        self.order = order;
        self.queue.push(Pending {
            node,
            due,
            order,
            cause,
        });
        Ok(())
    }
    pub(super) fn trigger(&mut self, node: u64, now: u64) -> Result<()> {
        ensure!(
            self.graph
                .nodes
                .iter()
                .any(|n| n.id == node && matches!(n.operation, EditorNodeOp::Manual)),
            "select a Manual trigger node"
        );
        ensure!(
            self.queue.len() < MAX_PENDING,
            "graph pending queue limit reached"
        );
        let cause = self.record(
            None,
            Some(node),
            now,
            0,
            "trigger",
            "Manual trigger queued",
            true,
        );
        self.enqueue(node, now, Some(cause))
    }
    pub(super) fn tick(&mut self, now: u64, duration: u64, enabled: bool) -> Result<()> {
        let cause = self.record(
            None,
            None,
            now,
            duration,
            "tick",
            "Authoritative fixed tick committed atomically",
            true,
        );
        if !enabled {
            return Ok(());
        }
        let mut nodes: Vec<_> = self
            .graph
            .nodes
            .iter()
            .filter(|n| matches!(n.operation, EditorNodeOp::Tick))
            .map(|n| n.id)
            .collect();
        nodes.sort_unstable();
        ensure!(
            self.queue.len() + nodes.len() <= MAX_PENDING,
            "graph pending queue limit reached"
        );
        for node in nodes {
            self.enqueue(node, now, Some(cause))?;
        }
        Ok(())
    }
    /// Returns true only when a before-node breakpoint pauses execution.
    pub(super) fn drive(
        &mut self,
        server: &mut Server,
        session: &mut Option<String>,
        now: u64,
        single: bool,
    ) -> Result<bool> {
        ensure!(
            session.is_some(),
            "connect an authenticated local pilot before running graph work"
        );
        for _ in 0..if single { 1 } else { RUN_BUDGET } {
            let next = self
                .queue
                .iter()
                .enumerate()
                .filter(|(_, p)| p.due <= now)
                .min_by_key(|(_, p)| (p.due, p.order))
                .map(|(i, _)| i);
            let Some(index) = next else {
                if single {
                    bail!("no ready graph node; delayed work needs a simulation clock step");
                }
                return Ok(false);
            };
            let pending = self.queue[index].clone();
            let node = self
                .graph
                .nodes
                .iter()
                .find(|n| n.id == pending.node)
                .cloned()
                .context("queued graph node no longer exists")?;
            if node.breakpoint
                && !single
                && !(self.skip_breakpoint && self.breakpoint == Some(node.id))
            {
                if self.breakpoint != Some(node.id) {
                    self.record(
                        pending.cause,
                        Some(node.id),
                        now,
                        0,
                        "breakpoint",
                        "Paused before node; no partial simulation tick",
                        true,
                    );
                }
                self.breakpoint = Some(node.id);
                return Ok(true);
            }
            self.queue.remove(index);
            self.breakpoint = None;
            self.skip_breakpoint = false;
            let started = Instant::now();
            let mut port = EditorGraphPort::Next;
            let mut due = now;
            let (ok, message) = match &node.operation {
                EditorNodeOp::Manual | EditorNodeOp::Tick => (true, "Trigger executed".into()),
                EditorNodeOp::Delay { ms } => {
                    due = now.checked_add(*ms).context("graph delay time exhausted")?;
                    (
                        true,
                        format!("Continuation scheduled at simulation {due} ms"),
                    )
                }
                EditorNodeOp::Trace { message } => (true, message.clone()),
                EditorNodeOp::IfDocked => {
                    let response = server.handle_at(&mut None, Request::Snapshot, now);
                    let pilot = session.as_ref().unwrap();
                    let docked = response
                        .snapshot
                        .and_then(|s| s.pilots.into_iter().find(|p| &p.name == pilot))
                        .map(|p| p.location == Location::Station);
                    match docked {
                        Some(value) => {
                            port = if value {
                                EditorGraphPort::True
                            } else {
                                EditorGraphPort::False
                            };
                            (true, format!("Docked condition: {value}"))
                        }
                        None => (false, "Bound pilot is unavailable".into()),
                    }
                }
                operation => {
                    let request = match operation {
                        EditorNodeOp::RoomLight { on } => Request::SetPioneerRoom {
                            action: PioneerRoomCommand::Light { on: *on },
                        },
                        EditorNodeOp::RoomBreaker { closed } => Request::SetPioneerRoom {
                            action: PioneerRoomCommand::Breaker { closed: *closed },
                        },
                        EditorNodeOp::RoomDoor { open } => Request::SetPioneerRoom {
                            action: PioneerRoomCommand::Door {
                                open: *open,
                                manual: false,
                            },
                        },
                        EditorNodeOp::StopShip => Request::Navigate {
                            mode: NavigationMode::Stop,
                        },
                        _ => unreachable!(),
                    };
                    let room_action = matches!(request, Request::SetPioneerRoom { .. });
                    let r = server.handle_at(session, request, now);
                    let message = if room_action {
                        match r.assets.as_ref().and_then(|a| a.pioneer_room.as_ref()) {
                            Some(room) => format!(
                                "{}; light(cmd={},effective={}) breaker={} door(cmd={},effective={}) pressure={}Pa interlock={}",
                                r.message.chars().take(32).collect::<String>(),
                                room.light_commanded, room.light_on, room.breaker_closed,
                                room.door_commanded_open, room.door_open, room.pressure_pa,
                                room.interlock.as_deref().unwrap_or("none").chars().take(48).collect::<String>(),
                            ),
                            None => format!("{}; room state unavailable", r.message),
                        }
                    } else {
                        r.message
                    };
                    (r.ok, message)
                }
            };
            let duration = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            let cause = self.record(
                pending.cause,
                Some(node.id),
                now,
                duration,
                "node",
                &message,
                ok,
            );
            if ok {
                if let Some(next) = self
                    .graph
                    .edges
                    .iter()
                    .find(|e| e.from == node.id && e.port == port)
                    .map(|e| e.to)
                {
                    self.enqueue(next, due, Some(cause))?;
                }
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod pending_tests {
    use super::*;
    #[test]
    fn debug_queue_matches_due_then_insertion_order_and_is_bounded() {
        let mut graph = GraphRuntime::default();
        graph.enqueue(9, 200, Some(41)).unwrap();
        graph.enqueue(3, 100, Some(42)).unwrap();
        graph.enqueue(2, 100, Some(43)).unwrap();
        let debug = graph.debug(true);
        assert_eq!(
            debug
                .pending_nodes
                .iter()
                .map(|p| p.node_id)
                .collect::<Vec<_>>(),
            vec![3, 2, 9]
        );
        assert_eq!(debug.pending_nodes[0].due_at_ms, 100);
        assert_eq!(debug.pending_nodes[0].cause, Some(42));
        for _ in 3..MAX_PENDING {
            graph.enqueue(1, 300, None).unwrap();
        }
        assert!(graph.enqueue(1, 300, None).is_err());
        assert_eq!(graph.debug(true).pending_nodes.len(), MAX_PENDING);
    }
}
