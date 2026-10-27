//! Bounded local surface assets. These documents do not add a gameplay request.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};

pub const MATERIAL_SCHEMA_VERSION: u32 = 1;
pub const MATERIAL_NOISE_VERSION: u32 = 1;
pub const MATERIAL_MAX_NODES: usize = 64;
pub const MATERIAL_MAX_DEPTH: usize = 16;
pub const MATERIAL_MAX_SOURCE_BYTES: usize = 32 * 1024;
pub const MATERIAL_MAX_UNIFORM_SLOTS: usize = 32;
/// Initial admission ceiling; GPU frame/edit latency still needs measurement.
pub const MATERIAL_MAX_NOISE: usize = 4;
pub const MATERIAL_VALUE_LIMIT: f32 = 1_000_000.0;
/// Avoid subnormal feather division/flush-to-zero differences on GPU drivers.
pub const MATERIAL_MIN_FEATHER_M: f32 = 0.000001;
pub const PIONEER_MATERIAL_SHA256: &str =
    "7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd";
pub type MaterialNodeId = u32;
pub type MaterialParameterId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialValueType {
    Scalar,
    Vector3,
    Position,
    LinearColor,
    Seed,
    Surface,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MaterialParameterValue {
    Scalar(f32),
    Vector3([f32; 3]),
    LinearColor([f32; 3]),
    Seed(u32),
}
impl MaterialParameterValue {
    pub fn value_type(&self) -> MaterialValueType {
        match self {
            Self::Scalar(_) => MaterialValueType::Scalar,
            Self::Vector3(_) => MaterialValueType::Vector3,
            Self::LinearColor(_) => MaterialValueType::LinearColor,
            Self::Seed(_) => MaterialValueType::Seed,
        }
    }
    fn valid(&self) -> bool {
        match self {
            Self::Seed(_) => true,
            Self::Scalar(v) => v.is_finite() && v.abs() <= MATERIAL_VALUE_LIMIT,
            Self::Vector3(v) => v
                .iter()
                .all(|v| v.is_finite() && v.abs() <= MATERIAL_VALUE_LIMIT),
            Self::LinearColor(v) => v.iter().all(|v| v.is_finite() && (0.0..=4.0).contains(v)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetMaterialBinding {
    pub asset_id: String,
    pub asset_sha256: String,
    pub material_index: u32,
}
impl AssetMaterialBinding {
    pub fn pioneer() -> Self {
        Self {
            asset_id: "builtin:pioneer".into(),
            asset_sha256: PIONEER_MATERIAL_SHA256.into(),
            material_index: 0,
        }
    }
    fn valid(&self) -> bool {
        !self.asset_id.trim().is_empty()
            && self.asset_id.len() <= 128
            && !self.asset_id.chars().any(char::is_control)
            && self.asset_sha256.len() == 64
            && self
                .asset_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && self.material_index < 512
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialParameter {
    pub id: MaterialParameterId,
    pub name: String,
    pub value: MaterialParameterValue,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialNode {
    pub id: MaterialNodeId,
    pub op: MaterialNodeOp,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MaterialNodeOp {
    BodyPosition,
    AuthoredSurface,
    Parameter {
        parameter: MaterialParameterId,
    },
    Constant {
        value: MaterialParameterValue,
    },
    TransformPosition {
        position: MaterialNodeId,
        scale: MaterialNodeId,
        offset: MaterialNodeId,
    },
    Noise3D {
        position: MaterialNodeId,
        seed: MaterialNodeId,
    },
    BodyBoxMask {
        position: MaterialNodeId,
        min: MaterialNodeId,
        max: MaterialNodeId,
        feather: MaterialNodeId,
    },
    Remap {
        value: MaterialNodeId,
        in_min: MaterialNodeId,
        in_max: MaterialNodeId,
        out_min: MaterialNodeId,
        out_max: MaterialNodeId,
    },
    Smoothstep {
        value: MaterialNodeId,
        edge0: MaterialNodeId,
        edge1: MaterialNodeId,
    },
    Surface {
        base_color: MaterialNodeId,
        roughness: MaterialNodeId,
        metallic: MaterialNodeId,
        emission: MaterialNodeId,
    },
    MixSurface {
        a: MaterialNodeId,
        b: MaterialNodeId,
        factor: MaterialNodeId,
    },
    SurfaceOutput {
        surface: MaterialNodeId,
    },
}
impl MaterialNodeOp {
    pub fn name(&self) -> &'static str {
        match self {
            Self::BodyPosition => "Body Position",
            Self::AuthoredSurface => "Authored Surface",
            Self::Parameter { .. } => "Parameter",
            Self::Constant { .. } => "Constant",
            Self::TransformPosition { .. } => "Transform Position",
            Self::Noise3D { .. } => "Noise 3D",
            Self::BodyBoxMask { .. } => "Body Box Mask",
            Self::Remap { .. } => "Remap",
            Self::Smoothstep { .. } => "Smoothstep",
            Self::Surface { .. } => "Surface",
            Self::MixSurface { .. } => "Mix Surface",
            Self::SurfaceOutput { .. } => "Surface Output",
        }
    }
    pub fn inputs(&self) -> Vec<(&'static str, MaterialValueType, MaterialNodeId)> {
        use MaterialValueType::*;
        match *self {
            Self::TransformPosition {
                position,
                scale,
                offset,
            } => vec![
                ("Position", Position, position),
                ("Scale", Vector3, scale),
                ("Offset (m)", Vector3, offset),
            ],
            Self::Noise3D { position, seed } => {
                vec![("Position", Position, position), ("Seed", Seed, seed)]
            }
            Self::BodyBoxMask {
                position,
                min,
                max,
                feather,
            } => vec![
                ("Position", Position, position),
                ("Min (m)", Vector3, min),
                ("Max (m)", Vector3, max),
                ("Feather (m)", Scalar, feather),
            ],
            Self::Remap {
                value,
                in_min,
                in_max,
                out_min,
                out_max,
            } => vec![
                ("Value", Scalar, value),
                ("Input min", Scalar, in_min),
                ("Input max", Scalar, in_max),
                ("Output min", Scalar, out_min),
                ("Output max", Scalar, out_max),
            ],
            Self::Smoothstep {
                value,
                edge0,
                edge1,
            } => vec![
                ("Value", Scalar, value),
                ("Edge 0", Scalar, edge0),
                ("Edge 1", Scalar, edge1),
            ],
            Self::Surface {
                base_color,
                roughness,
                metallic,
                emission,
            } => vec![
                ("Base color", LinearColor, base_color),
                ("Roughness", Scalar, roughness),
                ("Metallic", Scalar, metallic),
                ("Emission", LinearColor, emission),
            ],
            Self::MixSurface { a, b, factor } => vec![
                ("A", Surface, a),
                ("B", Surface, b),
                ("Factor", Scalar, factor),
            ],
            Self::SurfaceOutput { surface } => vec![("Surface", Surface, surface)],
            _ => vec![],
        }
    }
    /// Zero is allowed as an editor draft placeholder, but validation rejects it.
    pub fn set_input(&mut self, index: usize, source: MaterialNodeId) -> Result<(), String> {
        let inputs: Vec<&mut MaterialNodeId> = match self {
            Self::TransformPosition {
                position,
                scale,
                offset,
            } => vec![position, scale, offset],
            Self::Noise3D { position, seed } => vec![position, seed],
            Self::BodyBoxMask {
                position,
                min,
                max,
                feather,
            } => vec![position, min, max, feather],
            Self::Remap {
                value,
                in_min,
                in_max,
                out_min,
                out_max,
            } => vec![value, in_min, in_max, out_min, out_max],
            Self::Smoothstep {
                value,
                edge0,
                edge1,
            } => vec![value, edge0, edge1],
            Self::Surface {
                base_color,
                roughness,
                metallic,
                emission,
            } => vec![base_color, roughness, metallic, emission],
            Self::MixSurface { a, b, factor } => vec![a, b, factor],
            Self::SurfaceOutput { surface } => vec![surface],
            _ => vec![],
        };
        let target = inputs
            .into_iter()
            .nth(index)
            .ok_or("Unknown material input")?;
        *target = source;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialGraphDocument {
    pub schema_version: u32,
    pub noise_version: u32,
    pub binding: AssetMaterialBinding,
    pub nodes: Vec<MaterialNode>,
    pub parameters: Vec<MaterialParameter>,
    #[serde(default)]
    pub layout: BTreeMap<MaterialNodeId, [f32; 2]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialDiagnosticCode {
    Schema,
    Binding,
    Budget,
    Identity,
    Value,
    Missing,
    Type,
    Cycle,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialDiagnostic {
    pub node_id: Option<MaterialNodeId>,
    pub code: MaterialDiagnosticCode,
    pub message: String,
}
impl MaterialDiagnostic {
    pub(crate) fn new(
        node_id: Option<u32>,
        code: MaterialDiagnosticCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            node_id,
            code,
            message: message.into(),
        }
    }
}
impl fmt::Display for MaterialDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(id) = self.node_id {
            write!(f, "Node {id}: ")?;
        }
        f.write_str(&self.message)
    }
}
impl std::error::Error for MaterialDiagnostic {}

impl MaterialGraphDocument {
    pub fn node_type(&self, id: MaterialNodeId) -> Result<MaterialValueType, MaterialDiagnostic> {
        use MaterialValueType::*;
        let error = |code, message| MaterialDiagnostic::new(Some(id), code, message);
        let node = self
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| error(MaterialDiagnosticCode::Missing, "Missing input node"))?;
        Ok(match &node.op {
            MaterialNodeOp::BodyPosition | MaterialNodeOp::TransformPosition { .. } => Position,
            MaterialNodeOp::Noise3D { .. }
            | MaterialNodeOp::BodyBoxMask { .. }
            | MaterialNodeOp::Remap { .. }
            | MaterialNodeOp::Smoothstep { .. } => Scalar,
            MaterialNodeOp::AuthoredSurface
            | MaterialNodeOp::Surface { .. }
            | MaterialNodeOp::MixSurface { .. } => Surface,
            MaterialNodeOp::Constant { value } => value.value_type(),
            MaterialNodeOp::Parameter { parameter } => self
                .parameters
                .iter()
                .find(|p| p.id == *parameter)
                .ok_or_else(|| error(MaterialDiagnosticCode::Missing, "Missing parameter"))?
                .value
                .value_type(),
            MaterialNodeOp::SurfaceOutput { .. } => {
                return Err(error(
                    MaterialDiagnosticCode::Type,
                    "Surface Output cannot be an input source",
                ))
            }
        })
    }
    pub fn validate(&self) -> Result<(), Vec<MaterialDiagnostic>> {
        validate_material_document(self)
    }
    pub fn pioneer_default() -> Self {
        use MaterialNodeOp::*;
        use MaterialParameterValue as V;
        let values = vec![
            ("Scale", V::Vector3([0.12; 3])),
            ("Offset (m)", V::Vector3([0.; 3])),
            ("Noise seed", V::Seed(7)),
            ("Mask low", V::Scalar(0.35)),
            ("Mask high", V::Scalar(0.65)),
            ("Armor A color", V::LinearColor([0.055, 0.075, 0.072])),
            ("Armor A roughness", V::Scalar(0.38)),
            ("Armor A metallic", V::Scalar(0.08)),
            ("Armor A emission", V::LinearColor([0.; 3])),
            ("Armor B color", V::LinearColor([0.015, 0.3, 0.115])),
            ("Armor B roughness", V::Scalar(0.7)),
            ("Armor B metallic", V::Scalar(0.75)),
            ("Armor B emission", V::LinearColor([0.; 3])),
            ("Section min (m)", V::Vector3([-22., -8., -20.])),
            ("Section max (m)", V::Vector3([2., 7., -14.])),
            ("Section feather (m)", V::Scalar(0.5)),
        ];
        let ops = vec![
            BodyPosition,
            Parameter { parameter: 1 },
            Parameter { parameter: 2 },
            TransformPosition {
                position: 1,
                scale: 2,
                offset: 3,
            },
            Parameter { parameter: 3 },
            Noise3D {
                position: 4,
                seed: 5,
            },
            Constant {
                value: V::Scalar(0.),
            },
            Constant {
                value: V::Scalar(1.),
            },
            Remap {
                value: 6,
                in_min: 7,
                in_max: 8,
                out_min: 7,
                out_max: 8,
            },
            Parameter { parameter: 4 },
            Parameter { parameter: 5 },
            Smoothstep {
                value: 9,
                edge0: 10,
                edge1: 11,
            },
            Parameter { parameter: 6 },
            Parameter { parameter: 7 },
            Parameter { parameter: 8 },
            Parameter { parameter: 9 },
            Surface {
                base_color: 13,
                roughness: 14,
                metallic: 15,
                emission: 16,
            },
            Parameter { parameter: 10 },
            Parameter { parameter: 11 },
            Parameter { parameter: 12 },
            Parameter { parameter: 13 },
            Surface {
                base_color: 18,
                roughness: 19,
                metallic: 20,
                emission: 21,
            },
            MixSurface {
                a: 17,
                b: 22,
                factor: 12,
            },
            AuthoredSurface,
            Parameter { parameter: 14 },
            Parameter { parameter: 15 },
            Parameter { parameter: 16 },
            BodyBoxMask {
                position: 1,
                min: 25,
                max: 26,
                feather: 27,
            },
            MixSurface {
                a: 24,
                b: 23,
                factor: 28,
            },
            SurfaceOutput { surface: 29 },
        ];
        let nodes: Vec<_> = ops
            .into_iter()
            .enumerate()
            .map(|(i, op)| MaterialNode {
                id: i as u32 + 1,
                op,
            })
            .collect();
        let layout = nodes
            .iter()
            .map(|n| {
                (
                    n.id,
                    [
                        ((n.id - 1) % 4) as f32 * 230.,
                        ((n.id - 1) / 4) as f32 * 130.,
                    ],
                )
            })
            .collect();
        Self {
            schema_version: MATERIAL_SCHEMA_VERSION,
            noise_version: MATERIAL_NOISE_VERSION,
            binding: AssetMaterialBinding::pioneer(),
            nodes,
            layout,
            parameters: values
                .into_iter()
                .enumerate()
                .map(|(i, (name, value))| MaterialParameter {
                    id: i as u32 + 1,
                    name: name.into(),
                    value,
                })
                .collect(),
        }
    }
}

pub fn validate_material_document(
    doc: &MaterialGraphDocument,
) -> Result<(), Vec<MaterialDiagnostic>> {
    use MaterialDiagnosticCode as C;
    let mut errors = Vec::new();
    let mut add = |id, code, msg| errors.push(MaterialDiagnostic::new(id, code, msg));
    if doc.schema_version != MATERIAL_SCHEMA_VERSION || doc.noise_version != MATERIAL_NOISE_VERSION
    {
        add(None, C::Schema, "Unsupported material or noise version");
    }
    if !doc.binding.valid() {
        add(
            None,
            C::Binding,
            "Invalid asset revision or material slot binding",
        );
    }
    if doc.nodes.is_empty()
        || doc.nodes.len() > MATERIAL_MAX_NODES
        || doc.parameters.len() > MATERIAL_MAX_NODES
        || doc.layout.len() > MATERIAL_MAX_NODES
    {
        add(
            None,
            C::Budget,
            "Material needs 1–64 nodes, at most 64 parameters/layout entries",
        );
        return Err(errors);
    }
    let mut nodes = BTreeMap::new();
    for node in &doc.nodes {
        if node.id == 0 || nodes.insert(node.id, node).is_some() {
            add(
                Some(node.id),
                C::Identity,
                "Node IDs must be positive and unique",
            );
        }
        if let MaterialNodeOp::Constant { value } = &node.op {
            if !value.valid() {
                add(Some(node.id), C::Value, "Invalid or nonfinite constant");
            }
        }
    }
    let mut parameters = BTreeMap::new();
    for parameter in &doc.parameters {
        if parameter.id == 0 || parameters.insert(parameter.id, parameter).is_some() {
            add(
                None,
                C::Identity,
                "Parameter IDs must be positive and unique",
            );
        }
        if parameter.name.trim().is_empty()
            || parameter.name.len() > 80
            || parameter.name.chars().any(char::is_control)
            || !parameter.value.valid()
        {
            add(None, C::Value, "Invalid parameter name, value or range");
        }
    }
    for (id, position) in &doc.layout {
        if !nodes.contains_key(id)
            || position
                .iter()
                .any(|v| !v.is_finite() || v.abs() > MATERIAL_VALUE_LIMIT)
        {
            add(
                Some(*id),
                C::Value,
                "Invalid canvas position or node reference",
            );
        }
    }
    if doc
        .nodes
        .iter()
        .filter(|n| matches!(n.op, MaterialNodeOp::SurfaceOutput { .. }))
        .count()
        != 1
    {
        add(None, C::Type, "Exactly one Surface Output is required");
    }
    for node in &doc.nodes {
        if let MaterialNodeOp::Parameter { parameter } = node.op {
            if !parameters.contains_key(&parameter) {
                errors.push(MaterialDiagnostic::new(
                    Some(node.id),
                    C::Missing,
                    "Missing parameter",
                ));
            }
        }
        for (name, expected, source) in node.op.inputs() {
            match doc.node_type(source) {
                Ok(actual) if actual == expected => {}
                Ok(_) => errors.push(MaterialDiagnostic::new(
                    Some(node.id),
                    C::Type,
                    format!("{name} input requires {expected:?}"),
                )),
                Err(error) => errors.push(MaterialDiagnostic::new(
                    Some(node.id),
                    error.code,
                    format!("{name}: {}", error.message),
                )),
            }
        }
        if let MaterialNodeOp::BodyBoxMask {
            min, max, feather, ..
        } = node.op
        {
            let value = |id: u32| -> Option<&MaterialParameterValue> {
                match &nodes.get(&id)?.op {
                    MaterialNodeOp::Constant { value } => Some(value),
                    MaterialNodeOp::Parameter { parameter } => {
                        Some(&parameters.get(parameter)?.value)
                    }
                    _ => None,
                }
            };
            match (value(min), value(max), value(feather)) {
                (Some(MaterialParameterValue::Vector3(min)), Some(MaterialParameterValue::Vector3(max)), Some(MaterialParameterValue::Scalar(feather)))
                    if valid_body_box(*min, *max, *feather) => {},
                _ => errors.push(MaterialDiagnostic::new(Some(node.id), C::Value,
                    "Body Box Mask needs finite Min < Max on every axis and feather in 0.000001–1000000 m, supplied by direct parameters or constants")),
            }
        }
    }
    // All nodes, including unused branches, must be acyclic and within depth limits.
    fn depth(
        id: u32,
        nodes: &BTreeMap<u32, &MaterialNode>,
        active: &mut Vec<u32>,
        done: &mut BTreeMap<u32, usize>,
    ) -> Result<usize, u32> {
        if let Some(d) = done.get(&id) {
            return Ok(*d);
        }
        if active.contains(&id) {
            return Err(id);
        }
        let Some(node) = nodes.get(&id) else {
            return Ok(0);
        };
        active.push(id);
        let mut result = 1;
        for (_, _, source) in node.op.inputs() {
            result = result.max(1 + depth(source, nodes, active, done)?);
        }
        active.pop();
        done.insert(id, result);
        Ok(result)
    }
    let mut done = BTreeMap::new();
    for id in nodes.keys() {
        match depth(*id, &nodes, &mut Vec::new(), &mut done) {
            Ok(d) if d > MATERIAL_MAX_DEPTH => errors.push(MaterialDiagnostic::new(
                Some(*id),
                C::Budget,
                "Material graph exceeds depth 16",
            )),
            Err(cycle) => {
                errors.push(MaterialDiagnostic::new(
                    Some(cycle),
                    C::Cycle,
                    "Material graph contains a cycle",
                ));
                break;
            }
            _ => {}
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn valid_body_box(min: [f32; 3], max: [f32; 3], feather: f32) -> bool {
    feather.is_finite()
        && (MATERIAL_MIN_FEATHER_M..=MATERIAL_VALUE_LIMIT).contains(&feather)
        && (0..3).all(|i| {
            min[i].is_finite()
                && max[i].is_finite()
                && min[i] < max[i]
                && min[i].abs() <= MATERIAL_VALUE_LIMIT
                && max[i].abs() <= MATERIAL_VALUE_LIMIT
        })
}

/// Reference for fixture checks. The mask is zero outside/on the box and feathers
/// inward, so authored values remain exact outside the selected body-metre region.
pub fn body_box_mask(
    position: [f32; 3],
    min: [f32; 3],
    max: [f32; 3],
    feather: f32,
) -> Result<f32, MaterialDiagnostic> {
    if !valid_body_box(min, max, feather) || position.iter().any(|v| !v.is_finite()) {
        return Err(MaterialDiagnostic::new(
            None,
            MaterialDiagnosticCode::Value,
            "Invalid body box coordinates or feather",
        ));
    }
    let edge = (0..3)
        .map(|i| (position[i] - min[i]).min(max[i] - position[i]))
        .fold(f32::INFINITY, f32::min);
    let t = (edge / feather).clamp(0.0, 1.0);
    Ok(t * t * (3.0 - 2.0 * t))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn historical_pioneer_hash_survives_roundtrip_and_lowering_after_model_adoption() {
        let historical_hash = "f8f8cc65772c678a46606c27abe205d62c4e6ab37c350f79a0f3a4cdc0c6f402";
        let current = MaterialGraphDocument::pioneer_default();
        assert_ne!(current.binding.asset_sha256, historical_hash);
        assert_eq!(current.binding.asset_sha256, PIONEER_MATERIAL_SHA256);

        let mut historical = current;
        historical.binding.asset_sha256 = historical_hash.into();
        historical.validate().unwrap();
        let serialized = serde_json::to_vec(&historical).unwrap();
        let restored: MaterialGraphDocument = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(restored, historical);
        assert_eq!(restored.binding.asset_sha256, historical_hash);
        let lowered = crate::material_compile::lower_material(&restored).unwrap();
        assert_eq!(lowered.binding, historical.binding);
        assert_ne!(lowered.binding, AssetMaterialBinding::pioneer());
        assert_eq!(serde_json::to_vec(&restored).unwrap(), serialized);
    }

    #[test]
    fn default_round_trip_and_fixed_binding() {
        let doc = MaterialGraphDocument::pioneer_default();
        doc.validate().unwrap();
        let bytes = serde_json::to_vec(&doc).unwrap();
        assert_eq!(
            serde_json::from_slice::<MaterialGraphDocument>(&bytes).unwrap(),
            doc
        );
        assert_eq!(doc.binding.material_index, 0);
        assert_eq!(doc.binding.asset_sha256, PIONEER_MATERIAL_SHA256);
    }
    #[test]
    fn types_missing_nodes_and_unreachable_cycles_are_rejected() {
        let mut doc = MaterialGraphDocument::pioneer_default();
        doc.nodes[3].op.set_input(1, 13).unwrap(); // Color is not a scale vector.
        assert!(doc
            .validate()
            .unwrap_err()
            .iter()
            .any(|e| e.code == MaterialDiagnosticCode::Type));
        doc.nodes[3].op.set_input(1, 0).unwrap();
        assert!(doc
            .validate()
            .unwrap_err()
            .iter()
            .any(|e| e.code == MaterialDiagnosticCode::Missing));
        let mut doc = MaterialGraphDocument::pioneer_default();
        doc.nodes.push(MaterialNode {
            id: 99,
            op: MaterialNodeOp::Smoothstep {
                value: 99,
                edge0: 7,
                edge1: 8,
            },
        });
        assert!(doc
            .validate()
            .unwrap_err()
            .iter()
            .any(|e| e.code == MaterialDiagnosticCode::Cycle));
    }
    #[test]
    fn rejects_invalid_values_identity_schema_and_file_fields() {
        let doc = MaterialGraphDocument::pioneer_default();
        for value in [f32::NAN, f32::INFINITY, MATERIAL_VALUE_LIMIT * 2.] {
            let mut bad = doc.clone();
            bad.parameters[3].value = MaterialParameterValue::Scalar(value);
            assert!(bad.validate().is_err());
        }
        let mut bad = doc.clone();
        bad.nodes[1].id = 1;
        assert!(bad.validate().is_err());
        let mut bad = doc.clone();
        bad.noise_version = 99;
        assert!(bad.validate().is_err());
        let mut bad = doc.clone();
        bad.binding.asset_sha256 = "../../shader.glsl".into();
        assert!(bad.validate().is_err());
        let mut json = serde_json::to_value(doc).unwrap();
        json["glsl"] = "void main(){}".into();
        assert!(serde_json::from_value::<MaterialGraphDocument>(json).is_err());
    }

    #[test]
    fn section_mask_has_exact_authored_outside_and_smooth_inward_feather() {
        let lo = [-22., -8., -20.];
        let hi = [2., 7., -14.];
        assert_eq!(body_box_mask([-10., 0., -17.], lo, hi, 0.5).unwrap(), 1.0);
        assert_eq!(body_box_mask([10., 0., -17.], lo, hi, 0.5).unwrap(), 0.0);
        assert_eq!(body_box_mask([-22., 0., -17.], lo, hi, 0.5).unwrap(), 0.0);
        assert_eq!(body_box_mask([-21.75, 0., -17.], lo, hi, 0.5).unwrap(), 0.5);
        for axis in 0..3 {
            let mut p = [-10., 0., -17.];
            p[axis] = hi[axis] + 0.1;
            assert_eq!(body_box_mask(p, lo, hi, 0.5).unwrap(), 0.0);
        }
        assert!(body_box_mask([f32::NAN, 0., 0.], lo, hi, 0.5).is_err());
    }

    #[test]
    fn section_validation_rejects_inverted_or_nonfinite_bounds_and_bad_feather() {
        let doc = MaterialGraphDocument::pioneer_default();
        for value in [0.0, -0.5, f32::NAN, f32::INFINITY, 1e-30] {
            let mut bad = doc.clone();
            bad.parameters[15].value = MaterialParameterValue::Scalar(value);
            assert!(bad
                .validate()
                .unwrap_err()
                .iter()
                .any(|e| e.node_id == Some(28)));
        }
        for value in [[2., -8., -20.], [3., -8., -20.], [-22., f32::NAN, -20.]] {
            let mut bad = doc.clone();
            bad.parameters[13].value = MaterialParameterValue::Vector3(value);
            assert!(bad
                .validate()
                .unwrap_err()
                .iter()
                .any(|e| e.node_id == Some(28)));
        }
        let mut bad = doc;
        bad.nodes
            .iter_mut()
            .find(|n| n.id == 28)
            .unwrap()
            .op
            .set_input(3, 6)
            .unwrap();
        assert!(bad
            .validate()
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("direct parameters or constants")));
    }
}
