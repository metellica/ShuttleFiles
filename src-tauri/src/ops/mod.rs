//! Background file operations.
//!
//! Copy / move / delete run as cancellable jobs on blocking threads so
//! the UI keeps responding while a large tree is processed. Progress is
//! pushed to the frontend as throttled `fileop:update` events rather
//! than polled.

pub mod engine;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    Copy,
    Move,
    Delete,
    Extract,
    Compress,
}

impl JobKind {
    pub fn label(self) -> &'static str {
        match self {
            JobKind::Copy => "Copying",
            JobKind::Move => "Moving",
            JobKind::Delete => "Deleting",
            JobKind::Extract => "Extracting",
            JobKind::Compress => "Compressing",
        }
    }
}

/// Extra input for the archive jobs, which need more than a source list
/// and a destination folder.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobOptions {
    /// Compress: full path of the archive to create. Its extension
    /// picks the format.
    #[serde(default)]
    pub archive_path: String,
    /// Compress: 0 stores, 9 compresses hardest.
    #[serde(default)]
    pub level: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// Counting files to get a total before any work starts.
    Scanning,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
        )
    }
}

/// Snapshot sent to the frontend on every update.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    pub id: String,
    pub kind: JobKind,
    pub status: JobStatus,
    pub label: String,
    pub dest_dir: String,
    pub total_files: u64,
    pub done_files: u64,
    pub total_bytes: u64,
    pub done_bytes: u64,
    /// Name of the entry currently being processed.
    pub current: String,
    pub error: String,
    /// Sliding-window throughput; 0 for delete jobs.
    pub bytes_per_sec: u64,
}

impl JobState {
    fn new(id: String, kind: JobKind, dest_dir: String) -> Self {
        Self {
            id,
            kind,
            status: JobStatus::Scanning,
            label: kind.label().to_string(),
            dest_dir,
            total_files: 0,
            done_files: 0,
            total_bytes: 0,
            done_bytes: 0,
            current: String::new(),
            error: String::new(),
            bytes_per_sec: 0,
        }
    }
}

pub struct Job {
    pub state: Mutex<JobState>,
    cancel: AtomicBool,
}

impl Job {
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> JobState {
        self.state.lock().unwrap().clone()
    }
}

/// All jobs of this session, finished ones included until cleared.
#[derive(Default)]
pub struct OpsRegistry {
    jobs: Mutex<HashMap<String, Arc<Job>>>,
    /// Insertion order, so the UI lists jobs oldest-first.
    order: Mutex<Vec<String>>,
}

impl OpsRegistry {
    pub fn create(&self, kind: JobKind, dest_dir: String) -> Arc<Job> {
        let id = uuid::Uuid::new_v4().to_string();
        let job = Arc::new(Job {
            state: Mutex::new(JobState::new(id.clone(), kind, dest_dir)),
            cancel: AtomicBool::new(false),
        });
        self.jobs.lock().unwrap().insert(id.clone(), job.clone());
        self.order.lock().unwrap().push(id);
        job
    }

    pub fn get(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs.lock().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<JobState> {
        let jobs = self.jobs.lock().unwrap();
        self.order
            .lock()
            .unwrap()
            .iter()
            .filter_map(|id| jobs.get(id))
            .map(|j| j.snapshot())
            .collect()
    }

    /// Drop finished jobs; running ones are left alone.
    pub fn clear_finished(&self) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|_, job| !job.snapshot().status.is_finished());
        let ids = self.order.lock().unwrap().clone();
        *self.order.lock().unwrap() = ids.into_iter().filter(|id| jobs.contains_key(id)).collect();
    }
}
