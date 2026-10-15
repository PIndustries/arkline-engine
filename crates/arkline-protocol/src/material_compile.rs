//! Trusted typed lowering for local surface graphs. No project shader source is read.
use crate::material_graph::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MaterialUniformSlot {
    pub parameter_id: MaterialParameterId,
    pub value_type: MaterialValueType,
    /// Index in params or seeds according to value_type.
    pub slot: usize,
    /// Seed parameters pack four uints per slot; other types start at component0.
    pub component: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialSourceSpan {
    pub node_id: MaterialNodeId,
    /// One-based lines in fragment_function, before renderer boilerplate insertion.
    pub start_line: usize,
    pub end_line: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialCosts {
    pub nodes: usize,
    pub depth: usize,
    pub source_bytes: usize,
    pub uniform_vec4_slots: usize,
    pub noise_evaluations: usize,
}
/// Renderer payload made only from a validated document, never deserialized GLSL.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredMaterial {
    pub topology_key: String,
    pub content_key: String,
    pub binding: AssetMaterialBinding,
    /// Struct, uniform declarations, helpers and ark_graph_surface; no main/version.
    pub fragment_function: String,
    pub params: Vec<[f32; 4]>,
    pub seeds: Vec<[u32; 4]>,
    pub uniform_layout: Vec<MaterialUniformSlot>,
    pub source_map: Vec<MaterialSourceSpan>,
    pub costs: MaterialCosts,
}

#[derive(Clone, Debug)]
enum Literal {
    Scalar(f32),
    Vector([f32; 3]),
    Color([f32; 3]),
    Seed(u32),
    Surface([f32; 8]),
}
#[derive(Clone)]
struct Expression {
    name: String,
    key: String,
    literal: Option<Literal>,
    line: usize,
    depth: usize,
}
fn normalized(v: f32) -> f32 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}
fn scalar(v: f32) -> String {
    let v = normalized(v);
    let mut text = format!("{v:?}");
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        text.push_str(".0");
    }
    text
}
fn vector(v: [f32; 3]) -> String {
    format!("vec3({},{},{})", scalar(v[0]), scalar(v[1]), scalar(v[2]))
}
impl Literal {
    fn from_value(value: &MaterialParameterValue) -> Self {
        match *value {
            MaterialParameterValue::Scalar(v) => Self::Scalar(normalized(v)),
            MaterialParameterValue::Vector3(v) => Self::Vector(v.map(normalized)),
            MaterialParameterValue::LinearColor(v) => Self::Color(v.map(normalized)),
            MaterialParameterValue::Seed(v) => Self::Seed(v),
        }
    }
    fn text(&self) -> String {
        match *self {
            Self::Scalar(v) => scalar(v),
            Self::Vector(v) | Self::Color(v) => vector(v),
            Self::Seed(v) => format!("{v}u"),
            Self::Surface(v) => format!(
                "ArkSurface({},{},{},{})",
                vector([v[0], v[1], v[2]]),
                scalar(v[3]),
                scalar(v[4]),
                vector([v[5], v[6], v[7]])
            ),
        }
    }
    fn key(&self) -> String {
        let kind = match self {
            Self::Scalar(_) => "scalar",
            Self::Vector(_) => "vector3",
            Self::Color(_) => "linear_color",
            Self::Seed(_) => "seed",
            Self::Surface(_) => "surface",
        };
        format!("{kind}:{}", self.text())
    }
}

struct Compiler<'a> {
    nodes: BTreeMap<u32, &'a MaterialNode>,
    slots: BTreeMap<u32, MaterialUniformSlot>,
    by_node: BTreeMap<u32, Expression>,
    shared: BTreeMap<String, Expression>,
    body: Vec<String>,
    spans: Vec<MaterialSourceSpan>,
    noises: usize,
}
impl Compiler<'_> {
    fn emit(&mut self, id: u32) -> Expression {
        if let Some(value) = self.by_node.get(&id) {
            return value.clone();
        }
        let node = self.nodes[&id];
        let children: Vec<_> = node
            .op
            .inputs()
            .iter()
            .map(|(_, _, id)| self.emit(*id))
            .collect();
        let depth = 1 + children.iter().map(|v| v.depth).max().unwrap_or(0);
        let mut literal = None;
        let (kind, expression, key) = match &node.op {
            MaterialNodeOp::BodyPosition => ("vec3", "body_meters".into(), "body_position".into()),
            MaterialNodeOp::AuthoredSurface => (
                "ArkSurface",
                "authored_surface".into(),
                "authored_surface".into(),
            ),
            MaterialNodeOp::Parameter { parameter } => {
                let slot = &self.slots[parameter];
                let (kind, suffix) = match slot.value_type {
                    MaterialValueType::Scalar => ("float", ".x"),
                    MaterialValueType::Vector3 | MaterialValueType::LinearColor => ("vec3", ".xyz"),
                    MaterialValueType::Seed => ("uint", [".x", ".y", ".z", ".w"][slot.component]),
                    _ => unreachable!(),
                };
                let array = if slot.value_type == MaterialValueType::Seed {
                    "ark_graph_seeds"
                } else {
                    "ark_graph_params"
                };
                (
                    kind,
                    format!("{array}[{}]{suffix}", slot.slot),
                    format!("parameter:{parameter}:{:?}", slot.value_type),
                )
            }
            MaterialNodeOp::Constant { value } => {
                literal = Some(Literal::from_value(value));
                let kind = match value {
                    MaterialParameterValue::Scalar(_) => "float",
                    MaterialParameterValue::Seed(_) => "uint",
                    _ => "vec3",
                };
                (
                    kind,
                    literal.as_ref().unwrap().text(),
                    literal.as_ref().unwrap().key(),
                )
            }
            MaterialNodeOp::TransformPosition { .. } => (
                "vec3",
                format!(
                    "clamp({}*{}+{},vec3(-1000000.0),vec3(1000000.0))",
                    children[0].name, children[1].name, children[2].name
                ),
                child_key("transform", &children),
            ),
            MaterialNodeOp::Noise3D { .. } => (
                "float",
                format!(
                    "ark_graph_noise3({},{})",
                    children[0].name, children[1].name
                ),
                child_key("noise_v1", &children),
            ),
            MaterialNodeOp::BodyBoxMask { .. } => (
                "float",
                format!(
                    "ark_graph_box({},{},{},{})",
                    children[0].name, children[1].name, children[2].name, children[3].name
                ),
                child_key("body_box_mask_v1", &children),
            ),
            MaterialNodeOp::Remap { .. } => {
                if let [Some(Literal::Scalar(x)), Some(Literal::Scalar(a)), Some(Literal::Scalar(b)), Some(Literal::Scalar(c)), Some(Literal::Scalar(d))] =
                    children
                        .iter()
                        .map(|v| &v.literal)
                        .collect::<Vec<_>>()
                        .as_slice()
                {
                    literal = Some(Literal::Scalar(remap(*x, *a, *b, *c, *d)));
                }
                (
                    "float",
                    format!(
                        "ark_graph_remap({})",
                        children
                            .iter()
                            .map(|c| c.name.as_str())
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                    child_key("remap", &children),
                )
            }
            MaterialNodeOp::Smoothstep { .. } => {
                if let [Some(Literal::Scalar(x)), Some(Literal::Scalar(a)), Some(Literal::Scalar(b))] =
                    children
                        .iter()
                        .map(|v| &v.literal)
                        .collect::<Vec<_>>()
                        .as_slice()
                {
                    literal = Some(Literal::Scalar(smoothstep(*x, *a, *b)));
                }
                (
                    "float",
                    format!(
                        "ark_graph_smooth({},{},{})",
                        children[0].name, children[1].name, children[2].name
                    ),
                    child_key("smoothstep", &children),
                )
            }
            MaterialNodeOp::Surface { .. } => {
                if let [Some(Literal::Color(c)), Some(Literal::Scalar(r)), Some(Literal::Scalar(m)), Some(Literal::Color(e))] =
                    children
                        .iter()
                        .map(|v| &v.literal)
                        .collect::<Vec<_>>()
                        .as_slice()
                {
                    literal = Some(Literal::Surface([
                        c[0], c[1], c[2], *r, *m, e[0], e[1], e[2],
                    ]));
                }
                (
                    "ArkSurface",
                    format!(
                        "ArkSurface({})",
                        children
                            .iter()
                            .map(|c| c.name.as_str())
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                    child_key("surface", &children),
                )
            }
            MaterialNodeOp::MixSurface { .. } => {
                if let [Some(Literal::Surface(a)), Some(Literal::Surface(b)), Some(Literal::Scalar(t))] =
                    children
                        .iter()
                        .map(|v| &v.literal)
                        .collect::<Vec<_>>()
                        .as_slice()
                {
                    let t = t.clamp(0., 1.);
                    literal = Some(Literal::Surface(std::array::from_fn(|i| {
                        a[i] * (1. - t) + b[i] * t
                    })));
                }
                (
                    "ArkSurface",
                    format!(
                        "ark_graph_mix({},{},{})",
                        children[0].name, children[1].name, children[2].name
                    ),
                    child_key("mix_surface", &children),
                )
            }
            MaterialNodeOp::SurfaceOutput { .. } => (
                "ArkSurface",
                format!("ark_graph_finish({})", children[0].name),
                child_key("output", &children),
            ),
        };
        let (expression, key) = if let Some(value) = &literal {
            (value.text(), value.key())
        } else {
            (expression, key)
        };
        let result = if let Some(shared) = self.shared.get(&key) {
            let mut shared = shared.clone();
            shared.depth = depth;
            shared
        } else {
            if matches!(node.op, MaterialNodeOp::Noise3D { .. }) {
                self.noises += 1;
            }
            let name = format!("ark_n{}", self.body.len());
            self.body.push(format!("{kind} {name}={expression};"));
            let value = Expression {
                name,
                key: key.clone(),
                literal,
                line: self.body.len(),
                depth,
            };
            self.shared.insert(key, value.clone());
            value
        };
        self.spans.push(MaterialSourceSpan {
            node_id: id,
            start_line: result.line,
            end_line: result.line,
        });
        self.by_node.insert(id, result.clone());
        result
    }
}
fn child_key(op: &str, children: &[Expression]) -> String {
    // Child hashes bound key size even when a graph repeatedly reuses an expression.
    let mut hash = Sha256::new();
    hash.update(op.as_bytes());
    for child in children {
        hash.update([0]);
        hash.update(child.key.as_bytes());
    }
    format!("{op}:{}", hex::encode(hash.finalize()))
}
fn remap(x: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    if (b - a).abs() < 0.000001 {
        c
    } else {
        (c + (x - a) / (b - a) * (d - c)).clamp(-MATERIAL_VALUE_LIMIT, MATERIAL_VALUE_LIMIT)
    }
}
fn smoothstep(x: f32, a: f32, b: f32) -> f32 {
    if (b - a).abs() < 0.000001 {
        if x >= b {
            1.
        } else {
            0.
        }
    } else {
        let t = ((x - a) / (b - a)).clamp(0., 1.);
        t * t * (3. - 2. * t)
    }
}

pub fn lower_material(
    doc: &MaterialGraphDocument,
) -> Result<LoweredMaterial, Vec<MaterialDiagnostic>> {
    doc.validate()?;
    let nodes: BTreeMap<_, _> = doc.nodes.iter().map(|n| (n.id, n)).collect();
    let output = doc
        .nodes
        .iter()
        .find(|n| matches!(n.op, MaterialNodeOp::SurfaceOutput { .. }))
        .unwrap()
        .id;
    fn reachable(id: u32, nodes: &BTreeMap<u32, &MaterialNode>, seen: &mut BTreeSet<u32>) {
        if seen.insert(id) {
            for (_, _, source) in nodes[&id].op.inputs() {
                reachable(source, nodes, seen);
            }
        }
    }
    let mut seen = BTreeSet::new();
    reachable(output, &nodes, &mut seen);
    let used: BTreeSet<_> = seen
        .iter()
        .filter_map(|id| match nodes[id].op {
            MaterialNodeOp::Parameter { parameter } => Some(parameter),
            _ => None,
        })
        .collect();
    let parameters: BTreeMap<_, _> = doc.parameters.iter().map(|p| (p.id, p)).collect();
    let mut params = Vec::new();
    let mut seed_values = Vec::new();
    let mut uniform_layout = Vec::new();
    for id in used {
        let parameter = parameters[&id];
        let value_type = parameter.value.value_type();
        let (slot, component) = match parameter.value {
            MaterialParameterValue::Seed(v) => {
                let n = seed_values.len();
                seed_values.push(v);
                (n / 4, n % 4)
            }
            MaterialParameterValue::Scalar(v) => {
                let n = params.len();
                params.push([normalized(v), 0., 0., 0.]);
                (n, 0)
            }
            MaterialParameterValue::Vector3(v) | MaterialParameterValue::LinearColor(v) => {
                let n = params.len();
                params.push([normalized(v[0]), normalized(v[1]), normalized(v[2]), 0.]);
                (n, 0)
            }
        };
        uniform_layout.push(MaterialUniformSlot {
            parameter_id: id,
            value_type,
            slot,
            component,
        });
    }
    let mut seeds = vec![[0u32; 4]; seed_values.len().div_ceil(4)];
    for (i, seed) in seed_values.into_iter().enumerate() {
        seeds[i / 4][i % 4] = seed;
    }
    if params.len() + seeds.len() > MATERIAL_MAX_UNIFORM_SLOTS {
        return Err(vec![MaterialDiagnostic::new(
            None,
            MaterialDiagnosticCode::Budget,
            "Material exceeds 32 vec4-equivalent parameter slots",
        )]);
    }
    let slots = uniform_layout
        .iter()
        .map(|s| (s.parameter_id, s.clone()))
        .collect();
    let mut compiler = Compiler {
        nodes,
        slots,
        by_node: BTreeMap::new(),
        shared: BTreeMap::new(),
        body: Vec::new(),
        spans: Vec::new(),
        noises: 0,
    };
    let result = compiler.emit(output);
    if compiler.noises > MATERIAL_MAX_NOISE {
        return Err(vec![MaterialDiagnostic::new(None,MaterialDiagnosticCode::Budget,"Material exceeds four distinct Noise3D evaluations; this initial budget is provisional")]);
    }
    let mut source = String::from("// Arkline material compiler 1 / value-noise 1\n");
    if !params.is_empty() {
        source.push_str(&format!(
            "uniform vec4 ark_graph_params[{}];\n",
            params.len()
        ));
    }
    if !seeds.is_empty() {
        source.push_str(&format!(
            "uniform uvec4 ark_graph_seeds[{}];\n",
            seeds.len()
        ));
    }
    source.push_str(HELPERS);
    source
        .push_str("ArkSurface ark_graph_surface(vec3 body_meters, ArkSurface authored_surface){\n");
    let offset = source.lines().count();
    for line in &compiler.body {
        source.push_str(line);
        source.push('\n');
    }
    source.push_str(&format!("return {};\n}}\n", result.name));
    if source.len() > MATERIAL_MAX_SOURCE_BYTES {
        return Err(vec![MaterialDiagnostic::new(
            None,
            MaterialDiagnosticCode::Budget,
            "Generated material function exceeds 32 KiB",
        )]);
    }
    for span in &mut compiler.spans {
        span.start_line += offset;
        span.end_line += offset;
    }
    compiler.spans.sort_by_key(|s| s.node_id);
    let mut hash = Sha256::new();
    hash.update(source.as_bytes());
    hash.update(serde_json::to_vec(&uniform_layout).unwrap());
    let topology_key = hex::encode(hash.finalize());
    let mut hash = Sha256::new();
    hash.update(topology_key.as_bytes());
    hash.update(serde_json::to_vec(&(&doc.binding, &params, &seeds)).unwrap());
    let content_key = hex::encode(hash.finalize());
    let costs = MaterialCosts {
        nodes: seen.len(),
        depth: result.depth,
        source_bytes: source.len(),
        uniform_vec4_slots: params.len() + seeds.len(),
        noise_evaluations: compiler.noises,
    };
    Ok(LoweredMaterial {
        topology_key,
        content_key,
        binding: doc.binding.clone(),
        fragment_function: source,
        params,
        seeds,
        uniform_layout,
        source_map: compiler.spans,
        costs,
    })
}

// The same bounded operators are emitted on desktop GLSL150 and WebGL2 ES300.
// Degenerate remap returns output-min; degenerate smoothstep is a step at edge1.
// Coordinates and scalar remap outputs saturate at 1e6 to keep lattice/int math finite.
const HELPERS: &str = r#"struct ArkSurface {vec3 base_color;float roughness;float metallic;vec3 emission;};
float ark_graph_box(vec3 p,vec3 lo,vec3 hi,float feather){vec3 d=min(p-lo,hi-p);float t=clamp(min(d.x,min(d.y,d.z))/feather,0.0,1.0);return t*t*(3.0-2.0*t);}
float ark_graph_remap(float x,float a,float b,float c,float d){if(abs(b-a)<0.000001)return c;return clamp(c+(x-a)/(b-a)*(d-c),-1000000.0,1000000.0);}
float ark_graph_smooth(float x,float a,float b){if(abs(b-a)<0.000001)return step(b,x);float t=clamp((x-a)/(b-a),0.0,1.0);return t*t*(3.0-2.0*t);}
ArkSurface ark_graph_mix(ArkSurface a,ArkSurface b,float t){t=clamp(t,0.0,1.0);return ArkSurface(mix(a.base_color,b.base_color,t),mix(a.roughness,b.roughness,t),mix(a.metallic,b.metallic,t),mix(a.emission,b.emission,t));}
ArkSurface ark_graph_finish(ArkSurface s){return ArkSurface(clamp(s.base_color,vec3(0.0),vec3(1.0)),clamp(s.roughness,0.08,1.0),clamp(s.metallic,0.0,1.0),clamp(s.emission,vec3(0.0),vec3(4.0)));}
uint ark_graph_hash(ivec3 p,uint seed){uint h=uint(p.x)*1597334677u^uint(p.y)*3812015801u^uint(p.z)*2798796415u^seed;h^=h>>16u;h*=2246822519u;h^=h>>13u;h*=3266489917u;return h^(h>>16u);}
float ark_graph_corner(ivec3 p,uint seed){return float(ark_graph_hash(p,seed)&16777215u)/16777215.0;}
float ark_graph_noise3(vec3 p,uint seed){p=clamp(p,vec3(-1000000.0),vec3(1000000.0));ivec3 i=ivec3(floor(p));vec3 f=fract(p);f=f*f*(3.0-2.0*f);return mix(mix(mix(ark_graph_corner(i,seed),ark_graph_corner(i+ivec3(1,0,0),seed),f.x),mix(ark_graph_corner(i+ivec3(0,1,0),seed),ark_graph_corner(i+ivec3(1,1,0),seed),f.x),f.y),mix(mix(ark_graph_corner(i+ivec3(0,0,1),seed),ark_graph_corner(i+ivec3(1,0,1),seed),f.x),mix(ark_graph_corner(i+ivec3(0,1,1),seed),ark_graph_corner(i+ivec3(1,1,1),seed),f.x),f.y),f.z);}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    fn parameter(doc: &mut MaterialGraphDocument, id: u32, value: MaterialParameterValue) {
        doc.parameters
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .value = value;
    }
    #[test]
    fn spatial_default_has_bounded_glsl_uniforms_and_diagnostic_map() {
        let d = MaterialGraphDocument::pioneer_default();
        let p = lower_material(&d).unwrap();
        assert!(p.fragment_function.contains(
            "ArkSurface ark_graph_surface(vec3 body_meters, ArkSurface authored_surface)"
        ));
        assert!(p.fragment_function.contains("ark_graph_noise3(ark_n"));
        assert_eq!(p.costs.noise_evaluations, 1);
        assert_eq!(p.seeds, vec![[7, 0, 0, 0]]);
        assert_eq!(p.params.len(), 15);
        assert_eq!(p.costs.uniform_vec4_slots, 16);
        assert_eq!(p.source_map.len(), 30);
        assert!(p
            .source_map
            .iter()
            .all(|s| s.start_line > 1 && s.end_line <= p.fragment_function.lines().count()));
        assert!(p.costs.source_bytes <= MATERIAL_MAX_SOURCE_BYTES);
        assert!(!p.fragment_function.contains("#version"));
        assert!(!p.fragment_function.contains("void main"));
    }
    #[test]
    fn layout_node_parameter_order_names_and_numeric_edits_have_correct_keys() {
        let mut d = MaterialGraphDocument::pioneer_default();
        let a = lower_material(&d).unwrap();
        d.nodes.reverse();
        d.parameters.reverse();
        d.layout.insert(1, [99., 77.]);
        d.parameters[0].name = "renamed".into();
        let b = lower_material(&d).unwrap();
        assert_eq!(a.topology_key, b.topology_key);
        assert_eq!(a.content_key, b.content_key);
        assert_eq!(a.fragment_function, b.fragment_function);
        parameter(&mut d, 4, MaterialParameterValue::Scalar(0.42));
        let c = lower_material(&d).unwrap();
        assert_eq!(b.topology_key, c.topology_key);
        assert_ne!(b.params, c.params);
        assert_ne!(b.content_key, c.content_key);
        parameter(&mut d, 3, MaterialParameterValue::Seed(u32::MAX));
        let e = lower_material(&d).unwrap();
        assert_eq!(c.topology_key, e.topology_key);
        assert_eq!(e.seeds, vec![[u32::MAX, 0, 0, 0]]);
        assert_ne!(e.content_key, c.content_key);
        d.binding.material_index = 1;
        let f = lower_material(&d).unwrap();
        assert_eq!(e.topology_key, f.topology_key);
        assert_ne!(e.content_key, f.content_key);
        d.nodes.iter_mut().find(|n| n.id == 7).unwrap().op = MaterialNodeOp::Constant {
            value: MaterialParameterValue::Scalar(0.1),
        };
        assert_ne!(lower_material(&d).unwrap().topology_key, f.topology_key);
    }
    #[test]
    fn folds_constants_but_not_exposed_parameters_and_shares_noise() {
        let mut d = MaterialGraphDocument::pioneer_default();
        d.nodes.push(MaterialNode {
            id: 31,
            op: MaterialNodeOp::Noise3D {
                position: 4,
                seed: 5,
            },
        });
        d.nodes.iter_mut().find(|n| n.id == 9).unwrap().op = MaterialNodeOp::Remap {
            value: 31,
            in_min: 7,
            in_max: 8,
            out_min: 7,
            out_max: 8,
        };
        // Keep both equal noise nodes reachable through Smoothstep's value and edge.
        d.nodes.iter_mut().find(|n| n.id == 12).unwrap().op = MaterialNodeOp::Smoothstep {
            value: 9,
            edge0: 6,
            edge1: 11,
        };
        let p = lower_material(&d).unwrap();
        assert_eq!(p.costs.noise_evaluations, 1);
        assert_eq!(
            p.source_map
                .iter()
                .find(|s| s.node_id == 6)
                .unwrap()
                .start_line,
            p.source_map
                .iter()
                .find(|s| s.node_id == 31)
                .unwrap()
                .start_line
        );
        let mut d = MaterialGraphDocument::pioneer_default();
        d.nodes.iter_mut().find(|n| n.id == 9).unwrap().op = MaterialNodeOp::Remap {
            value: 8,
            in_min: 7,
            in_max: 8,
            out_min: 7,
            out_max: 8,
        };
        let p = lower_material(&d).unwrap();
        assert_eq!(p.costs.noise_evaluations, 0);
        assert!(!p.fragment_function.contains("=ark_graph_remap("));
        assert!(p.fragment_function.contains("=ark_graph_params["));
    }
    #[test]
    fn unreachable_invalid_nodes_and_budgeted_deep_paths_are_rejected() {
        let mut d = MaterialGraphDocument::pioneer_default();
        for id in 31..=46 {
            d.nodes.push(MaterialNode {
                id,
                op: MaterialNodeOp::Smoothstep {
                    value: if id == 31 { 7 } else { id - 1 },
                    edge0: 7,
                    edge1: 8,
                },
            });
        }
        assert!(lower_material(&d)
            .unwrap_err()
            .iter()
            .any(|e| e.code == MaterialDiagnosticCode::Budget));
        let mut d = MaterialGraphDocument::pioneer_default();
        while d.nodes.len() <= 64 {
            let id = d.nodes.len() as u32 + 1;
            d.nodes.push(MaterialNode {
                id,
                op: MaterialNodeOp::Constant {
                    value: MaterialParameterValue::Scalar(0.),
                },
            });
        }
        assert!(lower_material(&d)
            .unwrap_err()
            .iter()
            .any(|e| e.code == MaterialDiagnosticCode::Budget));
    }
    #[test]
    fn degenerate_scalar_operations_are_defined_and_finite() {
        assert_eq!(remap(9., 1., 1., 3., 8.), 3.);
        assert_eq!(smoothstep(0., 1., 1.), 0.);
        assert_eq!(smoothstep(1., 1., 1.), 1.);
        assert_eq!(smoothstep(0.25, 1., 0.), 0.84375);
        assert!(remap(1e6, -1e6, 1e6, 1e6, -1e6).is_finite());
    }

    fn distinct_noises(extra: u32) -> MaterialGraphDocument {
        let mut doc = MaterialGraphDocument::pioneer_default();
        let mut previous = 6;
        for i in 0..extra {
            let seed = 31 + i * 3;
            let noise = seed + 1;
            let remap = seed + 2;
            doc.nodes.extend([
                MaterialNode {
                    id: seed,
                    op: MaterialNodeOp::Constant {
                        value: MaterialParameterValue::Seed(100 + i),
                    },
                },
                MaterialNode {
                    id: noise,
                    op: MaterialNodeOp::Noise3D { position: 4, seed },
                },
                MaterialNode {
                    id: remap,
                    op: MaterialNodeOp::Remap {
                        value: previous,
                        in_min: 7,
                        in_max: 8,
                        out_min: noise,
                        out_max: 8,
                    },
                },
            ]);
            previous = remap;
        }
        doc.nodes.iter_mut().find(|n| n.id == 12).unwrap().op = MaterialNodeOp::Smoothstep {
            value: previous,
            edge0: 10,
            edge1: 11,
        };
        doc
    }

    #[test]
    fn admits_four_distinct_noises_and_rejects_five() {
        let four = distinct_noises(3);
        assert_eq!(lower_material(&four).unwrap().costs.noise_evaluations, 4);
        let five = distinct_noises(4);
        five.validate().unwrap();
        assert!(lower_material(&five)
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("four distinct Noise3D")));
    }

    #[test]
    fn equal_seed_values_do_not_alias_independently_editable_seeds() {
        let mut doc = MaterialGraphDocument::pioneer_default();
        doc.parameters.push(MaterialParameter {
            id: 17,
            name: "Independent seed".into(),
            value: MaterialParameterValue::Seed(7),
        });
        doc.nodes.extend([
            MaterialNode {
                id: 31,
                op: MaterialNodeOp::Parameter { parameter: 17 },
            },
            MaterialNode {
                id: 32,
                op: MaterialNodeOp::Noise3D {
                    position: 4,
                    seed: 31,
                },
            },
        ]);
        doc.nodes.iter_mut().find(|n| n.id == 12).unwrap().op = MaterialNodeOp::Smoothstep {
            value: 9,
            edge0: 32,
            edge1: 11,
        };
        let first = lower_material(&doc).unwrap();
        assert_eq!(first.costs.noise_evaluations, 2);
        assert_eq!(first.seeds, vec![[7, 7, 0, 0]]);
        parameter(&mut doc, 17, MaterialParameterValue::Seed(8));
        let second = lower_material(&doc).unwrap();
        assert_eq!(first.topology_key, second.topology_key);
        assert_eq!(second.seeds, vec![[7, 8, 0, 0]]);
        assert_ne!(first.content_key, second.content_key);
    }

    fn numeric_uniforms(count: u32) -> MaterialGraphDocument {
        use MaterialNodeOp::*;
        let mut doc = MaterialGraphDocument::pioneer_default();
        doc.nodes.clear();
        doc.parameters.clear();
        doc.layout.clear();
        doc.nodes.push(MaterialNode {
            id: 1,
            op: Parameter { parameter: 1 },
        });
        doc.parameters.push(MaterialParameter {
            id: 1,
            name: "P1".into(),
            value: MaterialParameterValue::Scalar(0.5),
        });
        let mut previous = 1;
        // Four new scalar inputs per remap keep many independent uniforms reachable
        // without using more than the node/depth budgets.
        while doc.parameters.len() < count as usize - 1 {
            let mut inputs = [1u32; 4];
            for input in &mut inputs {
                if doc.parameters.len() < count as usize - 1 {
                    let parameter = doc.parameters.len() as u32 + 1;
                    let id = doc.nodes.len() as u32 + 1;
                    doc.parameters.push(MaterialParameter {
                        id: parameter,
                        name: format!("P{parameter}"),
                        value: MaterialParameterValue::Scalar(0.5),
                    });
                    doc.nodes.push(MaterialNode {
                        id,
                        op: Parameter { parameter },
                    });
                    *input = id;
                }
            }
            let id = doc.nodes.len() as u32 + 1;
            doc.nodes.push(MaterialNode {
                id,
                op: Remap {
                    value: previous,
                    in_min: inputs[0],
                    in_max: inputs[1],
                    out_min: inputs[2],
                    out_max: inputs[3],
                },
            });
            previous = id;
        }
        let color = doc.nodes.len() as u32 + 1;
        doc.parameters.push(MaterialParameter {
            id: count,
            name: "Color".into(),
            value: MaterialParameterValue::LinearColor([0.2; 3]),
        });
        doc.nodes.extend([
            MaterialNode {
                id: color,
                op: Parameter { parameter: count },
            },
            MaterialNode {
                id: color + 1,
                op: Surface {
                    base_color: color,
                    roughness: previous,
                    metallic: 1,
                    emission: color,
                },
            },
            MaterialNode {
                id: color + 2,
                op: SurfaceOutput { surface: color + 1 },
            },
        ]);
        doc
    }

    #[test]
    fn uniform_budget_is_independent_of_node_budget() {
        let boundary = numeric_uniforms(32);
        assert_eq!(
            lower_material(&boundary).unwrap().costs.uniform_vec4_slots,
            32
        );
        let over = numeric_uniforms(33);
        over.validate().unwrap();
        assert!(over.nodes.len() <= 64);
        assert!(lower_material(&over)
            .unwrap_err()
            .iter()
            .any(|e| e.message.contains("32 vec4-equivalent")));
    }

    #[test]
    fn negative_zero_is_canonical_and_type_edits_require_new_topology() {
        let mut doc = MaterialGraphDocument::pioneer_default();
        parameter(
            &mut doc,
            2,
            MaterialParameterValue::Vector3([-0.0, 0.0, -0.0]),
        );
        let a = lower_material(&doc).unwrap();
        parameter(&mut doc, 2, MaterialParameterValue::Vector3([0.0; 3]));
        let b = lower_material(&doc).unwrap();
        assert_eq!(a.topology_key, b.topology_key);
        assert_eq!(a.content_key, b.content_key);
        parameter(&mut doc, 2, MaterialParameterValue::LinearColor([0.0; 3]));
        assert!(lower_material(&doc)
            .unwrap_err()
            .iter()
            .any(|e| e.code == MaterialDiagnosticCode::Type));
    }

    #[test]
    fn authored_section_bounds_are_uniform_only_and_authored_input_is_preserved() {
        let mut doc = MaterialGraphDocument::pioneer_default();
        let a = lower_material(&doc).unwrap();
        assert!(a.fragment_function.contains("=authored_surface;"));
        assert!(a.fragment_function.contains("=ark_graph_box("));
        parameter(
            &mut doc,
            14,
            MaterialParameterValue::Vector3([-20., -7., -19.]),
        );
        parameter(&mut doc, 16, MaterialParameterValue::Scalar(0.75));
        let b = lower_material(&doc).unwrap();
        assert_eq!(a.topology_key, b.topology_key);
        assert_eq!(a.fragment_function, b.fragment_function);
        assert_ne!(a.params, b.params);
        assert_ne!(a.content_key, b.content_key);
        // The default final blend is authored A + procedural B using the body mask.
        assert_eq!(
            doc.nodes.iter().find(|n| n.id == 29).unwrap().op,
            MaterialNodeOp::MixSurface {
                a: 24,
                b: 23,
                factor: 28
            }
        );
    }
}
