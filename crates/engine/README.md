# scribecraft-engine (L3)

- `JobList`: dictations with author, priority, status, due date, notes, saved position and
  bookmarks; versioned JSON written atomically; a corrupt file is reported, never overwritten.
- `scan_folder`: new supported recordings under a folder (depth-capped, no symlink following).
- `Session`: the job list plus the open dictation's `Arc<Mutex<Transport>>`; resumes where
  the dictation was left; `lock_transport` recovers from lock poisoning.
