use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

/// Canonicalize the parent, even when the final file does not yet exist. This
/// makes relative paths and symlinked directories share the same writer lock.
pub(crate) fn normalize_path(path: &Path) -> Result<PathBuf> {
    let name = path.file_name().context("Path must name a file")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .with_context(|| format!("Cannot access directory {}", parent.display()))?;
    Ok(parent.join(name))
}

pub(crate) fn check_regular_file(path: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("Cannot access {}", path.display()))?;
    ensure!(
        metadata.file_type().is_file(),
        "Expected a regular file, not a directory or symbolic link: {}",
        path.display()
    );
    Ok(())
}

pub(crate) fn reject_hard_links(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            fs::metadata(path)?.nlink() == 1,
            "Writable disk images cannot have multiple hard links"
        );
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) fn lock_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".dpegfs.lock");
    PathBuf::from(name)
}

pub(crate) struct WriterLock {
    // Dropping the handle releases the OS lock. Never unlink this sidecar:
    // another process may hold/open the same inode while we release it.
    _file: File,
}

impl WriterLock {
    pub(crate) fn acquire(image: &Path) -> Result<Self> {
        let path = lock_path(image);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => ensure!(
                metadata.file_type().is_file(),
                "Lock path is not a regular file: {}",
                path.display()
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("Cannot open writer lock {}", path.display()))?;
        file.try_lock()
            .context("Cannot acquire disk writer lock; another writer may be using this image")?;
        Ok(Self { _file: file })
    }
}

/// Stage a complete file in the destination directory, flush and sync it, then
/// publish it. No fallible work follows a successful publication, so callers can
/// commit their in-memory state without mistaking a post-rename failure for a
/// rollback. Parent directory fsync/power-loss durability is not promised in v1.
pub(crate) fn atomic_write(
    path: &Path,
    overwrite: bool,
    write: impl FnOnce(&mut File) -> Result<()>,
) -> Result<u64> {
    let permissions = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                overwrite,
                "Output already exists: {}; use overwrite to replace it",
                path.display()
            );
            ensure!(
                metadata.file_type().is_file(),
                "Output is not a regular file: {}",
                path.display()
            );
            ensure!(
                !metadata.permissions().readonly(),
                "Output is read-only: {}",
                path.display()
            );
            Some(metadata.permissions())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let parent = path.parent().context("Output has no parent directory")?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".dpegfs-")
        .tempfile_in(parent)
        .context("Cannot create temporary output in destination directory")?;
    write(temporary.as_file_mut())?;
    temporary.flush()?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary
        .as_file()
        .sync_all()
        .context("Cannot sync staged output")?;
    let length = temporary.as_file().metadata()?.len();
    if overwrite {
        temporary
            .persist(path)
            .map_err(|error| error.error)
            .with_context(|| format!("Cannot replace {}", path.display()))?;
    } else {
        temporary
            .persist_noclobber(path)
            .map_err(|error| error.error)
            .with_context(|| format!("Cannot create {} (it may already exist)", path.display()))?;
    }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_staging_preserves_old_file_and_cleans_temporary_file() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("disk.png");
        fs::write(&path, b"original")?;
        let result = atomic_write(&path, true, |out| {
            out.write_all(b"incomplete replacement")?;
            anyhow::bail!("simulated disk write failure");
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path)?, b"original");
        assert_eq!(fs::read_dir(directory.path())?.count(), 1);
        Ok(())
    }

    #[test]
    fn create_does_not_clobber_a_file_created_during_staging() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("output");
        let result = atomic_write(&path, false, |out| {
            out.write_all(b"our content")?;
            fs::write(&path, b"concurrent content")?;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path)?, b"concurrent content");
        assert_eq!(fs::read_dir(directory.path())?.count(), 1);
        Ok(())
    }
}
