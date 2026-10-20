//! Explicit surface response independent of color or emitted light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Material {
    Rock,
    Ore,
    PaintedHull,
    CoatedPanel,
    GripDeck,
    Composite,
    ServiceCeiling,
    FixtureDiffuser,
    BareMetal,
    Glass,
}
impl Material {
    /// Roughness, metallic fraction, procedural ore inclusion, glass-coated backing.
    pub(super) const fn parameters(self) -> [f32; 4] {
        match self {
            Self::Rock => [0.92, 0.0, 0.0, 0.0],
            Self::Ore => [0.90, 0.0, 1.0, 0.0],
            Self::PaintedHull => [0.46, 0.0, 0.0, 0.0],
            Self::CoatedPanel => [0.52, 0.0, 2.0, 0.0],
            Self::GripDeck => [0.90, 0.0, 3.0, 0.0],
            Self::Composite => [0.84, 0.0, 4.0, 0.0],
            Self::ServiceCeiling => [0.88, 0.0, 5.0, 0.0],
            Self::FixtureDiffuser => [0.65, 0.0, 0.0, 0.0],
            Self::BareMetal => [0.30, 1.0, 0.0, 0.0],
            Self::Glass => [0.12, 0.0, 0.0, 1.0],
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn materials_are_bounded_and_do_not_infer_metal_from_brightness() {
        for material in [
            Material::Rock,
            Material::Ore,
            Material::PaintedHull,
            Material::CoatedPanel,
            Material::GripDeck,
            Material::Composite,
            Material::ServiceCeiling,
            Material::FixtureDiffuser,
            Material::BareMetal,
            Material::Glass,
        ] {
            let [roughness, metal, ore, glass] = material.parameters();
            assert!((0.08..=1.0).contains(&roughness));
            assert!([metal, glass].iter().all(|v| (0.0..=1.0).contains(v)));
            assert!((0.0..=5.0).contains(&ore) && ore.fract() == 0.0);
        }
        assert_eq!(Material::Rock.parameters()[1], 0.0);
        assert_eq!(Material::PaintedHull.parameters()[1], 0.0);
        assert_eq!(Material::Glass.parameters()[1], 0.0);
        assert_eq!(Material::BareMetal.parameters()[1], 1.0);
        assert!(Material::Glass.parameters()[0] < Material::PaintedHull.parameters()[0]);
    }
    #[test]
    fn fresnel_split_never_adds_metal_diffuse_energy() {
        for metal in [0.0_f32, 0.5, 1.0] {
            for albedo in [0.0_f32, 0.18, 0.7, 1.0] {
                for cosine in [0.0_f32, 0.2, 0.7, 1.0] {
                    let f0 = 0.04 * (1.0 - metal) + albedo * metal;
                    let specular = f0 + (1.0 - f0) * (1.0 - cosine).powi(5);
                    let diffuse = (1.0 - specular) * (1.0 - metal);
                    assert!(diffuse + specular <= 1.000001);
                    if metal == 1.0 {
                        assert_eq!(diffuse, 0.0);
                    }
                }
            }
        }
    }
}
