//! The one architectural rule, mechanised.
//!
//! Platform-conditional compilation may appear only in `src-tauri/` and
//! `crates/pandaspy-store/`. Everywhere else it is a design error, because it is
//! the first step on the road that ends with the same protocol implemented
//! twice and fixed once.
//!
//! This check is intentionally blunt. It is not trying to be a borrow checker;
//! it is trying to make "just this once" impossible to merge quietly.

use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use walkdir::WalkDir;

use crate::source::{blank_comments_and_strings, line_of};
use crate::{Violation, display_path};

/// Directories that are scanned. Everything under them is subject to the rule
/// unless it appears in [`ALLOWED`].
///
/// `src-tauri/` is deliberately absent: it is the platform layer, and branching
/// on the OS is its whole job.
pub const SCANNED: &[&str] = &["crates", "xtask"];

/// The only place under [`SCANNED`] where platform-conditional code is allowed.
///
/// Secret storage is genuinely different per OS — Keychain, Credential Manager,
/// Secret Service — and no abstraction makes them the same thing.
pub const ALLOWED: &[&str] = &["crates/pandaspy-store"];

/// `cfg` predicates that are always about the target platform. These are
/// unambiguous: seeing one anywhere in real code is a violation.
static PLATFORM_PREDICATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(target_os|target_family|target_vendor|target_env|target_arch|target_endian|target_pointer_width)\b",
    )
    .expect("static regex")
});

/// Bare platform predicates (`cfg(unix)`, `cfg!(windows)`). These two words are
/// far too common in prose to flag on their own, so they only count when they
/// appear inside a `cfg`.
static BARE_PLATFORM_CFG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bcfg(_attr)?\s*!?\s*\([^)]*\b(unix|windows)\b").expect("static regex")
});

/// Walk the tree and return every place platform-conditional code has leaked
/// out of the two crates allowed to have it.
///
/// # Errors
///
/// If a directory cannot be walked or a file cannot be read.
pub fn check(root: &Path) -> Result<Vec<Violation>> {
    let mut violations = Vec::new();

    for scanned in SCANNED {
        let dir = root.join(scanned);
        if !dir.exists() {
            continue;
        }

        for entry in WalkDir::new(&dir) {
            // Do not swallow walk errors: an unreadable directory means the
            // check silently covered less than it claimed to.
            let entry = entry.with_context(|| format!("walking {scanned}/"))?;
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }

            let relative = display_path(root, path);
            if ALLOWED
                .iter()
                .any(|allowed| relative.starts_with(&format!("{allowed}/")))
            {
                continue;
            }

            match path.extension().and_then(|ext| ext.to_str()) {
                Some("rs") => {
                    let source = std::fs::read_to_string(path)
                        .with_context(|| format!("reading {relative}"))?;
                    violations.extend(check_rust(&relative, &source));
                }
                // A `[target.'cfg(windows)'.dependencies]` table is the same
                // rule broken one level up, and is easy to miss in review.
                Some("toml") => {
                    let source = std::fs::read_to_string(path)
                        .with_context(|| format!("reading {relative}"))?;
                    violations.extend(check_manifest(&relative, &source));
                }
                _ => {}
            }
        }
    }

    violations.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    Ok(violations)
}

/// Find platform predicates in one Rust file. Public so the tests — and any
/// future editor integration — can exercise it without touching the disk.
#[must_use]
pub fn check_rust(relative_path: &str, source: &str) -> Vec<Violation> {
    let code = blank_comments_and_strings(source);
    let mut violations = Vec::new();

    for capture in PLATFORM_PREDICATE.find_iter(&code) {
        violations.push(Violation {
            file: relative_path.to_owned(),
            line: Some(line_of(&code, capture.start())),
            message: format!(
                "`{}` is platform-conditional code outside src-tauri/ and pandaspy-store/",
                capture.as_str()
            ),
        });
    }

    for capture in BARE_PLATFORM_CFG.find_iter(&code) {
        violations.push(Violation {
            file: relative_path.to_owned(),
            line: Some(line_of(&code, capture.start())),
            message: "platform-conditional `cfg` outside src-tauri/ and pandaspy-store/".to_owned(),
        });
    }

    violations
}

/// Find `[target.'cfg(…)'.dependencies]` tables in one manifest.
#[must_use]
pub fn check_manifest(relative_path: &str, source: &str) -> Vec<Violation> {
    let Ok(manifest) = source.parse::<toml::Table>() else {
        // Malformed manifests are Cargo's problem to report, not ours.
        return Vec::new();
    };

    let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) else {
        return Vec::new();
    };

    targets
        .keys()
        .map(|selector| Violation {
            file: relative_path.to_owned(),
            line: source
                .lines()
                .position(|line| line.contains(selector))
                .map(|index| index + 1),
            message: format!(
                "`[target.{selector}]` is a platform-conditional dependency outside \
                 src-tauri/ and pandaspy-store/"
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cfg_attribute_is_caught() {
        let violations = check_rust(
            "crates/pandaspy-proto/src/lib.rs",
            "#[cfg(target_os = \"macos\")]\nfn mac_only() {}\n",
        );

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, Some(1));
    }

    #[test]
    fn the_cfg_macro_is_caught_too() {
        let violations = check_rust(
            "crates/x/src/a.rs",
            "let mac = cfg!(target_os = \"macos\");",
        );
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn a_bare_platform_cfg_is_caught() {
        let violations = check_rust("crates/x/src/a.rs", "#[cfg(unix)]\nfn u() {}\n");
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn a_multiline_cfg_is_caught() {
        let violations = check_rust(
            "crates/x/src/a.rs",
            "#[cfg(any(\n    target_os = \"linux\",\n    target_os = \"freebsd\"\n))]\nfn u() {}\n",
        );
        assert_eq!(violations.len(), 2, "both predicates should be reported");
        assert_eq!(violations[0].line, Some(2));
        assert_eq!(violations[1].line, Some(3));
    }

    #[test]
    fn prose_about_the_rule_is_not_a_violation() {
        // The repository's own documentation must not trip its own check.
        let violations = check_rust(
            "crates/x/src/a.rs",
            "//! `#[cfg(target_os)]` is banned here.\n/// See cfg(unix) discussion.\nfn a() {}\n",
        );
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn a_feature_cfg_is_fine() {
        let violations = check_rust(
            "crates/x/src/a.rs",
            "#[cfg(feature = \"serde\")]\nfn a() {}\n",
        );
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn cfg_test_is_fine() {
        let violations = check_rust("crates/x/src/a.rs", "#[cfg(test)]\nmod tests {}\n");
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn platform_conditional_dependencies_are_caught() {
        let manifest =
            "[package]\nname = \"x\"\n\n[target.'cfg(windows)'.dependencies]\nwinapi = \"0.3\"\n";
        let violations = check_manifest("crates/x/Cargo.toml", manifest);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].line, Some(4));
    }

    #[test]
    fn an_ordinary_manifest_is_fine() {
        let manifest = "[package]\nname = \"x\"\n\n[dependencies]\nserde = \"1\"\n";
        assert!(check_manifest("crates/x/Cargo.toml", manifest).is_empty());
    }

    #[test]
    fn the_repository_itself_is_clean() {
        // The check that actually gates CI. `pandaspy-store` contains a real
        // `#[cfg(target_os)]`; this asserting zero proves the allow-list works
        // as well as proving the rest of the tree is clean.
        let violations = check(&crate::repo_root()).unwrap();
        assert!(
            violations.is_empty(),
            "platform-conditional code has leaked:\n{}",
            violations
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
