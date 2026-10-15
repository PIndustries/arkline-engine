//! Bounded, typed material DAG. Only channels supported by the fleet renderer are exposed.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_NODES: usize = 64;
pub type NodeId = u32;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Color,
    Scalar,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialGraph {
    pub version: u32,
    pub nodes: Vec<Node>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: NodeId,
    pub position: [f32; 2],
    pub kind: NodeKind,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum NodeKind {
    Color {
        value: [f32; 3],
    },
    Scalar {
        value: f32,
    },
    MixColor {
        a: Option<NodeId>,
        b: Option<NodeId>,
        factor: Option<NodeId>,
        amount: f32,
    },
    MultiplyScalar {
        a: Option<NodeId>,
        b: Option<NodeId>,
        value_a: f32,
        value_b: f32,
    },
    Output {
        hull: Option<NodeId>,
        trim: Option<NodeId>,
        emission: Option<NodeId>,
        roughness: Option<NodeId>,
        metallic: Option<NodeId>,
    },
}
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialValues {
    pub hull: [f32; 3],
    pub trim: [f32; 3],
    pub emission: f32,
    pub roughness: Option<f32>,
    pub metallic: Option<f32>,
}
#[derive(Clone, Copy)]
enum Value {
    Color([f32; 3]),
    Scalar(f32),
}
impl NodeKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Color { .. } => "Color",
            Self::Scalar { .. } => "Scalar",
            Self::MixColor { .. } => "Mix Color",
            Self::MultiplyScalar { .. } => "Multiply Scalar",
            Self::Output { .. } => "Fleet Material",
        }
    }
    pub fn output_type(&self) -> Option<ValueType> {
        match self {
            Self::Color { .. } | Self::MixColor { .. } => Some(ValueType::Color),
            Self::Scalar { .. } | Self::MultiplyScalar { .. } => Some(ValueType::Scalar),
            Self::Output { .. } => None,
        }
    }
    pub fn inputs(&self) -> Vec<(&'static str, ValueType, Option<NodeId>)> {
        use ValueType::*;
        match self {
            Self::MixColor { a, b, factor, .. } => vec![
                ("A", Color, *a),
                ("B", Color, *b),
                ("Factor", Scalar, *factor),
            ],
            Self::MultiplyScalar { a, b, .. } => vec![("A", Scalar, *a), ("B", Scalar, *b)],
            Self::Output {
                hull,
                trim,
                emission,
                roughness,
                metallic,
            } => vec![
                ("Hull", Color, *hull),
                ("Trim", Color, *trim),
                ("Emission", Scalar, *emission),
                ("Roughness", Scalar, *roughness),
                ("Metallic", Scalar, *metallic),
            ],
            _ => vec![],
        }
    }
    pub fn set_input(&mut self, index: usize, source: Option<NodeId>) -> Result<(), String> {
        let target = match self {
            Self::MixColor { a, b, factor, .. } => match index {
                0 => a,
                1 => b,
                2 => factor,
                _ => return Err("Unknown input".into()),
            },
            Self::MultiplyScalar { a, b, .. } => match index {
                0 => a,
                1 => b,
                _ => return Err("Unknown input".into()),
            },
            Self::Output {
                hull,
                trim,
                emission,
                roughness,
                metallic,
            } => match index {
                0 => hull,
                1 => trim,
                2 => emission,
                3 => roughness,
                4 => metallic,
                _ => return Err("Unknown input".into()),
            },
            _ => return Err("This node has no inputs".into()),
        };
        *target = source;
        Ok(())
    }
}
impl MaterialGraph {
    pub fn from_values(v: &MaterialValues) -> Self {
        let mut nodes = vec![
            Node {
                id: 1,
                position: [40., 40.],
                kind: NodeKind::Color { value: v.hull },
            },
            Node {
                id: 2,
                position: [40., 160.],
                kind: NodeKind::Color { value: v.trim },
            },
            Node {
                id: 3,
                position: [40., 280.],
                kind: NodeKind::Scalar { value: v.emission },
            },
        ];
        for (id, value, y) in [(4, v.roughness, 400.), (5, v.metallic, 520.)] {
            if let Some(value) = value {
                nodes.push(Node {
                    id,
                    position: [40., y],
                    kind: NodeKind::Scalar { value },
                });
            }
        }
        nodes.push(Node {
            id: 6,
            position: [380., 100.],
            kind: NodeKind::Output {
                hull: Some(1),
                trim: Some(2),
                emission: Some(3),
                roughness: v.roughness.map(|_| 4),
                metallic: v.metallic.map(|_| 5),
            },
        });
        Self { version: 1, nodes }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("Unsupported material graph version".into());
        }
        if self.nodes.is_empty() || self.nodes.len() > MAX_NODES {
            return Err(format!("A material graph needs 1–{MAX_NODES} nodes"));
        }
        let mut ids = BTreeSet::new();
        let mut outputs = 0;
        for node in &self.nodes {
            if node.id == 0 || !ids.insert(node.id) {
                return Err("Node IDs must be unique and nonzero".into());
            }
            if node
                .position
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 100_000.)
            {
                return Err("Invalid node position".into());
            }
            match node.kind {
                NodeKind::Color { value }
                    if value
                        .iter()
                        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) =>
                {
                    return Err("Color values must be in 0–1".into());
                }
                NodeKind::Scalar { value }
                    if !value.is_finite() || !(0.0..=4.0).contains(&value) =>
                {
                    return Err("Scalar values must be in 0–4".into());
                }
                NodeKind::MixColor { amount, .. }
                    if !amount.is_finite() || !(0.0..=1.0).contains(&amount) =>
                {
                    return Err("Mix amount must be in 0–1".into());
                }
                NodeKind::MultiplyScalar {
                    value_a, value_b, ..
                } if [value_a, value_b]
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=4.0).contains(v)) =>
                {
                    return Err("Multiply values must be in 0–4".into());
                }
                NodeKind::Output { .. } => outputs += 1,
                _ => {}
            }
        }
        if outputs != 1 {
            return Err("A material graph must have exactly one Fleet Material output".into());
        }
        for node in &self.nodes {
            for (_, kind, source) in node.kind.inputs() {
                if let Some(source) = source {
                    let source = self
                        .node(source)
                        .ok_or("A connection refers to a missing node")?;
                    if source.kind.output_type() != Some(kind) {
                        return Err("Connection types do not match".into());
                    }
                }
            }
        }
        let mut visited = BTreeSet::new();
        let mut active = BTreeSet::new();
        for node in &self.nodes {
            self.visit(node.id, &mut visited, &mut active)?;
        }
        Ok(())
    }
    fn visit(
        &self,
        id: NodeId,
        visited: &mut BTreeSet<NodeId>,
        active: &mut BTreeSet<NodeId>,
    ) -> Result<(), String> {
        if visited.contains(&id) {
            return Ok(());
        }
        if !active.insert(id) {
            return Err("This connection creates a cycle".into());
        }
        for (_, _, source) in self.node(id).ok_or("Missing node")?.kind.inputs() {
            if let Some(source) = source {
                self.visit(source, visited, active)?;
            }
        }
        active.remove(&id);
        visited.insert(id);
        Ok(())
    }
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    /// Reserve IDs before changing a graph. Imported files may use any nonzero u32 ID.
    pub fn next_ids(&self, count: u32) -> Result<Vec<NodeId>, String> {
        if count == 0 || self.nodes.len().saturating_add(count as usize) > MAX_NODES {
            return Err(format!(
                "A material graph supports at most {MAX_NODES} nodes"
            ));
        }
        let last = self.nodes.iter().map(|n| n.id).max().unwrap_or(0);
        let end = last
            .checked_add(count)
            .ok_or("No unused material node IDs remain")?;
        Ok(((last + 1)..=end).collect())
    }
    pub fn connect(&mut self, source: NodeId, target: NodeId, input: usize) -> Result<(), String> {
        let mut draft = self.clone();
        draft
            .nodes
            .iter_mut()
            .find(|n| n.id == target)
            .ok_or("Missing target")?
            .kind
            .set_input(input, Some(source))?;
        draft.validate()?;
        *self = draft;
        Ok(())
    }
    pub fn remove(&mut self, id: NodeId) -> bool {
        if self
            .node(id)
            .is_none_or(|n| matches!(n.kind, NodeKind::Output { .. }))
        {
            return false;
        }
        self.nodes.retain(|n| n.id != id);
        for n in &mut self.nodes {
            let inputs = n.kind.inputs();
            for (i, (_, _, source)) in inputs.into_iter().enumerate() {
                if source == Some(id) {
                    let _ = n.kind.set_input(i, None);
                }
            }
        }
        true
    }
    pub fn evaluate(&self, fallback: &MaterialValues) -> Result<MaterialValues, String> {
        self.validate()?;
        let mut cache = BTreeMap::new();
        let NodeKind::Output {
            hull,
            trim,
            emission,
            roughness,
            metallic,
        } = self
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Output { .. }))
            .ok_or("Missing output")?
            .kind
        else {
            unreachable!()
        };
        let color = |id: Option<NodeId>,
                     default: [f32; 3],
                     cache: &mut BTreeMap<NodeId, Value>|
         -> Result<[f32; 3], String> {
            match id {
                None => Ok(default),
                Some(id) => match self.eval(id, cache)? {
                    Value::Color(v) => Ok(v),
                    _ => Err("Expected color".into()),
                },
            }
        };
        let scalar = |id: Option<NodeId>,
                      cache: &mut BTreeMap<NodeId, Value>|
         -> Result<Option<f32>, String> {
            match id {
                None => Ok(None),
                Some(id) => match self.eval(id, cache)? {
                    Value::Scalar(v) => Ok(Some(v)),
                    _ => Err("Expected scalar".into()),
                },
            }
        };
        let result = MaterialValues {
            hull: color(hull, fallback.hull, &mut cache)?,
            trim: color(trim, fallback.trim, &mut cache)?,
            emission: scalar(emission, &mut cache)?.unwrap_or(fallback.emission),
            roughness: scalar(roughness, &mut cache)?,
            metallic: scalar(metallic, &mut cache)?,
        };
        if !result.emission.is_finite() || !(0.0..=4.0).contains(&result.emission) {
            return Err("Emission output must be in 0–4".into());
        }
        if [result.roughness, result.metallic]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
        {
            return Err("Roughness and metallic outputs must be in 0–1".into());
        }
        Ok(result)
    }
    fn eval(&self, id: NodeId, cache: &mut BTreeMap<NodeId, Value>) -> Result<Value, String> {
        if let Some(value) = cache.get(&id) {
            return Ok(*value);
        }
        let scalar = |source: Option<NodeId>,
                      fallback: f32,
                      cache: &mut BTreeMap<NodeId, Value>|
         -> Result<f32, String> {
            match source {
                None => Ok(fallback),
                Some(id) => match self.eval(id, cache)? {
                    Value::Scalar(v) => Ok(v),
                    _ => Err("Expected scalar".into()),
                },
            }
        };
        let color = |source: Option<NodeId>,
                     fallback: [f32; 3],
                     cache: &mut BTreeMap<NodeId, Value>|
         -> Result<[f32; 3], String> {
            match source {
                None => Ok(fallback),
                Some(id) => match self.eval(id, cache)? {
                    Value::Color(v) => Ok(v),
                    _ => Err("Expected color".into()),
                },
            }
        };
        let value = match self.node(id).ok_or("Missing node")?.kind {
            NodeKind::Color { value } => Value::Color(value),
            NodeKind::Scalar { value } => Value::Scalar(value),
            NodeKind::MixColor {
                a,
                b,
                factor,
                amount,
            } => {
                let a = color(a, [0., 0., 0.], cache)?;
                let b = color(b, [1., 1., 1.], cache)?;
                let t = scalar(factor, amount, cache)?.clamp(0., 1.);
                Value::Color(std::array::from_fn(|i| a[i] * (1. - t) + b[i] * t))
            }
            NodeKind::MultiplyScalar {
                a,
                b,
                value_a,
                value_b,
            } => Value::Scalar(scalar(a, value_a, cache)? * scalar(b, value_b, cache)?),
            NodeKind::Output { .. } => return Err("Material output is not a source".into()),
        };
        cache.insert(id, value);
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn values() -> MaterialValues {
        MaterialValues {
            hull: [0.1, 0.2, 0.3],
            trim: [0., 0.5, 0.1],
            emission: 1.,
            roughness: Some(0.4),
            metallic: None,
        }
    }
    #[test]
    fn evaluates_connected_mix_and_multiply() {
        let mut g = MaterialGraph::from_values(&values());
        g.nodes.push(Node {
            id: 7,
            position: [0., 0.],
            kind: NodeKind::MixColor {
                a: Some(1),
                b: Some(2),
                factor: None,
                amount: 0.5,
            },
        });
        g.connect(7, 6, 0).unwrap();
        g.nodes.push(Node {
            id: 8,
            position: [0., 0.],
            kind: NodeKind::MultiplyScalar {
                a: Some(3),
                b: None,
                value_a: 1.,
                value_b: 2.,
            },
        });
        g.connect(8, 6, 2).unwrap();
        let v = g.evaluate(&values()).unwrap();
        assert_eq!(v.hull, [0.05, 0.35, 0.2]);
        assert_eq!(v.emission, 2.);
        assert_eq!(v.roughness, Some(0.4));
        assert_eq!(v.metallic, None);
    }
    #[test]
    fn rejected_connections_preserve_graph() {
        let mut g = MaterialGraph::from_values(&values());
        let original = g.clone();
        assert!(g.connect(3, 6, 0).is_err());
        assert!(g.connect(999, 6, 0).is_err());
        assert_eq!(g, original);
        g.nodes.push(Node {
            id: 7,
            position: [0., 0.],
            kind: NodeKind::MultiplyScalar {
                a: None,
                b: None,
                value_a: 1.,
                value_b: 1.,
            },
        });
        assert!(g.connect(7, 7, 0).is_err());
        assert!(g.validate().is_ok());
    }
    #[test]
    fn validates_entire_graph_and_range() {
        let base = MaterialGraph::from_values(&values());
        let mut bad = base.clone();
        bad.nodes[0].position[0] = f32::NAN;
        assert!(bad.validate().is_err());
        bad = base.clone();
        bad.nodes[1].id = 1;
        assert!(bad.validate().is_err());
        bad = base.clone();
        bad.nodes.push(Node {
            id: 99,
            position: [0., 0.],
            kind: NodeKind::MultiplyScalar {
                a: Some(99),
                b: None,
                value_a: 1.,
                value_b: 1.,
            },
        });
        assert!(bad.validate().is_err());
        bad = base.clone();
        if let NodeKind::Scalar { value } =
            &mut bad.nodes.iter_mut().find(|n| n.id == 4).unwrap().kind
        {
            *value = 2.;
        }
        assert!(bad.evaluate(&values()).is_err());
        bad = base;
        for id in 100..164 {
            bad.nodes.push(Node {
                id,
                position: [0., 0.],
                kind: NodeKind::Scalar { value: 1. },
            });
        }
        assert!(bad.validate().is_err());
    }
    #[test]
    fn removing_source_disconnects_consumers() {
        let mut g = MaterialGraph::from_values(&values());
        assert!(g.remove(1));
        assert!(g.validate().is_ok());
        assert!(!g.remove(6));
        assert_eq!(g.evaluate(&values()).unwrap().hull, values().hull);
    }
}
