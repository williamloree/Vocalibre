pub mod download;
pub mod manifest;

use std::path::Path;

use manifest::{Manifest, ModelInfo, RemoteFile};

/// Present with the expected size. The full SHA-256 is checked when
/// downloading; this cheaply catches truncated or partial copies.
pub fn is_installed(dir: &Path, file: &RemoteFile) -> bool {
    std::fs::metadata(dir.join(&file.file_name)).is_ok_and(|m| m.is_file() && m.len() == file.size)
}

/// Usable models, in manifest order. None without the VAD model: dictation
/// never runs without it.
pub fn installed<'a>(manifest: &'a Manifest, dir: &Path) -> Vec<&'a ModelInfo> {
    if !is_installed(dir, &manifest.vad) {
        return Vec::new();
    }
    manifest
        .models
        .iter()
        .filter(|m| is_installed(dir, &m.file))
        .collect()
}

/// The preferred model if usable, otherwise the first installed one.
pub fn pick(manifest: &Manifest, dir: &Path, preferred: Option<&str>) -> Option<String> {
    let installed = installed(manifest, dir);
    preferred
        .and_then(|id| installed.iter().find(|m| m.id == id))
        .or(installed.first())
        .map(|m| m.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Embedded manifest with 3-byte sizes, and an empty directory.
    fn setup() -> (Manifest, tempfile::TempDir) {
        let mut m = Manifest::embedded().unwrap();
        m.vad.size = 3;
        for model in &mut m.models {
            model.file.size = 3;
        }
        (m, tempfile::tempdir().unwrap())
    }

    fn file(m: &Manifest, id: &str) -> String {
        m.get(id).unwrap().file.file_name.clone()
    }

    #[test]
    fn nothing_is_installed_without_the_vad() {
        let (m, dir) = setup();
        let base = file(&m, "whisper-base-q5_1");
        std::fs::write(dir.path().join(base), b"abc").unwrap();
        assert!(installed(&m, dir.path()).is_empty());
        assert_eq!(pick(&m, dir.path(), None), None);
    }

    #[test]
    fn wrong_size_is_not_installed() {
        let (m, dir) = setup();
        std::fs::write(dir.path().join(&m.vad.file_name), b"abcd").unwrap();
        assert!(!is_installed(dir.path(), &m.vad));
    }

    #[test]
    fn pick_prefers_the_setting_then_manifest_order() {
        let (m, dir) = setup();
        for f in [
            m.vad.file_name.clone(),
            file(&m, "whisper-base-q5_1"),
            file(&m, "whisper-small-q5_1"),
        ] {
            std::fs::write(dir.path().join(f), b"abc").unwrap();
        }
        let p = dir.path();
        assert_eq!(
            pick(&m, p, Some("whisper-base-q5_1")).as_deref(),
            Some("whisper-base-q5_1")
        );
        // Small comes before base in the manifest.
        assert_eq!(pick(&m, p, None).as_deref(), Some("whisper-small-q5_1"));
        assert_eq!(
            pick(&m, p, Some("whisper-large-v3-turbo-q5_0")).as_deref(),
            Some("whisper-small-q5_1")
        );
    }
}
