//! Local GPU budgets. These settings never change simulation or sensor range.
use std::path::Path;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightingQuality {
    Low,
    #[default]
    Balanced,
    High,
}
impl LightingQuality {
    pub const ALL: [Self; 3] = [Self::Low, Self::Balanced, Self::High];
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Balanced => "Balanced",
            Self::High => "High",
        }
    }
    pub fn shadow_size(self, feed: bool) -> i32 {
        match self {
            Self::Low => 0,
            _ if feed => 512,
            Self::Balanced => 1024,
            Self::High => 2048,
        }
    }
    pub fn local_light_limit(self, feed: bool) -> usize {
        if feed {
            return 2;
        }
        match self {
            Self::Low => 2,
            Self::Balanced => 4,
            Self::High => 8,
        }
    }
    pub fn load(dir: &Path) -> Self {
        #[cfg(target_arch = "wasm32")]
        let value = super::browser::read(&dir.join("lighting.txt"));
        #[cfg(not(target_arch = "wasm32"))]
        let value = {
            use std::io::Read;
            let mut s = String::new();
            std::fs::File::open(dir.join("lighting.txt"))
                .and_then(|f| f.take(32).read_to_string(&mut s))
                .map(|_| s)
                .map_err(|e| e.to_string())
        };
        match value.as_deref() {
            Ok("Low") => Self::Low,
            Ok("High") => Self::High,
            _ => Self::Balanced,
        }
    }
    pub fn save(self, dir: &Path) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            super::browser::write(&dir.join("lighting.txt"), self.label())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::create_dir_all(dir)
                .and_then(|_| std::fs::write(dir.join("lighting.txt"), self.label()))
                .map_err(|e| e.to_string())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secondary_views_never_multiply_primary_budgets() {
        for quality in LightingQuality::ALL {
            assert!(quality.shadow_size(true) <= 512);
            assert_eq!(quality.local_light_limit(true), 2);
        }
        assert_eq!(LightingQuality::Low.shadow_size(false), 0);
        assert_eq!(LightingQuality::default(), LightingQuality::Balanced);
    }
}
