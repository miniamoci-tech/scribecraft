//! Atomic file writes.

use std::io::Write;
use std::path::Path;

use crate::{EngineError, Result};

/// Writes `bytes` to `path` through a temporary file in the same directory, flushed to disk
/// and then renamed over `path`. Readers see the old file or the new one, never a torn one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let io = |source| EngineError::Io { path: path.display().to_string(), source };
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        // Best effort; the write error is what the caller needs.
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(io)
}
