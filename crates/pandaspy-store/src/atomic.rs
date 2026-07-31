use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::StoreError;

/// Write `bytes` to `path` such that a crash mid-write cannot leave a truncated
/// file behind.
///
/// The idiom is write-to-temp-then-rename: `rename` over an existing path is
/// atomic on every platform PandaSpy targets, so a reader ever sees either the
/// old file or the new one, never a half-written one. That is the difference
/// between a power cut costing nothing and it costing the user their printer
/// list (config) or their pins.
///
/// The temp file is created in the *same directory* as the target on purpose:
/// `rename` is only atomic within a filesystem, and a temp dir may be on a
/// different one.
///
/// This crate deliberately does not `chmod` the file — doing so portably needs
/// per-OS code, which the architectural rule keeps out of the logic. The caller
/// (`src-tauri`) owns the config directory and is responsible for its
/// permissions; the encrypted-secret threat model documents that dependency.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    if let Some(dir) = parent_dir(path) {
        fs::create_dir_all(dir)
            .map_err(|e| StoreError::Io(format!("create {}: {e}", dir.display())))?;
    }

    let tmp = temp_path(path)?;
    // `create_new` refuses to open through an existing file or symlink, so a
    // stale temp never gets written through and the target is never the temp.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| StoreError::Io(format!("create temp {}: {e}", tmp.display())))?;

    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| StoreError::Io(format!("write temp {}: {e}", tmp.display())));
    drop(file);

    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        StoreError::Io(format!(
            "rename {} -> {}: {e}",
            tmp.display(),
            path.display()
        ))
    })
}

/// The target's directory, or `None` for a bare filename (write into the cwd).
fn parent_dir(path: &Path) -> Option<&Path> {
    path.parent().filter(|p| !p.as_os_str().is_empty())
}

/// A sibling temp path with a random suffix, so two writers cannot collide.
fn temp_path(path: &Path) -> Result<PathBuf, StoreError> {
    let mut rand = [0u8; 8];
    getrandom::fill(&mut rand)
        .map_err(|e| StoreError::Io(format!("randomness unavailable: {e}")))?;

    let mut suffix = String::with_capacity(16);
    for byte in rand {
        use core::fmt::Write as _;
        // Infallible: writing to a String never fails.
        let _ = write!(suffix, "{byte:02x}");
    }

    let stem = path
        .file_name()
        .map_or_else(|| "store".to_owned(), |n| n.to_string_lossy().into_owned());
    let tmp_name = format!(".{stem}.tmp.{suffix}");

    Ok(match parent_dir(path) {
        Some(dir) => dir.join(tmp_name),
        None => PathBuf::from(tmp_name),
    })
}
