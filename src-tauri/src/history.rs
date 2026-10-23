//! The last transcriptions, kept locally. Audio is never stored.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MAX_ENTRIES: usize = 50;
pub const FILE_NAME: &str = "history.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub text: String,
    /// RFC 3339, local time.
    pub at: String,
    pub duration_ms: u64,
    pub model_id: String,
}

#[derive(Debug)]
pub struct History {
    path: PathBuf,
    /// Newest first.
    entries: VecDeque<Entry>,
}

impl History {
    /// Loads `dir/history.json`. A missing or unreadable file gives an empty
    /// history: losing it is better than refusing to start.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        let entries = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!("history unreadable, starting empty: {e}");
                VecDeque::new()
            }),
            Err(_) => VecDeque::new(),
        };
        Self { path, entries }
    }

    pub fn entries(&self) -> &VecDeque<Entry> {
        &self.entries
    }

    /// Adds `entry` (its `id` is assigned here) and drops the oldest beyond
    /// [`MAX_ENTRIES`].
    pub fn push(&mut self, mut entry: Entry) {
        entry.id = self.entries.front().map_or(1, |e| e.id + 1);
        self.entries.push_front(entry);
        self.entries.truncate(MAX_ENTRIES);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_vec(&self.entries).map_err(std::io::Error::other)?;
        crate::fsutil::write_atomic(&self.path, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> Entry {
        Entry {
            id: 0,
            text: text.into(),
            at: "2026-10-23T10:00:00+02:00".into(),
            duration_ms: 1_000,
            model_id: "m".into(),
        }
    }

    #[test]
    fn keeps_the_newest_entries_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::load(dir.path());
        for i in 0..MAX_ENTRIES + 5 {
            h.push(entry(&i.to_string()));
        }
        assert_eq!(h.entries().len(), MAX_ENTRIES);
        let last = (MAX_ENTRIES + 4).to_string();
        assert_eq!(h.entries()[0].text, last);
        assert_eq!(h.entries()[MAX_ENTRIES - 1].text, "5");
        assert!(h.entries()[0].id > h.entries()[1].id);
    }

    #[test]
    fn round_trips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = History::load(dir.path());
        h.push(entry("bonjour"));
        h.save().unwrap();
        let mut h = History::load(dir.path());
        assert_eq!(h.entries()[0].text, "bonjour");
        h.clear();
        h.save().unwrap();
        assert!(History::load(dir.path()).entries().is_empty());
    }

    #[test]
    fn corrupt_file_gives_empty_history() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), b"{not json").unwrap();
        assert!(History::load(dir.path()).entries().is_empty());
    }
}
