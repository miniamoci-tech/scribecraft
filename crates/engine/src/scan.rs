//! Folder intake: find recordings that are not in the job list yet.

use std::path::{Path, PathBuf};

use crate::{EngineError, JobList, Result};

/// Deepest sub-folder level visited.
pub const MAX_SCAN_DEPTH: usize = 8;
/// Most directory entries examined in one scan.
pub const MAX_SCAN_FILES: usize = 100_000;

/// Lists supported recordings under `dir` (up to `depth` levels of sub-folders, capped at
/// [`MAX_SCAN_DEPTH`]) that `jobs` does not contain yet, sorted by path. Symbolic links are
/// not followed, so link cycles cannot loop. Unreadable sub-folders are skipped.
pub fn scan_folder(dir: &Path, depth: usize, jobs: &JobList) -> Result<Vec<PathBuf>> {
    let depth = depth.min(MAX_SCAN_DEPTH);
    let mut found = Vec::new();
    let mut examined = 0usize;
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    let root_entries = std::fs::read_dir(dir).map_err(|source| EngineError::Io { path: dir.display().to_string(), source })?;
    drop(root_entries);
    while let Some((folder, level)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            examined += 1;
            if examined > MAX_SCAN_FILES {
                return Err(EngineError::Invalid(format!("{} holds more than {MAX_SCAN_FILES} entries; choose a smaller folder", dir.display())));
            }
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                if level < depth {
                    stack.push((path, level + 1));
                }
            } else if kind.is_file() && scribecraft_media::is_supported_path(&path) && !jobs.contains_path(&path) && !is_partial(&path) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Files still being written by an uploader or a recorder dock are skipped.
fn is_partial(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.') || n.starts_with('~'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dictation;

    #[test]
    fn finds_new_supported_files_within_depth() {
        let root = std::env::temp_dir().join(format!("scribecraft-scan-{}", std::process::id()));
        let deep = root.join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        for f in ["one.wav", "two.MP3", "notes.docx", ".partial.wav"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        std::fs::write(root.join("a").join("three.m4a"), b"x").unwrap();
        std::fs::write(deep.join("four.flac"), b"x").unwrap();
        let mut jobs = JobList::default();
        jobs.add(Dictation { path: root.join("one.wav"), ..Default::default() }).unwrap();
        let names = |v: Vec<PathBuf>| v.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(names(scan_folder(&root, 0, &jobs).unwrap()), vec!["two.MP3"]);
        assert_eq!(names(scan_folder(&root, 1, &jobs).unwrap()), vec!["three.m4a", "two.MP3"]);
        assert_eq!(names(scan_folder(&root, 99, &jobs).unwrap()).len(), 3);
        assert!(scan_folder(&root.join("missing"), 1, &jobs).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
