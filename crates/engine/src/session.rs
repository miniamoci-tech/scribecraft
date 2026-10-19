//! A working session: the job list plus the one dictation open in the transport.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use scribecraft_media::{Limits, decode_file};
use scribecraft_transport::{Settings, Transport};

use crate::{Dictation, EngineError, JobList, Result, Status};

/// The job list and the dictation currently open for transcription.
///
/// The transport sits behind `Arc<Mutex<_>>` because the audio callback (in `platform`)
/// renders from it while the UI or automation drives it.
#[derive(Debug, Default)]
pub struct Session {
    jobs: JobList,
    jobs_path: Option<PathBuf>,
    dirty: bool,
    settings: Settings,
    limits: Limits,
    open: Option<(u64, Arc<Mutex<Transport>>)>,
}

/// Locks a transport, recovering from a poisoned lock: the transport's state is always
/// valid between calls, so a panic elsewhere must not take playback down with it.
pub fn lock(transport: &Mutex<Transport>) -> MutexGuard<'_, Transport> {
    transport.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Session {
    /// A session with an in-memory job list.
    pub fn new(settings: Settings) -> Result<Self> {
        settings.validate()?;
        Ok(Self { settings, ..Self::default() })
    }

    /// A session whose job list lives in `path` (loaded now, written by [`Session::save`]).
    pub fn with_job_file(path: &Path, settings: Settings) -> Result<Self> {
        let mut s = Self::new(settings)?;
        s.jobs = JobList::load(path)?;
        s.jobs_path = Some(path.to_path_buf());
        Ok(s)
    }

    pub fn jobs(&self) -> &JobList {
        &self.jobs
    }

    /// Mutable access to the job list; marks the session as having unsaved changes.
    pub fn jobs_mut(&mut self) -> &mut JobList {
        self.dirty = true;
        &mut self.jobs
    }

    /// Whether the job list has changes not yet saved.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn job_file(&self) -> Option<&Path> {
        self.jobs_path.as_deref()
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    /// Replaces playback settings, applying them to the open dictation too.
    pub fn set_settings(&mut self, settings: Settings) -> Result<()> {
        settings.validate()?;
        if let Some((_, t)) = &self.open {
            lock(t).set_settings(settings.clone())?;
        }
        self.settings = settings;
        Ok(())
    }

    /// Adds a recording to the job list.
    pub fn add_file(&mut self, path: &Path, title: Option<&str>, author: &str) -> Result<u64> {
        let title = title.map(str::to_owned).unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        let d = Dictation { path: path.to_path_buf(), title, author: author.to_owned(), ..Default::default() };
        let id = self.jobs.add(d)?;
        self.dirty = true;
        Ok(id)
    }

    /// Opens dictation `id`: decodes it, resumes at its saved position and marks it in
    /// progress. Any dictation already open is closed first (saving its position).
    pub fn open(&mut self, id: u64) -> Result<Arc<Mutex<Transport>>> {
        let path = self.jobs.get(id)?.path.clone();
        let audio = decode_file(&path, &self.limits)?;
        if !audio.repairs.is_empty() {
            log::warn!("dictation {id}: {} damaged section(s) skipped while decoding", audio.repairs.len());
        }
        let mut transport = Transport::new(Arc::new(audio), self.settings.clone())?;
        self.close();
        let job = self.jobs.get_mut(id)?;
        transport.seek_ms(job.position_ms);
        job.duration_ms = Some(transport.duration_ms());
        if job.status == Status::New {
            job.status = Status::InProgress;
        }
        self.dirty = true;
        let shared = Arc::new(Mutex::new(transport));
        self.open = Some((id, shared.clone()));
        Ok(shared)
    }

    /// Id of the open dictation.
    pub fn open_id(&self) -> Option<u64> {
        self.open.as_ref().map(|(id, _)| *id)
    }

    /// The open dictation's transport.
    pub fn transport(&self) -> Result<Arc<Mutex<Transport>>> {
        self.open.as_ref().map(|(_, t)| t.clone()).ok_or(EngineError::NothingOpen)
    }

    /// Runs `f` on the open transport.
    pub fn with_transport<R>(&self, f: impl FnOnce(&mut Transport) -> R) -> Result<R> {
        let t = self.transport()?;
        let mut guard = lock(&t);
        Ok(f(&mut guard))
    }

    /// Adds a bookmark at the current position of the open dictation.
    pub fn bookmark_here(&mut self, label: &str) -> Result<u64> {
        let (id, ms) = match &self.open {
            Some((id, t)) => (*id, lock(t).position_ms()),
            None => return Err(EngineError::NothingOpen),
        };
        self.jobs.get_mut(id)?.add_bookmark(ms, label)?;
        self.dirty = true;
        Ok(ms)
    }

    /// Closes the open dictation, remembering where playback was.
    pub fn close(&mut self) {
        if let Some((id, t)) = self.open.take() {
            let ms = lock(&t).position_ms();
            if let Ok(job) = self.jobs.get_mut(id) {
                job.position_ms = ms;
                self.dirty = true;
            }
        }
    }

    /// Writes the job list to its file, if it has one, remembering the open position too.
    pub fn save(&mut self) -> Result<()> {
        if let Some((id, t)) = &self.open {
            let ms = lock(t).position_ms();
            if let Ok(job) = self.jobs.get_mut(*id) {
                job.position_ms = ms;
            }
        }
        if let Some(path) = &self.jobs_path {
            self.jobs.save(path)?;
            self.dirty = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scribecraft_media::{WavFormat, synth, write_wav};

    #[test]
    fn open_resume_bookmark_and_save() {
        let dir = std::env::temp_dir().join(format!("scribecraft-session-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("interview.wav");
        write_wav(&wav, &synth::dictation(8_000, 2, 3.0), WavFormat::Pcm16).unwrap();
        let jobs = dir.join("jobs.json");
        let mut s = Session::with_job_file(&jobs, Settings::default()).unwrap();
        let id = s.add_file(&wav, None, "Author A").unwrap();
        assert!(s.is_dirty());
        s.open(id).unwrap();
        s.with_transport(|t| t.seek_ms(2_000)).unwrap();
        assert_eq!(s.bookmark_here("check this word").unwrap(), 2_000);
        s.save().unwrap();
        assert!(!s.is_dirty());

        let mut again = Session::with_job_file(&jobs, Settings::default()).unwrap();
        let job = again.jobs().get(id).unwrap();
        assert_eq!((job.status, job.position_ms, job.duration_ms, job.title.as_str()), (Status::InProgress, 2_000, Some(3_000), "interview"));
        again.open(id).unwrap();
        assert_eq!(again.with_transport(|t| t.position_ms()).unwrap(), 2_000);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn errors_are_actionable() {
        let mut s = Session::new(Settings::default()).unwrap();
        assert!(matches!(s.open(42), Err(EngineError::NoSuchDictation(42))));
        assert!(matches!(s.bookmark_here("x"), Err(EngineError::NothingOpen)));
        let id = s.add_file(Path::new("does-not-exist.wav"), Some("t"), "").unwrap();
        assert!(matches!(s.open(id), Err(EngineError::Media(_))));
        assert_eq!(s.open_id(), None);
    }
}
