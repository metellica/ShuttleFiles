//! Checksums for the selected files.
//!
//! Both digests are fed from one pass over the file: reading a multi-
//! gigabyte ISO twice to answer one dialog would be the slow part by far.
//! Progress is pushed as events and the work is cancellable, because a
//! file large enough to need a checksum is large enough to change your
//! mind about.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use md5::Md5;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

/// Large enough to keep the disk streaming, small enough that cancelling
/// still feels immediate.
const CHUNK: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgo {
    Md5,
    Sha256,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashResult {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub md5: String,
    pub sha256: String,
    /// Empty on success; the file is reported either way so one
    /// unreadable item does not hide the rest.
    pub error: String,
}

/// Emitted while a file is being read, so the dialog can show a bar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashProgress {
    pub id: String,
    pub path: String,
    /// 1-based position in the batch.
    pub index: usize,
    pub total: usize,
    pub done_bytes: u64,
    pub total_bytes: u64,
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{:02x}", b);
    }
    out
}

/// Hash one file, calling `on_progress` every chunk.
pub fn hash_file(
    path: &str,
    algos: &[HashAlgo],
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, u64),
) -> AppResult<(String, String)> {
    let file = std::fs::File::open(path)
        .map_err(|e| AppError::Io(format!("Cannot open {}: {}", path, e)))?;
    let total = file.metadata().map(|m| m.len()).unwrap_or(0);

    let want_md5 = algos.contains(&HashAlgo::Md5);
    let want_sha = algos.contains(&HashAlgo::Sha256);
    let mut md5 = want_md5.then(Md5::new);
    let mut sha = want_sha.then(Sha256::new);

    let mut reader = std::io::BufReader::with_capacity(CHUNK, file);
    let mut buffer = vec![0u8; CHUNK];
    let mut done = 0u64;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|e| AppError::Io(format!("Cannot read {}: {}", path, e)))?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        if let Some(h) = md5.as_mut() {
            h.update(chunk);
        }
        if let Some(h) = sha.as_mut() {
            h.update(chunk);
        }
        done += read as u64;
        on_progress(done, total);
    }

    Ok((
        md5.map(|h| to_hex(&h.finalize())).unwrap_or_default(),
        sha.map(|h| to_hex(&h.finalize())).unwrap_or_default(),
    ))
}

/// Hash a batch, reporting each file as it completes. Returns the results
/// gathered before a cancellation, so partial work is not thrown away.
pub fn hash_batch(
    id: &str,
    paths: &[String],
    algos: &[HashAlgo],
    cancel: Arc<AtomicBool>,
    mut on_progress: impl FnMut(HashProgress),
    mut on_result: impl FnMut(HashResult),
) -> bool {
    let total = paths.len();
    for (i, path) in paths.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return true;
        }
        let name = crate::fs::path::display_name(path);
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let mut last_emit = std::time::Instant::now();

        let outcome = hash_file(path, algos, &cancel, |done, total_bytes| {
            // Throttled: a fast SSD would otherwise flood the event loop
            // with thousands of updates per second.
            if last_emit.elapsed() >= std::time::Duration::from_millis(80) {
                last_emit = std::time::Instant::now();
                on_progress(HashProgress {
                    id: id.to_string(),
                    path: path.clone(),
                    index: i + 1,
                    total,
                    done_bytes: done,
                    total_bytes,
                });
            }
        });

        match outcome {
            Ok((md5, sha256)) => on_result(HashResult {
                path: path.clone(),
                name,
                size,
                md5,
                sha256,
                error: String::new(),
            }),
            Err(AppError::Cancelled) => return true,
            Err(e) => on_result(HashResult {
                path: path.clone(),
                name,
                size,
                md5: String::new(),
                sha256: String::new(),
                error: e.to_string(),
            }),
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("shuttle-files-hash-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn file(&self, name: &str, body: &[u8]) -> String {
            let p = self.0.join(name);
            std::fs::write(&p, body).unwrap();
            p.to_string_lossy().to_string()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const BOTH: &[HashAlgo] = &[HashAlgo::Md5, HashAlgo::Sha256];

    fn hash(path: &str, algos: &[HashAlgo]) -> (String, String) {
        hash_file(path, algos, &AtomicBool::new(false), |_, _| {}).unwrap()
    }

    #[test]
    fn known_vectors_match_the_reference_digests() {
        let tmp = TempDir::new();
        let path = tmp.file("abc.txt", b"abc");
        let (md5, sha256) = hash(&path, BOTH);
        assert_eq!(md5, "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn an_empty_file_hashes_to_the_empty_digests() {
        let tmp = TempDir::new();
        let path = tmp.file("empty.bin", b"");
        let (md5, sha256) = hash(&path, BOTH);
        assert_eq!(md5, "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_file_larger_than_one_chunk_is_hashed_across_reads() {
        let tmp = TempDir::new();
        let body = vec![b'a'; CHUNK * 2 + 12345];
        let path = tmp.file("big.bin", &body);
        let (_, sha256) = hash(&path, &[HashAlgo::Sha256]);

        let mut expected = Sha256::new();
        expected.update(&body);
        assert_eq!(sha256, to_hex(&expected.finalize()));
    }

    #[test]
    fn only_the_requested_digests_are_computed() {
        let tmp = TempDir::new();
        let path = tmp.file("one.txt", b"abc");
        let (md5, sha256) = hash(&path, &[HashAlgo::Md5]);
        assert_eq!(md5, "900150983cd24fb0d6963f7d28e17f72");
        assert!(sha256.is_empty());
    }

    #[test]
    fn cancelling_aborts_instead_of_returning_a_wrong_digest() {
        let tmp = TempDir::new();
        let path = tmp.file("big.bin", &vec![b'x'; CHUNK * 2]);
        let cancel = AtomicBool::new(true);
        let result = hash_file(&path, BOTH, &cancel, |_, _| {});
        assert!(matches!(result, Err(AppError::Cancelled)));
    }

    #[test]
    fn an_unreadable_file_is_reported_per_item_not_fatal() {
        let tmp = TempDir::new();
        let good = tmp.file("good.txt", b"abc");
        let missing = format!("{}\\nope.txt", tmp.0.to_string_lossy());

        let mut results = Vec::new();
        let cancelled = hash_batch(
            "id",
            &[missing, good],
            BOTH,
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |r| results.push(r),
        );

        assert!(!cancelled);
        assert_eq!(results.len(), 2);
        assert!(!results[0].error.is_empty());
        assert!(results[1].error.is_empty());
        assert_eq!(results[1].md5, "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn a_batch_reports_progress_and_size() {
        let tmp = TempDir::new();
        let path = tmp.file("big.bin", &vec![b'z'; CHUNK * 3]);
        let mut results = Vec::new();
        hash_batch(
            "id",
            std::slice::from_ref(&path),
            BOTH,
            Arc::new(AtomicBool::new(false)),
            |_| {},
            |r| results.push(r),
        );
        assert_eq!(results[0].size, (CHUNK * 3) as u64);
        assert_eq!(results[0].name, "big.bin");
    }
}
