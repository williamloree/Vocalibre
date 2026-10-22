//! Model catalogue, embedded at build time. The UI reads sizes and
//! requirements from here; nothing is hard-coded elsewhere.

use serde::{Deserialize, Serialize};

const EMBEDDED: &str = include_str!("../../../models/manifest.json");

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("invalid manifest JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported manifest version {0}")]
    Version(u32),
    #[error("invalid manifest: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Whisper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareClass {
    Gpu,
    CpuStrong,
    CpuWeak,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteFile {
    pub url: String,
    pub file_name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub engine: Engine,
    pub display_name: String,
    pub file: RemoteFile,
    /// ISO 639-1 codes, or `["*"]` for all Whisper languages.
    pub languages: Vec<String>,
    pub ram_mb: u32,
    pub vram_mb: u32,
    /// 1 (slow) – 5 (fast).
    pub speed: u8,
    /// 1 – 5.
    pub accuracy: u8,
    pub supports_prompt: bool,
    /// Encoder context may be shrunk to the audio length (big CPU speed-up).
    /// Off for large-v3-turbo: it degrades accuracy badly (benchmark).
    pub reduced_audio_ctx: bool,
    pub recommended_for: Vec<HardwareClass>,
}

impl ModelInfo {
    pub fn supports_language(&self, lang: &str) -> bool {
        self.languages.iter().any(|l| l == "*" || l == lang)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub vad: RemoteFile,
    pub models: Vec<ModelInfo>,
}

impl Manifest {
    pub fn embedded() -> Result<Self, ManifestError> {
        Self::parse(EMBEDDED)
    }

    pub fn parse(json: &str) -> Result<Self, ManifestError> {
        let m: Manifest = serde_json::from_str(json)?;
        if m.version != 1 {
            return Err(ManifestError::Version(m.version));
        }
        m.check()?;
        Ok(m)
    }

    pub fn get(&self, id: &str) -> Option<&ModelInfo> {
        self.models.iter().find(|m| m.id == id)
    }

    /// First model recommended for `class`, in manifest order.
    pub fn recommended(&self, class: HardwareClass) -> Option<&ModelInfo> {
        self.models
            .iter()
            .find(|m| m.recommended_for.contains(&class))
    }

    fn check(&self) -> Result<(), ManifestError> {
        let invalid = |msg: String| Err(ManifestError::Invalid(msg));
        let mut files = vec![&self.vad];
        let mut ids = std::collections::HashSet::new();
        for m in &self.models {
            if !ids.insert(&m.id) {
                return invalid(format!("duplicate id {}", m.id));
            }
            if !(1..=5).contains(&m.speed) || !(1..=5).contains(&m.accuracy) {
                return invalid(format!("{}: speed/accuracy out of 1..=5", m.id));
            }
            if m.languages.is_empty() {
                return invalid(format!("{}: no language", m.id));
            }
            files.push(&m.file);
        }
        let mut names = std::collections::HashSet::new();
        for f in files {
            let hex = f.sha256.len() == 64 && f.sha256.bytes().all(|b| b.is_ascii_hexdigit());
            if !hex {
                return invalid(format!("{}: bad sha256", f.file_name));
            }
            if f.size == 0 || !f.url.starts_with("https://") {
                return invalid(format!("{}: bad size or url", f.file_name));
            }
            let safe = !f.file_name.is_empty()
                && f.file_name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
            if !safe || !names.insert(&f.file_name) {
                return invalid(format!("{}: unsafe or duplicate file name", f.file_name));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_is_valid() {
        let m = Manifest::embedded().unwrap();
        assert!(!m.models.is_empty());
    }

    #[test]
    fn every_hardware_class_has_a_recommendation() {
        let m = Manifest::embedded().unwrap();
        for class in [
            HardwareClass::Gpu,
            HardwareClass::CpuStrong,
            HardwareClass::CpuWeak,
        ] {
            assert!(m.recommended(class).is_some(), "{class:?}");
        }
    }

    #[test]
    fn french_specialist_only_supports_french() {
        let m = Manifest::embedded().unwrap();
        let fr = m.get("whisper-large-v3-french-distil-q5_0").unwrap();
        assert!(fr.supports_language("fr"));
        assert!(!fr.supports_language("en"));
        assert!(m.get("whisper-base-q5_1").unwrap().supports_language("en"));
    }

    fn with(edit: impl FnOnce(&mut serde_json::Value)) -> Result<Manifest, ManifestError> {
        let mut v: serde_json::Value = serde_json::from_str(EMBEDDED).unwrap();
        edit(&mut v);
        Manifest::parse(&v.to_string())
    }

    #[test]
    fn rejects_bad_entries() {
        assert!(matches!(
            with(|v| v["version"] = 2.into()),
            Err(ManifestError::Version(2))
        ));
        assert!(with(|v| v["models"][0]["file"]["sha256"] = "abc".into()).is_err());
        assert!(with(|v| v["models"][0]["file"]["file_name"] = "../evil.bin".into()).is_err());
        assert!(with(|v| v["models"][0]["file"]["url"] = "http://x".into()).is_err());
        assert!(with(|v| v["models"][0]["speed"] = 9.into()).is_err());
        assert!(with(|v| {
            let first = v["models"][0].clone();
            v["models"].as_array_mut().unwrap().push(first);
        })
        .is_err());
        assert!(with(|v| v["models"][0]["engine"] = "parakeet".into()).is_err());
    }
}
