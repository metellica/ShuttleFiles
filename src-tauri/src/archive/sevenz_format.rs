//! 7z, through `sevenz-rust2`: LZMA/LZMA2 (multi-threaded both ways),
//! bzip2, deflate, zstd, lz4 and PPMd.
//!
//! A 7z archive is usually *solid* — members share one compressed
//! block — so extraction always streams the blocks in order and picks
//! out the selected members on the way past, rather than seeking to
//! each one and decoding its block again.

use std::path::Path;

use sevenz_rust2::encoder_options::Lzma2Options;
use sevenz_rust2::{Archive, ArchiveEntry, ArchiveReader, ArchiveWriter, Password};

use super::{normalise_inner, Entry, Member, ProgressReader, Selection};
use crate::error::{AppError, AppResult};
use crate::ops::engine::Progress;

fn threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
        .clamp(1, 16)
}

pub fn list(archive: &Path) -> AppResult<Vec<Entry>> {
    let meta = Archive::open(archive)
        .map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))?;
    Ok(meta
        .files
        .iter()
        .filter_map(|file| {
            Some(Entry {
                path: normalise_inner(&file.name)?,
                is_dir: file.is_directory,
                size: file.size,
                packed: file.compressed_size,
                modified: if file.has_last_modified_date {
                    super::unix_seconds(file.last_modified_date.into())
                } else {
                    0
                },
            })
        })
        .collect())
}

pub fn extract(
    archive: &Path,
    selection: &Selection,
    dest: &Path,
    progress: &dyn Progress,
) -> AppResult<()> {
    let mut reader = ArchiveReader::open(archive, Password::empty())
        .map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))?;
    reader.set_thread_count(threads());

    let mut failure: Option<AppError> = None;
    let outcome = reader.for_each_entries(|entry, stream| {
        if progress.is_cancelled() {
            return Ok(false);
        }
        let Some(inner) = normalise_inner(&entry.name) else {
            return Ok(true);
        };
        let Some(relative) = selection.output_for(&inner) else {
            return Ok(true);
        };
        if entry.is_directory {
            let dir = dest.join(&relative);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                failure = Some(AppError::Io(format!(
                    "Cannot create {}: {}",
                    dir.display(),
                    e
                )));
                return Ok(false);
            }
            return Ok(true);
        }

        let modified = if entry.has_last_modified_date {
            super::unix_seconds(entry.last_modified_date.into())
        } else {
            0
        };
        match super::write_member(dest, &relative, stream, modified, progress) {
            Ok(()) => Ok(true),
            Err(e) => {
                failure = Some(e);
                Ok(false)
            }
        }
    });

    if let Some(e) = failure {
        return Err(e);
    }
    outcome.map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))?;
    progress.check_cancel()
}

pub fn create(
    archive: &Path,
    members: &[Member],
    level: Option<i32>,
    progress: &dyn Progress,
) -> AppResult<()> {
    let mut writer = ArchiveWriter::create(archive)
        .map_err(|e| AppError::Io(format!("Cannot write {}: {}", archive.display(), e)))?;

    let level = level.unwrap_or(6).clamp(0, 9) as u32;
    // LZMA2 is the only 7z encoder that compresses on several threads;
    // the chunk size trades a little ratio for that parallelism.
    writer.set_content_methods(vec![
        Lzma2Options::from_level_mt(level, threads(), 8 << 20).into()
    ]);

    for member in members {
        progress.check_cancel()?;
        if member.is_dir {
            writer
                .push_archive_entry::<&[u8]>(ArchiveEntry::new_directory(&member.inner), None)
                .map_err(|e| AppError::Io(format!("Cannot add {}: {}", member.inner, e)))?;
            continue;
        }

        progress.set_current(&member.inner);
        let mut source = std::fs::File::open(&member.source)
            .map_err(|e| AppError::Io(format!("Cannot read {}: {}", member.source.display(), e)))?;
        let mut entry = ArchiveEntry::new_file(&member.inner);
        entry.size = member.size;
        writer
            .push_archive_entry(entry, Some(ProgressReader::new(&mut source, progress)))
            .map_err(|e| AppError::Io(format!("Cannot add {}: {}", member.inner, e)))?;
        progress.file_done();
    }

    writer
        .finish()
        .map_err(|e| AppError::Io(format!("Cannot finish {}: {}", archive.display(), e)))?;
    Ok(())
}
