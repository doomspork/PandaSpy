//! A minimal temp-directory RAII helper for tests.
//!
//! The workspace has no `tempfile` dependency, and the store's on-disk stores
//! all need a real, writable, unique directory to exercise atomic writes. This
//! creates one under the system temp dir and removes it on drop. Test-only.

use std::path::{Path, PathBuf};

/// A unique directory under the system temp dir, deleted when dropped.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        let mut rand = [0u8; 8];
        getrandom::fill(&mut rand).expect("randomness for a temp dir name");

        let mut name = String::from("pandaspy-store-test-");
        for byte in rand {
            use core::fmt::Write as _;
            let _ = write!(name, "{byte:02x}");
        }

        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.path.join(rel)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best-effort: a leaked test temp dir is noise, not a failure.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
