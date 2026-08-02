//! ZIP, through the `zip` crate: deflate via `zlib-rs`, plus deflate64,
//! bzip2, zstd, lzma/xz and AES-encrypted members on the read side.

use std::io::{Read, Seek, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::{io_error, normalise_inner, Entry, Member, ProgressReader, Selection};
use crate::error::{AppError, AppResult};
use crate::ops::engine::Progress;

/// Zip records an MS-DOS date rather than an epoch timestamp.
fn modified_of(file: &zip::read::ZipFile<'_, impl Read>) -> u64 {
    file.last_modified()
        .map(|t| {
            super::unix_from_civil(
                t.year() as i64,
                t.month() as i64,
                t.day() as i64,
                t.hour() as u64,
                t.minute() as u64,
                t.second() as u64,
            )
        })
        .unwrap_or(0)
}

fn open(archive: &Path) -> AppResult<ZipArchive<std::io::BufReader<std::fs::File>>> {
    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::Io(format!("Cannot open {}: {}", archive.display(), e)))?;
    ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))
}

pub fn list(archive: &Path) -> AppResult<Vec<Entry>> {
    let mut zip = open(archive)?;
    let mut entries = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let file = zip
            .by_index(i)
            .map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))?;
        let Some(path) = normalise_inner(file.name()) else {
            continue;
        };
        entries.push(Entry {
            path,
            is_dir: file.is_dir(),
            size: file.size(),
            packed: file.compressed_size(),
            modified: modified_of(&file),
        });
    }
    Ok(entries)
}

pub fn extract(
    archive: &Path,
    selection: &Selection,
    dest: &Path,
    progress: &dyn Progress,
) -> AppResult<()> {
    let mut zip = open(archive)?;
    for i in 0..zip.len() {
        progress.check_cancel()?;
        let mut file = zip
            .by_index(i)
            .map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))?;
        let Some(inner) = normalise_inner(file.name()) else {
            continue;
        };
        let Some(relative) = selection.output_for(&inner) else {
            continue;
        };
        if file.is_dir() {
            let dir = dest.join(&relative);
            std::fs::create_dir_all(&dir)
                .map_err(|e| AppError::Io(format!("Cannot create {}: {}", dir.display(), e)))?;
            continue;
        }
        let modified = modified_of(&file);
        super::write_member(dest, &relative, &mut file, modified, progress)?;
    }
    Ok(())
}

/// `level` follows the UI scale: 0 stores, 1-9 deflate harder.
fn options(level: Option<i32>) -> SimpleFileOptions {
    let level = level.unwrap_or(6);
    if level <= 0 {
        return SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    }
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(level.clamp(1, 9) as i64))
}

pub fn create(
    archive: &Path,
    members: &[Member],
    level: Option<i32>,
    progress: &dyn Progress,
) -> AppResult<()> {
    let file = std::fs::File::create(archive)
        .map_err(|e| AppError::Io(format!("Cannot write {}: {}", archive.display(), e)))?;
    let mut zip = ZipWriter::new(std::io::BufWriter::new(file));
    let base = options(level);

    for member in members {
        progress.check_cancel()?;
        // Zip names are `/`-separated regardless of platform.
        let name = member.inner.replace('\\', "/");
        if member.is_dir {
            zip.add_directory(name, base)
                .map_err(|e| AppError::Io(format!("Cannot add {}: {}", member.inner, e)))?;
            continue;
        }

        progress.set_current(&member.inner);
        // Sizes past 4 GiB need the zip64 record, which has to be
        // requested before the member is written.
        zip.start_file(name, base.large_file(member.size >= u32::MAX as u64))
            .map_err(|e| AppError::Io(format!("Cannot add {}: {}", member.inner, e)))?;

        let mut source = std::fs::File::open(&member.source)
            .map_err(|e| AppError::Io(format!("Cannot read {}: {}", member.source.display(), e)))?;
        let mut reader = ProgressReader::new(&mut source, progress);
        std::io::copy(&mut reader, &mut zip).map_err(|e| io_error(&member.source, e))?;
        progress.file_done();
    }

    finish(zip, archive)
}

fn finish<W: Write + Seek>(zip: ZipWriter<W>, archive: &Path) -> AppResult<()> {
    zip.finish()
        .map_err(|e| AppError::Io(format!("Cannot finish {}: {}", archive.display(), e)))?;
    Ok(())
}

/// Read one member into memory. Used by nothing in the hot path, but it
/// keeps the trait bounds above honest.
#[allow(dead_code)]
pub fn read_member<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> AppResult<Vec<u8>> {
    let mut file = zip
        .by_name(name)
        .map_err(|e| AppError::Io(format!("Cannot read {}: {}", name, e)))?;
    let mut buf = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut buf)
        .map_err(|e| AppError::Io(format!("Cannot read {}: {}", name, e)))?;
    Ok(buf)
}
