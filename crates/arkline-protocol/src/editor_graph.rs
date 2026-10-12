//! Local editor graph data, not a network request or an arbitrary script surface.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const EDITOR_GRAPH_MAX_NODES: usize = 128;
pub const EDITOR_GRAPH_MAX_EDGES: usize = 256;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorEventGraph {
    pub schema_version: u32,
    pub nodes: Vec<EditorGraphNode>,
    pub edges: Vec<EditorGraphEdge>,
}
impl Default for EditorEventGraph {
    fn default() -> Self {
        Self {
            schema_version: 1,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorGraphNode {
    pub id: u64,
    pub label: String,
    pub position: [f32; 2],
    pub operation: EditorNodeOp,
    pub breakpoint: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorNodeOp {
    Manual,
    Tick,
    Delay { ms: u64 },
    IfDocked,
    RoomLight { on: bool },
    RoomBreaker { closed: bool },
    RoomDoor { open: bool },
    StopShip,
    Trace { message: String },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorGraphPort {
    Next,
    True,
    False,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorGraphEdge {
    pub from: u64,
    pub to: u64,
    pub port: EditorGraphPort,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorGraphTrace {
    pub graph_revision: u64,
    pub sequence: u64,
    pub cause: Option<u64>,
    pub node_id: Option<u64>,
    pub simulation_time_ms: u64,
    pub host_duration_us: u64,
    pub kind: String,
    pub message: String,
    pub ok: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorPendingNode {
    pub node_id: u64,
    pub due_at_ms: u64,
    pub cause: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorGraphDebug {
    pub revision: u64,
    pub graph: EditorEventGraph,
    pub pending: usize,
    pub pending_nodes: Vec<EditorPendingNode>,
    pub breakpoint_node: Option<u64>,
    pub trace: Vec<EditorGraphTrace>,
    pub paused: bool,
}
impl EditorEventGraph {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.nodes.len() > EDITOR_GRAPH_MAX_NODES
            || self.edges.len() > EDITOR_GRAPH_MAX_EDGES
        {
            return Err("Graph schema must be 1; maximum 128 nodes / 256 edges".into());
        }
        let mut nodes = BTreeMap::new();
        for n in &self.nodes {
            if n.id == 0 || nodes.insert(n.id, n).is_some() {
                return Err("Node IDs must be positive and unique".into());
            }
            if n.label.len() > 80
                || n.label.chars().any(char::is_control)
                || n.position
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 100_000.)
            {
                return Err("Invalid node label or canvas position".into());
            }
            match &n.operation {
                EditorNodeOp::Delay { ms } if *ms > 60_000 => {
                    return Err("Delay is limited to 60000 simulation milliseconds".into())
                }
                EditorNodeOp::Trace { message }
                    if message.len() > 256 || message.chars().any(char::is_control) =>
                {
                    return Err(
                        "Trace text must be at most 256 bytes without control characters".into(),
                    )
                }
                _ => {}
            }
        }
        let mut ports = BTreeSet::new();
        let mut incoming: BTreeMap<u64, usize> = nodes.keys().map(|&id| (id, 0)).collect();
        for edge in &self.edges {
            let from = nodes
                .get(&edge.from)
                .ok_or("Edge references missing source")?;
            let to = nodes
                .get(&edge.to)
                .ok_or("Edge references missing destination")?;
            if !ports.insert((edge.from, edge.port)) {
                return Err("Only one edge per output port is supported".into());
            }
            if matches!(to.operation, EditorNodeOp::Manual | EditorNodeOp::Tick) {
                return Err("Trigger nodes cannot receive edges".into());
            }
            let valid = if matches!(from.operation, EditorNodeOp::IfDocked) {
                matches!(edge.port, EditorGraphPort::True | EditorGraphPort::False)
            } else {
                edge.port == EditorGraphPort::Next
            };
            if !valid {
                return Err("Edge output port does not match node operation".into());
            }
            *incoming.get_mut(&edge.to).unwrap() += 1;
        }
        let mut ready: BTreeSet<u64> = incoming
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(&id, _)| id)
            .collect();
        let mut visited = 0;
        while let Some(id) = ready.pop_first() {
            visited += 1;
            for e in self.edges.iter().filter(|e| e.from == id) {
                let count = incoming.get_mut(&e.to).unwrap();
                *count -= 1;
                if *count == 0 {
                    ready.insert(e.to);
                }
            }
        }
        if visited != nodes.len() {
            return Err("Graph cycles are not supported".into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn node(id: u64, operation: EditorNodeOp) -> EditorGraphNode {
        EditorGraphNode {
            id,
            label: String::new(),
            position: [0.; 2],
            operation,
            breakpoint: false,
        }
    }
    #[test]
    fn rejects_cycles_ports_and_duplicate_ids() {
        let mut g = EditorEventGraph {
            nodes: vec![
                node(1, EditorNodeOp::Delay { ms: 1 }),
                node(
                    2,
                    EditorNodeOp::Trace {
                        message: "ok".into(),
                    },
                ),
            ],
            edges: vec![EditorGraphEdge {
                from: 1,
                to: 2,
                port: EditorGraphPort::Next,
            }],
            ..Default::default()
        };
        assert!(g.validate().is_ok());
        g.edges.push(EditorGraphEdge {
            from: 2,
            to: 1,
            port: EditorGraphPort::Next,
        });
        assert!(g.validate().is_err());
        g.edges.pop();
        g.edges[0].port = EditorGraphPort::True;
        assert!(g.validate().is_err());
        g.edges.clear();
        g.nodes[1].id = 1;
        assert!(g.validate().is_err());
    }
}
