//! Repository automation for PandaSpy.
//!
//! These checks are Rust rather than shell scripts for one reason: they must
//! produce identical results on a maintainer's Mac, a contributor's Windows
//! box, and an Ubuntu CI runner. A `grep` pipeline that only works on one of
//! those is a check that only gets run on one of those.
//!
//! Two rules are enforced here, both of which exist to stop the codebase
//! quietly forking into per-platform variants:
//!
//! * [`cfg_leak`] — platform-conditional compilation may appear only in
//!   `src-tauri/` and `crates/pandaspy-store/`.
//! * [`locale`] — every locale defines exactly the keys `en-US` defines.
//!
//! Run them with `cargo xtask cfg-check` / `cargo xtask locale-check`, or the
//! whole pre-push gate with `cargo xtask check`.

pub mod cfg_leak;
pub mod locale;
pub mod source;

use std::path::{Path, PathBuf};

/// The repository root.
///
/// `xtask` is the workspace root package, so its manifest directory *is* the
/// repo root. Resolving it at compile time means the checks behave the same
/// whatever directory they are invoked from.
#[must_use]
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// A single rule violation, in a format that reads well in a CI log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Repo-relative, so the message is the same locally and in CI.
    pub file: String,
    /// 1-indexed. `None` when the problem is about the file as a whole.
    pub line: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{}: {}", self.file, line, self.message),
            None => write!(f, "{}: {}", self.file, self.message),
        }
    }
}

/// Render a path relative to the repo root, with forward slashes, so that
/// Windows and Unix CI logs are diffable against each other.
#[must_use]
pub fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}
