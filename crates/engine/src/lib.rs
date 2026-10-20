//! The ScribeCraft engine (layer L3): the dictation job list, folder intake, bookmarks, and
//! the session that opens a recording into a [`Transport`].
//!
//! Nothing here touches an audio device or a window; `platform` and `ui-egui` sit above.

mod fsutil;
pub mod hotkeys;
mod jobs;
pub mod pedal;
mod scan;
mod session;

pub use fsutil::write_atomic;
pub use jobs::{Bookmark, Dictation, JobList, MAX_BOOKMARKS, MAX_TEXT_CHARS, Priority, Status};
pub use scan::{MAX_SCAN_DEPTH, MAX_SCAN_FILES, scan_folder};
pub use session::{Session, lock as lock_transport};

pub use scribecraft_media as media;
pub use scribecraft_transport as transport;
pub use scribecraft_transport::{Settings, State, Transport};

/// Errors from engine operations. Messages say what was wrong and what is allowed.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("cannot read or write {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("{path} is not a valid ScribeCraft job list: {detail}. The file was left untouched")]
    CorruptJobList { path: String, detail: String },
    #[error("no dictation with id {0}")]
    NoSuchDictation(u64),
    #[error("no dictation is open; open one first")]
    NothingOpen,
    #[error("{0} is already in the job list")]
    Duplicate(String),
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Media(#[from] scribecraft_media::MediaError),
    #[error(transparent)]
    Transport(#[from] scribecraft_transport::TransportError),
}

pub type Result<T, E = EngineError> = std::result::Result<T, E>;
