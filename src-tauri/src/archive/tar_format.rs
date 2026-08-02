//! Tar and the compressed streams around it: gzip (`zlib-rs` backend),
//! bzip2, xz and zstd (multi-threaded).
//!
//! A `.tar.*` is one continuous stream, so listing and extraction both
//! read it front to back — there is no central directory to seek to.

use std::io::{Read, Write};
use std::path::Path;

use super::{io_error, normalise_inner, Entry, Format, Member, ProgressReader, Selection};
use crate::error::{AppError, AppResult};
use crate::ops::engine::Progress;

fn threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
        .clamp(1, 16)
}

/// Name a bare `.gz` / `.xz` / `.bz2` / `.zst` stream unpacks to.
fn stripped_name(archive: &Path) -> String {
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "data".to_string());
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => format!("{}.out", name),
    }
}

fn open(archive: &Path) -> AppResult<std::io::BufReader<std::fs::File>> {
    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::Io(format!("Cannot open {}: {}", archive.display(), e)))?;
    Ok(std::io::BufReader::with_capacity(1 << 20, file))
}

/// Wrap the archive file in the decoder its format needs.
fn decoder(archive: &Path, format: Format) -> AppResult<Box<dyn Read>> {
    let file = open(archive)?;
    Ok(match format {
        Format::Tar => Box::new(file),
        // "Multi" decoders keep reading past the first member, which is
        // what tools like `pigz` and `cat a.gz b.gz` produce.
        Format::TarGz | Format::Gz => Box::new(flate2::read::MultiGzDecoder::new(file)),
        Format::TarBz2 | Format::Bz2 => Box::new(bzip2::read::MultiBzDecoder::new(file)),
        Format::TarXz | Format::Xz => Box::new(liblzma::read::XzDecoder::new_multi_decoder(file)),
        Format::TarZst | Format::Zst => Box::new(
            zstd::stream::read::Decoder::new(file)
                .map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))?,
        ),
        other => {
            return Err(AppError::InvalidPath(format!(
                "Not a tar stream: .{}",
                other.extension()
            )))
        }
    })
}

pub fn list(archive: &Path, format: Format) -> AppResult<Vec<Entry>> {
    if format.is_single_stream() {
        // The uncompressed size is only known after inflating the whole
        // stream, which a listing must not pay for.
        return Ok(vec![Entry {
            path: stripped_name(archive),
            is_dir: false,
            size: 0,
            packed: std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0),
            modified: std::fs::metadata(archive)
                .and_then(|m| m.modified())
                .map(super::unix_seconds)
                .unwrap_or(0),
        }]);
    }

    let mut tar = tar::Archive::new(decoder(archive, format)?);
    let mut entries = Vec::new();
    for entry in tar
        .entries()
        .map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))?
    {
        let entry = entry.map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))?;
        let header = entry.header();
        let Ok(name) = entry.path() else { continue };
        let Some(path) = normalise_inner(&name.to_string_lossy()) else {
            continue;
        };
        entries.push(Entry {
            path,
            is_dir: header.entry_type().is_dir(),
            size: header.size().unwrap_or(0),
            packed: 0,
            modified: header.mtime().unwrap_or(0),
        });
    }
    Ok(entries)
}

pub fn extract(
    archive: &Path,
    format: Format,
    selection: &Selection,
    dest: &Path,
    progress: &dyn Progress,
) -> AppResult<()> {
    if format.is_single_stream() {
        let name = stripped_name(archive);
        let Some(relative) = selection.output_for(&name) else {
            return Ok(());
        };
        let mut reader = decoder(archive, format)?;
        return super::write_member(dest, &relative, &mut reader, 0, progress);
    }

    let mut tar = tar::Archive::new(decoder(archive, format)?);
    for entry in tar
        .entries()
        .map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))?
    {
        progress.check_cancel()?;
        let mut entry =
            entry.map_err(|e| AppError::Io(format!("Cannot read {}: {}", archive.display(), e)))?;
        let is_dir = entry.header().entry_type().is_dir();
        let modified = entry.header().mtime().unwrap_or(0);
        let Ok(name) = entry.path().map(|p| p.to_string_lossy().to_string()) else {
            continue;
        };
        let Some(inner) = normalise_inner(&name) else {
            continue;
        };
        let Some(relative) = selection.output_for(&inner) else {
            continue;
        };

        if is_dir {
            let dir = dest.join(&relative);
            std::fs::create_dir_all(&dir)
                .map_err(|e| AppError::Io(format!("Cannot create {}: {}", dir.display(), e)))?;
            continue;
        }
        super::write_member(dest, &relative, &mut entry, modified, progress)?;
    }
    Ok(())
}

/// The compressed sink a new archive is written through. Each encoder
/// needs its own `finish` to flush its trailer, hence the enum rather
/// than a `Box<dyn Write>`.
enum Sink<W: Write> {
    Plain(W),
    Gz(flate2::write::GzEncoder<W>),
    Bz(bzip2::write::BzEncoder<W>),
    Xz(liblzma::write::XzEncoder<W>),
    Zst(zstd::stream::write::Encoder<'static, W>),
}

impl<W: Write> Write for Sink<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Sink::Plain(w) => w.write(buf),
            Sink::Gz(w) => w.write(buf),
            Sink::Bz(w) => w.write(buf),
            Sink::Xz(w) => w.write(buf),
            Sink::Zst(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Sink::Plain(w) => w.flush(),
            Sink::Gz(w) => w.flush(),
            Sink::Bz(w) => w.flush(),
            Sink::Xz(w) => w.flush(),
            Sink::Zst(w) => w.flush(),
        }
    }
}

impl<W: Write> Sink<W> {
    fn new(writer: W, format: Format, level: Option<i32>) -> AppResult<Self> {
        let level = level.unwrap_or(6).clamp(0, 9) as u32;
        Ok(match format {
            Format::Tar => Sink::Plain(writer),
            Format::TarGz | Format::Gz => {
                Sink::Gz(flate2::write::GzEncoder::new(
                    writer,
                    flate2::Compression::new(level),
                ))
            }
            Format::TarBz2 | Format::Bz2 => Sink::Bz(bzip2::write::BzEncoder::new(
                writer,
                bzip2::Compression::new(level.clamp(1, 9)),
            )),
            Format::TarXz | Format::Xz => Sink::Xz(liblzma::write::XzEncoder::new(writer, level)),
            Format::TarZst | Format::Zst => {
                // zstd's own scale runs to 19 usefully; spread 0-9 over it.
                let mut encoder =
                    zstd::stream::write::Encoder::new(writer, (level as i32 * 2).max(1))
                        .map_err(|e| AppError::Io(format!("Cannot compress: {}", e)))?;
                let _ = encoder.multithread(threads());
                Sink::Zst(encoder)
            }
            other => {
                return Err(AppError::InvalidPath(format!(
                    "Cannot create .{}",
                    other.extension()
                )))
            }
        })
    }

    fn finish(self) -> AppResult<()> {
        let result = match self {
            Sink::Plain(mut w) => w.flush(),
            Sink::Gz(w) => w.finish().map(|_| ()),
            Sink::Bz(w) => w.finish().map(|_| ()),
            Sink::Xz(w) => w.finish().map(|_| ()),
            Sink::Zst(w) => w.finish().map(|_| ()),
        };
        result.map_err(|e| AppError::Io(format!("Cannot finish the archive: {}", e)))
    }
}

pub fn create(
    archive: &Path,
    format: Format,
    members: &[Member],
    level: Option<i32>,
    progress: &dyn Progress,
) -> AppResult<()> {
    let file = std::fs::File::create(archive)
        .map_err(|e| AppError::Io(format!("Cannot write {}: {}", archive.display(), e)))?;
    let sink = Sink::new(std::io::BufWriter::with_capacity(1 << 20, file), format, level)?;

    if format.is_single_stream() {
        return create_single(sink, members, progress);
    }

    let mut builder = tar::Builder::new(sink);
    for member in members {
        progress.check_cancel()?;
        // Tar names are `/`-separated regardless of platform.
        let name = member.inner.replace('\\', "/");
        if member.is_dir {
            builder
                .append_dir(&name, &member.source)
                .map_err(|e| AppError::Io(format!("Cannot add {}: {}", member.inner, e)))?;
            continue;
        }

        progress.set_current(&member.inner);
        let mut source = std::fs::File::open(&member.source)
            .map_err(|e| AppError::Io(format!("Cannot read {}: {}", member.source.display(), e)))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(member.size);
        header.set_mode(0o644);
        header.set_mtime(member.modified);
        header.set_cksum();

        let reader = ProgressReader::new(&mut source, progress);
        builder
            .append_data(&mut header, &name, reader)
            .map_err(|e| io_error(&member.source, e))?;
        progress.file_done();
    }

    let sink = builder
        .into_inner()
        .map_err(|e| AppError::Io(format!("Cannot finish {}: {}", archive.display(), e)))?;
    sink.finish()
}

/// A bare `.gz` / `.xz` / `.bz2` / `.zst` is the file's bytes and
/// nothing else: no member names, no directories.
fn create_single<W: Write>(
    mut sink: Sink<W>,
    members: &[Member],
    progress: &dyn Progress,
) -> AppResult<()> {
    let member = members
        .iter()
        .find(|m| !m.is_dir)
        .ok_or_else(|| AppError::InvalidPath("Nothing to compress".into()))?;

    progress.set_current(&member.inner);
    let mut source = std::fs::File::open(&member.source)
        .map_err(|e| AppError::Io(format!("Cannot read {}: {}", member.source.display(), e)))?;
    let mut reader = ProgressReader::new(&mut source, progress);
    std::io::copy(&mut reader, &mut sink).map_err(|e| io_error(&member.source, e))?;
    progress.file_done();
    sink.finish()
}
