//! The dictation job list: what is waiting to be transcribed, by whom, and how urgently.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{EngineError, Result, write_atomic};

/// Current job-list file format.
const VERSION: u32 = 1;
/// Largest job-list file read (64 MiB is millions of dictations).
const MAX_JOBLIST_BYTES: u64 = 64 << 20;
/// Most bookmarks kept per dictation.
pub const MAX_BOOKMARKS: usize = 10_000;
/// Longest note or label kept, in characters.
pub const MAX_TEXT_CHARS: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
    Urgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Received, not started.
    #[default]
    New,
    /// Opened at least once.
    InProgress,
    /// Transcription finished.
    Done,
    /// Transcript sent back to the author.
    Dispatched,
}

impl Priority {
    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.into())).ok()
    }
}

impl Status {
    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.into())).ok()
    }
}

/// A named position in a recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub ms: u64,
    pub label: String,
}

/// One recording waiting for (or done with) transcription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dictation {
    pub id: u64,
    pub path: PathBuf,
    pub title: String,
    pub author: String,
    /// Seconds since the Unix epoch when it entered the list.
    pub received: u64,
    pub duration_ms: Option<u64>,
    pub priority: Priority,
    pub status: Status,
    /// Free-form due date as the user typed it (e.g. `2026-10-21`).
    pub due: String,
    pub notes: String,
    /// Where playback was when the dictation was last closed.
    pub position_ms: u64,
    pub bookmarks: Vec<Bookmark>,
}

impl Default for Dictation {
    fn default() -> Self {
        Self {
            id: 0,
            path: PathBuf::new(),
            title: String::new(),
            author: String::new(),
            received: 0,
            duration_ms: None,
            priority: Priority::Normal,
            status: Status::New,
            due: String::new(),
            notes: String::new(),
            position_ms: 0,
            bookmarks: Vec::new(),
        }
    }
}

impl Dictation {
    /// Adds a bookmark, keeping them sorted by time. Labels are truncated to [`MAX_TEXT_CHARS`].
    pub fn add_bookmark(&mut self, ms: u64, label: &str) -> Result<()> {
        if self.bookmarks.len() >= MAX_BOOKMARKS {
            return Err(EngineError::Invalid(format!("a dictation can hold at most {MAX_BOOKMARKS} bookmarks")));
        }
        let at = self.bookmarks.partition_point(|b| b.ms <= ms);
        self.bookmarks.insert(at, Bookmark { ms, label: truncate(label) });
        Ok(())
    }
}

/// Truncates to [`MAX_TEXT_CHARS`] characters (never inside a character).
pub(crate) fn truncate(text: &str) -> String {
    text.chars().take(MAX_TEXT_CHARS).collect()
}

/// The whole job list, as stored in `jobs.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobList {
    pub version: u32,
    next_id: u64,
    dictations: Vec<Dictation>,
}

impl Default for JobList {
    fn default() -> Self {
        Self { version: VERSION, next_id: 1, dictations: Vec::new() }
    }
}

impl JobList {
    /// Loads a job list. A missing file is an empty list; a corrupt one is an error that
    /// leaves the file untouched (it is never overwritten by an empty list).
    pub fn load(path: &Path) -> Result<Self> {
        let io = |source| EngineError::Io { path: path.display().to_string(), source };
        let corrupt = |detail: String| EngineError::CorruptJobList { path: path.display().to_string(), detail };
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(io(e)),
        };
        if meta.len() > MAX_JOBLIST_BYTES {
            return Err(corrupt(format!("it is {} bytes, more than the {MAX_JOBLIST_BYTES}-byte limit", meta.len())));
        }
        let bytes = std::fs::read(path).map_err(io)?;
        let mut list: JobList = serde_json::from_slice(&bytes).map_err(|e| corrupt(e.to_string()))?;
        if list.version > VERSION {
            return Err(corrupt(format!("it was written by a newer ScribeCraft (format {}, this build reads up to {VERSION})", list.version)));
        }
        // Repair what a hand edit could break: ids must be unique and below next_id.
        let mut seen = std::collections::HashSet::new();
        list.dictations.retain(|d| seen.insert(d.id));
        let max_id = list.dictations.iter().map(|d| d.id).max().unwrap_or(0);
        list.next_id = list.next_id.max(max_id.saturating_add(1));
        list.version = VERSION;
        Ok(list)
    }

    /// Saves atomically.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| EngineError::Invalid(e.to_string()))?;
        write_atomic(path, &json)
    }

    /// Adds a recording; refuses a path already in the list.
    pub fn add(&mut self, mut dictation: Dictation) -> Result<u64> {
        if self.dictations.iter().any(|d| d.path == dictation.path) {
            return Err(EngineError::Duplicate(dictation.path.display().to_string()));
        }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or_else(|| EngineError::Invalid("the job list has run out of ids".into()))?;
        dictation.id = id;
        dictation.title = truncate(&dictation.title);
        dictation.author = truncate(&dictation.author);
        dictation.notes = truncate(&dictation.notes);
        if dictation.received == 0 {
            dictation.received = now_secs();
        }
        self.dictations.push(dictation);
        Ok(id)
    }

    pub fn get(&self, id: u64) -> Result<&Dictation> {
        self.dictations.iter().find(|d| d.id == id).ok_or(EngineError::NoSuchDictation(id))
    }

    pub fn get_mut(&mut self, id: u64) -> Result<&mut Dictation> {
        self.dictations.iter_mut().find(|d| d.id == id).ok_or(EngineError::NoSuchDictation(id))
    }

    pub fn contains_path(&self, path: &Path) -> bool {
        self.dictations.iter().any(|d| d.path == path)
    }

    /// Removes a dictation from the list (the recording file itself is never touched).
    pub fn remove(&mut self, id: u64) -> Result<Dictation> {
        let at = self.dictations.iter().position(|d| d.id == id).ok_or(EngineError::NoSuchDictation(id))?;
        Ok(self.dictations.remove(at))
    }

    /// All dictations, most urgent first, then oldest first (the order work is done in).
    pub fn queue(&self) -> Vec<&Dictation> {
        let mut v: Vec<&Dictation> = self.dictations.iter().collect();
        v.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.received.cmp(&b.received)).then(a.id.cmp(&b.id)));
        v
    }

    pub fn len(&self) -> usize {
        self.dictations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dictations.is_empty()
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scribecraft-jobs-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("jobs.json")
    }

    #[test]
    fn add_save_load_round_trip_and_queue_order() {
        let path = temp("rt");
        let mut list = JobList::default();
        let a = list.add(Dictation { path: "a.wav".into(), received: 10, ..Default::default() }).unwrap();
        let b = list.add(Dictation { path: "b.wav".into(), received: 20, priority: Priority::Urgent, ..Default::default() }).unwrap();
        let c = list.add(Dictation { path: "c.wav".into(), received: 5, ..Default::default() }).unwrap();
        assert!(matches!(list.add(Dictation { path: "a.wav".into(), ..Default::default() }), Err(EngineError::Duplicate(_))));
        list.get_mut(a).unwrap().add_bookmark(5_000, "speaker 2").unwrap();
        list.get_mut(a).unwrap().add_bookmark(1_000, "start").unwrap();
        list.save(&path).unwrap();
        let back = JobList::load(&path).unwrap();
        assert_eq!(back, list);
        assert_eq!(back.queue().iter().map(|d| d.id).collect::<Vec<_>>(), vec![b, c, a]);
        assert_eq!(back.get(a).unwrap().bookmarks.first().unwrap().label, "start");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn missing_is_empty_and_corrupt_is_an_error_that_keeps_the_file() {
        let path = temp("corrupt");
        assert!(JobList::load(&path).unwrap().is_empty());
        std::fs::write(&path, b"{ not json").unwrap();
        let err = JobList::load(&path).unwrap_err().to_string();
        assert!(err.contains("left untouched"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
        std::fs::write(&path, br#"{"version": 99, "next_id": 1, "dictations": []}"#).unwrap();
        assert!(JobList::load(&path).unwrap_err().to_string().contains("newer ScribeCraft"));
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn hand_edited_duplicate_ids_are_repaired() {
        let path = temp("dup");
        std::fs::write(&path, br#"{"version":1,"next_id":1,"dictations":[{"id":7,"path":"x.wav"},{"id":7,"path":"y.wav"}]}"#).unwrap();
        let mut list = JobList::load(&path).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list.add(Dictation { path: "z.wav".into(), ..Default::default() }).unwrap(), 8);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn text_is_truncated_at_character_boundaries() {
        let mut d = Dictation::default();
        d.add_bookmark(0, &"é".repeat(MAX_TEXT_CHARS + 5)).unwrap();
        assert_eq!(d.bookmarks[0].label.chars().count(), MAX_TEXT_CHARS);
        assert_eq!(Priority::parse("urgent"), Some(Priority::Urgent));
        assert_eq!(Status::parse("in_progress"), Some(Status::InProgress));
        assert_eq!(Status::parse("bogus"), None);
    }
}
