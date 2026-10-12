//! Editor-only instance overrides. Authored meshes/material caches are never changed.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    not(target_arch = "wasm32"),
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct EmbeddedMaterialPreview {
    /// Linear RGB, matching the authored GLB base-color factors.
    pub hull_rgb: [f32; 3],
    pub trim_rgb: [f32; 3],
    /// Multiplier of authored emission, not a physical light power rating.
    pub emissive_strength: f32,
    /// Optional PBR overrides; None preserves each surface's authored value.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub roughness: Option<f32>,
    #[cfg_attr(
        not(target_arch = "wasm32"),
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub metallic: Option<f32>,
}
impl Default for EmbeddedMaterialPreview {
    fn default() -> Self {
        Self {
            hull_rgb: [0.055, 0.075, 0.072],
            trim_rgb: [0.015, 0.3, 0.115],
            emissive_strength: 1.,
            roughness: None,
            metallic: None,
        }
    }
}
impl EmbeddedMaterialPreview {
    pub fn validated(self) -> Self {
        let defaults = Self::default();
        let color = |v: [f32; 3], fallback: [f32; 3]| {
            std::array::from_fn(|i| {
                if v[i].is_finite() {
                    v[i].clamp(0., 1.)
                } else {
                    fallback[i]
                }
            })
        };
        let factor = |v: Option<f32>| v.filter(|v| v.is_finite()).map(|v| v.clamp(0., 1.));
        Self {
            roughness: factor(self.roughness),
            metallic: factor(self.metallic),
            hull_rgb: color(self.hull_rgb, defaults.hull_rgb),
            trim_rgb: color(self.trim_rgb, defaults.trim_rgb),
            emissive_strength: if self.emissive_strength.is_finite() {
                self.emissive_strength.clamp(0., 4.)
            } else {
                1.
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_or_unbounded_overrides_are_safe() {
        let preview = EmbeddedMaterialPreview {
            hull_rgb: [f32::NAN, -5., 10.],
            trim_rgb: [f32::INFINITY, 0.5, 0.2],
            emissive_strength: f32::INFINITY,
            roughness: Some(f32::NAN),
            metallic: Some(2.),
        }
        .validated();
        assert_eq!(preview.hull_rgb, [0.055, 0., 1.]);
        assert_eq!(preview.trim_rgb, [0.015, 0.5, 0.2]);
        assert_eq!(preview.emissive_strength, 1.);
        assert_eq!(preview.roughness, None);
        assert_eq!(preview.metallic, Some(1.));
        assert_eq!(
            EmbeddedMaterialPreview {
                emissive_strength: 100.,
                ..Default::default()
            }
            .validated()
            .emissive_strength,
            4.
        );
    }
}
