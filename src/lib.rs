//! A virtual filesystem whose bytes live in the RGB channels of a PNG image.
//!
//! Each successful mutation is persisted before it returns. Keep a writable
//! [`Disk`] open to hold its writer lock, or use [`Disk::open_read_only`] for a
//! snapshot that requires no lock file and cannot mutate the image.
//!
//! ```no_run
//! use dpegfs::{Compression, Disk};
//! # fn main() -> anyhow::Result<()> {
//! let mut disk = Disk::create("disk.png", 1024, 1024)?;
//! disk.put("notes/hello.txt", b"Hello, pixels!", Compression::Auto, false)?;
//! assert_eq!(disk.read("notes/hello.txt")?, b"Hello, pixels!");
//! disk.verify()?;
//! # Ok(())
//! # }
//! ```

mod codec;
mod storage;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use flate2::write::ZlibEncoder;
use serde::{Deserialize, Serialize};

/// Maximum decoded pixel capacity in this in-memory implementation: 256 MiB.
pub const MAX_CAPACITY: u64 = 256 * 1024 * 1024;
/// Maximum uncompressed size of a single stored file: 256 MiB.
pub const MAX_FILE_SIZE: u64 = 256 * 1024 * 1024;
/// Maximum width or height in pixels.
pub const MAX_DIMENSION: u32 = 16_384;
/// Maximum number of files in a volume.
pub const MAX_FILES: usize = 10_000;

/// File compression policy. The PNG container always uses lossless compression.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Compression {
    /// Use zlib only when it is smaller than the original file.
    #[default]
    Auto,
    /// Store the original file bytes directly in the pixel stream.
    None,
}

/// Encoding actually used for a stored file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoding {
    Raw,
    Zlib,
}

impl std::fmt::Display for Encoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::Raw => "raw",
            Self::Zlib => "zlib",
        })
    }
}

/// Metadata returned by [`Disk::list`]. Sizes are bytes, not filesystem blocks.
#[derive(Clone, Debug, Serialize)]
pub struct FileInfo {
    pub path: String,
    pub original_bytes: u64,
    pub stored_bytes: u64,
    pub encoding: Encoding,
    pub sha256: String,
}

/// Distinguishes logical pixel capacity from the PNG's physical file length.
#[derive(Clone, Debug, Serialize)]
pub struct DiskInfo {
    pub format_version: u32,
    pub width: u32,
    pub height: u32,
    pub files: usize,
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub metadata_bytes: u64,
    pub original_bytes: u64,
    pub stored_bytes: u64,
    pub png_bytes: u64,
}

#[derive(Clone)]
pub(crate) struct StoredFile {
    pub original_bytes: u64,
    pub encoding: Encoding,
    pub sha256: String,
    pub data: Arc<[u8]>,
}

#[derive(Clone)]
pub(crate) struct Image {
    pub width: u32,
    pub height: u32,
    pub files: BTreeMap<String, StoredFile>,
}

/// An image filesystem. Mutations use an exclusive writer lock and atomic saves.
///
/// A read-only handle is a snapshot: reopen it to see subsequent writes. A writer
/// lock is released when the handle is dropped. No file data lives in the lock
/// file; it may be left alongside the PNG between runs.
pub struct Disk {
    path: PathBuf,
    image: Image,
    png_bytes: u64,
    writer: Option<storage::WriterLock>,
}

impl Disk {
    /// Create a new RGB8 PNG disk. Never replaces an existing host file.
    pub fn create(path: impl AsRef<Path>, width: u32, height: u32) -> Result<Self> {
        pixel_capacity(width, height)?;
        let path = storage::normalize_path(path.as_ref())?;
        let writer = storage::WriterLock::acquire(&path)?;
        let image = Image {
            width,
            height,
            files: BTreeMap::new(),
        };
        let pixels = codec::encode(&image)?;
        let png_bytes = storage::atomic_write(&path, false, |out| {
            codec::write_png(out, width, height, &pixels)
        })?;
        Ok(Self {
            path,
            image,
            png_bytes,
            writer: Some(writer),
        })
    }

    /// Open a disk for reading and writing, holding its writer lock until drop.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_impl(path.as_ref(), true)
    }

    /// Open an immutable snapshot without creating a lock file.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_impl(path.as_ref(), false)
    }

    fn open_impl(path: &Path, writable: bool) -> Result<Self> {
        let path = storage::normalize_path(path)?;
        storage::check_regular_file(&path)?;
        let writer = if writable {
            storage::reject_hard_links(&path)?;
            Some(storage::WriterLock::acquire(&path)?)
        } else {
            None
        };
        let file = File::open(&path).with_context(|| format!("Cannot open {}", path.display()))?;
        let png_bytes = file.metadata()?.len();
        ensure!(
            png_bytes <= MAX_CAPACITY + codec::MAX_MANIFEST_BYTES as u64,
            "PNG exceeds the supported encoded file size limit"
        );
        let image = codec::read_png(file)?;
        Ok(Self {
            path,
            image,
            png_bytes,
            writer,
        })
    }

    /// Return a sorted list of files. Directory components are implicit.
    pub fn list(&self) -> Vec<FileInfo> {
        self.image
            .files
            .iter()
            .map(|(path, file)| FileInfo {
                path: path.clone(),
                original_bytes: file.original_bytes,
                stored_bytes: file.data.len() as u64,
                encoding: file.encoding,
                sha256: file.sha256.clone(),
            })
            .collect()
    }

    /// Report capacity, metadata overhead, file compression and PNG file length.
    pub fn info(&self) -> Result<DiskInfo> {
        let manifest_bytes = codec::manifest(&self.image)?.len() as u64;
        let stored_bytes = self.image.files.values().map(|f| f.data.len() as u64).sum();
        let metadata_bytes = codec::HEADER_BYTES as u64 + manifest_bytes;
        let used_bytes = metadata_bytes + stored_bytes;
        let capacity_bytes = pixel_capacity(self.image.width, self.image.height)?;
        Ok(DiskInfo {
            format_version: codec::VERSION,
            width: self.image.width,
            height: self.image.height,
            files: self.image.files.len(),
            capacity_bytes,
            used_bytes,
            free_bytes: capacity_bytes - used_bytes,
            metadata_bytes,
            original_bytes: self.image.files.values().map(|f| f.original_bytes).sum(),
            stored_bytes,
            png_bytes: self.png_bytes,
        })
    }

    /// Read and decompress one file, checking its original size and SHA-256.
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        validate_name(name)?;
        let file = self
            .image
            .files
            .get(name)
            .with_context(|| format!("File not found: {name}"))?;
        codec::decode_file(file).with_context(|| format!("Cannot read {name}"))
    }

    /// Add a file, or explicitly replace one. Persists before returning success.
    ///
    /// Capacity includes the file table. A rejected write leaves the previous
    /// image and this handle unchanged. Auto compression never stores a larger
    /// compressed payload than the original input.
    pub fn put(
        &mut self,
        name: &str,
        bytes: &[u8],
        compression: Compression,
        overwrite: bool,
    ) -> Result<()> {
        self.ensure_writable()?;
        validate_name(name)?;
        ensure!(
            bytes.len() as u64 <= MAX_FILE_SIZE,
            "File exceeds the 256 MiB input limit"
        );
        ensure!(
            overwrite || !self.image.files.contains_key(name),
            "File already exists: {name}; use overwrite to replace it"
        );
        ensure!(
            self.image.files.contains_key(name) || self.image.files.len() < MAX_FILES,
            "The disk already contains the maximum of {MAX_FILES} files"
        );
        ensure_no_path_conflict(self.image.files.keys().map(String::as_str), name)?;
        let mut encoding = Encoding::Raw;
        let mut data = bytes.to_vec();
        if compression == Compression::Auto && !bytes.is_empty() {
            let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(bytes)?;
            let compressed = encoder.finish()?;
            if compressed.len() < data.len() {
                encoding = Encoding::Zlib;
                data = compressed;
            }
        }
        let mut next = self.image.clone();
        next.files.insert(
            name.to_owned(),
            StoredFile {
                original_bytes: bytes.len() as u64,
                encoding,
                sha256: codec::hash_hex(bytes),
                data: data.into(),
            },
        );
        self.commit(next)
    }

    /// Import a regular host file, bounding reads even if it grows while read.
    pub fn put_file(
        &mut self,
        source: impl AsRef<Path>,
        name: &str,
        compression: Compression,
        overwrite: bool,
    ) -> Result<()> {
        self.ensure_writable()?;
        let source = source.as_ref();
        let input =
            File::open(source).with_context(|| format!("Cannot open {}", source.display()))?;
        let metadata = input.metadata()?;
        ensure!(metadata.is_file(), "Input must be a regular file");
        ensure!(
            metadata.len() <= MAX_FILE_SIZE,
            "File exceeds the 256 MiB input limit"
        );
        let mut bytes = Vec::new();
        input.take(MAX_FILE_SIZE + 1).read_to_end(&mut bytes)?;
        self.put(name, &bytes, compression, overwrite)
    }

    /// Export a single file to an explicit host path. Never infers host paths
    /// from virtual names. Refuses to replace the disk or its writer lock.
    pub fn extract(
        &self,
        name: &str,
        destination: impl AsRef<Path>,
        overwrite: bool,
    ) -> Result<()> {
        let destination = storage::normalize_path(destination.as_ref())?;
        ensure!(
            destination != self.path && destination != storage::lock_path(&self.path),
            "Output cannot replace the disk image or its lock file"
        );
        let bytes = self.read(name)?;
        storage::atomic_write(&destination, overwrite, |out| {
            out.write_all(&bytes)?;
            Ok(())
        })?;
        Ok(())
    }

    /// Delete a file and reclaim its payload and metadata space immediately.
    pub fn remove(&mut self, name: &str) -> Result<()> {
        self.ensure_writable()?;
        validate_name(name)?;
        let mut next = self.image.clone();
        ensure!(next.files.remove(name).is_some(), "File not found: {name}");
        self.commit(next)
    }

    /// Rename a file. Does not overwrite an existing virtual path.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.ensure_writable()?;
        validate_name(from)?;
        validate_name(to)?;
        ensure!(
            self.image.files.contains_key(from),
            "File not found: {from}"
        );
        if from == to {
            return Ok(());
        }
        ensure!(
            !self.image.files.contains_key(to),
            "File already exists: {to}"
        );
        let mut next = self.image.clone();
        let file = next.files.remove(from).context("Source file disappeared")?;
        ensure_no_path_conflict(next.files.keys().map(String::as_str), to)?;
        next.files.insert(to.to_owned(), file);
        self.commit(next)
    }

    /// Check every file's decompressed size and SHA-256. Opening a disk already
    /// checks its header, complete stored body, paths, offsets and zero padding.
    pub fn verify(&self) -> Result<()> {
        for (name, file) in &self.image.files {
            codec::decode_file(file)
                .with_context(|| format!("Integrity check failed for {name}"))?;
        }
        Ok(())
    }

    fn ensure_writable(&self) -> Result<()> {
        ensure!(self.writer.is_some(), "This disk was opened read-only");
        Ok(())
    }

    fn commit(&mut self, next: Image) -> Result<()> {
        // Build and validate the candidate before touching the existing image.
        let pixels = codec::encode(&next)?;
        let png_bytes = storage::atomic_write(&self.path, true, |out| {
            codec::write_png(out, next.width, next.height, &pixels)
        })?;
        self.image = next;
        self.png_bytes = png_bytes;
        Ok(())
    }
}

/// Raw RGB byte capacity, with the implementation's dimension and memory bounds.
pub fn pixel_capacity(width: u32, height: u32) -> Result<u64> {
    ensure!(
        (1..=MAX_DIMENSION).contains(&width) && (1..=MAX_DIMENSION).contains(&height),
        "Width and height must each be between 1 and {MAX_DIMENSION}"
    );
    let bytes = u64::from(width) * u64::from(height) * 3;
    ensure!(
        bytes <= MAX_CAPACITY,
        "Decoded pixel capacity exceeds the 256 MiB limit"
    );
    Ok(bytes)
}

/// Choose near-square dimensions that hold at least the requested raw bytes.
/// Metadata shares this capacity, and rounding to whole pixels adds a few bytes.
pub fn dimensions_for_size(bytes: u64) -> Result<(u32, u32)> {
    ensure!(
        bytes > 0 && bytes <= MAX_CAPACITY,
        "Size must be between 1 byte and 256 MiB"
    );
    let pixels = bytes.div_ceil(3);
    let root = pixels.isqrt();
    let width = if root * root == pixels {
        root
    } else {
        root + 1
    };
    let height = pixels.div_ceil(width);
    let (width, height) = (u32::try_from(width)?, u32::try_from(height)?);
    pixel_capacity(width, height)
        .context("Rounded image exceeds the capacity limit; choose a slightly smaller size")?;
    Ok((width, height))
}

pub(crate) fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= 1024,
        "Virtual path must contain 1 to 1024 UTF-8 bytes"
    );
    ensure!(
        !name
            .chars()
            .any(|c| c.is_control() || c == '\\' || c == ':'),
        "Virtual paths cannot contain control characters, backslashes or colons"
    );
    ensure!(
        name.split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".."),
        "Use a relative virtual path without empty, '.' or '..' components"
    );
    Ok(())
}

pub(crate) fn ensure_no_path_conflict<'a>(
    names: impl Iterator<Item = &'a str>,
    name: &str,
) -> Result<()> {
    for other in names {
        if other
            .strip_prefix(name)
            .is_some_and(|rest| rest.starts_with('/'))
            || name
                .strip_prefix(other)
                .is_some_and(|rest| rest.starts_with('/'))
        {
            bail!("File/directory path conflict between {name} and {other}");
        }
    }
    Ok(())
}
