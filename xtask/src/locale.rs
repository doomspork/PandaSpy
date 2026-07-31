//! Locale key parity.
//!
//! `en-US` is the reference. Every other locale must define exactly the same
//! keys — messages, terms, and attributes — in the same files.
//!
//! The check discovers locales by reading the directory, never from a list in
//! code. That is the property that makes "adding a language requires adding a
//! file and nothing else" true rather than aspirational: there is nowhere else
//! a language could need registering.
//!
//! Both directions are errors. A key missing from a translation is an obvious
//! bug; a key present *only* in a translation is usually a typo in the key
//! name, and reporting it turns one confusing "missing key" into a matched
//! pair that names the mistake.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use fluent_syntax::ast;
use fluent_syntax::parser::parse;

use crate::Violation;

/// The locale every other locale is measured against.
pub const REFERENCE: &str = "en-US";

/// What the check found, beyond the violations themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Locale directory names, sorted, reference first.
    pub locales: Vec<String>,
    /// Number of keys defined by the reference locale.
    pub reference_keys: usize,
}

/// Compare every locale against [`REFERENCE`].
///
/// # Errors
///
/// If `locales_dir` cannot be read, or the reference locale is missing —
/// neither is a translation problem, both mean the check itself cannot run.
pub fn check(locales_dir: &Path) -> Result<(Summary, Vec<Violation>)> {
    let locales = discover(locales_dir)?;

    if !locales.iter().any(|locale| locale == REFERENCE) {
        bail!(
            "reference locale `{REFERENCE}` not found in {}",
            locales_dir.display()
        );
    }

    let mut violations = Vec::new();
    let mut by_locale = BTreeMap::new();

    for locale in &locales {
        let (bundles, parse_problems) = read_locale(&locales_dir.join(locale), locale)?;
        violations.extend(parse_problems);
        by_locale.insert(locale.clone(), bundles);
    }

    let reference = by_locale
        .get(REFERENCE)
        .expect("reference presence checked above")
        .clone();

    let reference_keys = reference.values().map(BTreeSet::len).sum();

    for (locale, bundles) in &by_locale {
        if locale == REFERENCE {
            continue;
        }
        violations.extend(compare(locale, &reference, bundles));
    }

    violations.sort_by(|a, b| (&a.file, a.line, &a.message).cmp(&(&b.file, b.line, &b.message)));

    Ok((
        Summary {
            locales,
            reference_keys,
        },
        violations,
    ))
}

/// Locale directory names, reference first then alphabetical.
fn discover(locales_dir: &Path) -> Result<Vec<String>> {
    let entries = std::fs::read_dir(locales_dir)
        .with_context(|| format!("reading {}", locales_dir.display()))?;

    let mut locales: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", locales_dir.display()))?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        locales.push(entry.file_name().to_string_lossy().into_owned());
    }

    locales.sort_by_key(|locale| (locale != REFERENCE, locale.clone()));
    Ok(locales)
}

/// Every bundle in one locale: file stem -> the keys it defines.
type Bundles = BTreeMap<String, BTreeSet<String>>;

fn read_locale(dir: &Path, locale: &str) -> Result<(Bundles, Vec<Violation>)> {
    let mut bundles = Bundles::new();
    let mut violations = Vec::new();

    let entries =
        std::fs::read_dir(dir).with_context(|| format!("reading locale {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("ftl") {
            continue;
        }

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let display = format!("locales/{locale}/{name}");

        let source =
            std::fs::read_to_string(&path).with_context(|| format!("reading {display}"))?;
        let (keys, problems) = keys_of(&display, &source);

        violations.extend(problems);
        bundles.insert(name, keys);
    }

    Ok((bundles, violations))
}

/// Extract every addressable key from one `.ftl` file.
///
/// A key is a message id, a term id (with its leading `-`), or either of those
/// with an attribute suffix: `tray-quit`, `-brand-name`, `printer.tooltip`.
///
/// A message with attributes but no value does not contribute its bare id,
/// because there is nothing to look up under it. That asymmetry is deliberate:
/// it catches a translation that turns a plain message into an
/// attributes-only one.
#[must_use]
pub fn keys_of(display_path: &str, source: &str) -> (BTreeSet<String>, Vec<Violation>) {
    let mut keys = BTreeSet::new();
    let mut violations = Vec::new();

    let resource = match parse(source) {
        Ok(resource) => resource,
        Err((resource, errors)) => {
            for error in errors {
                violations.push(Violation {
                    file: display_path.to_owned(),
                    line: Some(crate::source::line_of(source, error.pos.start)),
                    message: format!("Fluent syntax error: {:?}", error.kind),
                });
            }
            resource
        }
    };

    let mut add = |key: String, violations: &mut Vec<Violation>| {
        if !keys.insert(key.clone()) {
            violations.push(Violation {
                file: display_path.to_owned(),
                line: None,
                message: format!("duplicate key `{key}` — the later definition wins silently"),
            });
        }
    };

    for entry in &resource.body {
        match entry {
            ast::Entry::Message(message) => {
                if message.value.is_some() {
                    add(message.id.name.to_owned(), &mut violations);
                }
                for attribute in &message.attributes {
                    add(
                        format!("{}.{}", message.id.name, attribute.id.name),
                        &mut violations,
                    );
                }
            }
            ast::Entry::Term(term) => {
                add(format!("-{}", term.id.name), &mut violations);
                for attribute in &term.attributes {
                    add(
                        format!("-{}.{}", term.id.name, attribute.id.name),
                        &mut violations,
                    );
                }
            }
            ast::Entry::Junk { content } => {
                violations.push(Violation {
                    file: display_path.to_owned(),
                    line: None,
                    message: format!(
                        "unparseable content ignored by Fluent: {:?}",
                        content.trim().chars().take(60).collect::<String>()
                    ),
                });
            }
            _ => {}
        }
    }

    (keys, violations)
}

fn compare(locale: &str, reference: &Bundles, actual: &Bundles) -> Vec<Violation> {
    let mut violations = Vec::new();

    for (bundle, expected_keys) in reference {
        let Some(actual_keys) = actual.get(bundle) else {
            violations.push(Violation {
                file: format!("locales/{locale}/{bundle}"),
                line: None,
                message: format!(
                    "missing bundle: `{REFERENCE}` has {bundle} with {} key(s)",
                    expected_keys.len()
                ),
            });
            continue;
        };

        for missing in expected_keys.difference(actual_keys) {
            violations.push(Violation {
                file: format!("locales/{locale}/{bundle}"),
                line: None,
                message: format!("missing key `{missing}` (defined in {REFERENCE})"),
            });
        }

        for extra in actual_keys.difference(expected_keys) {
            violations.push(Violation {
                file: format!("locales/{locale}/{bundle}"),
                line: None,
                message: format!(
                    "unknown key `{extra}` — not defined in {REFERENCE}; a typo, or a string \
                     that was never added to the reference locale"
                ),
            });
        }
    }

    for bundle in actual.keys() {
        if !reference.contains_key(bundle) {
            violations.push(Violation {
                file: format!("locales/{locale}/{bundle}"),
                line: None,
                message: format!("unknown bundle: `{REFERENCE}` has no {bundle}"),
            });
        }
    }

    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(source: &str) -> BTreeSet<String> {
        keys_of("test.ftl", source).0
    }

    #[test]
    fn messages_terms_and_attributes_all_count_as_keys() {
        let extracted =
            keys("-brand = Spool\ngreeting = Hello\n    .tooltip = A tooltip\nplain = Text\n");

        assert_eq!(
            extracted,
            BTreeSet::from([
                "-brand".to_owned(),
                "greeting".to_owned(),
                "greeting.tooltip".to_owned(),
                "plain".to_owned(),
            ])
        );
    }

    #[test]
    fn an_attributes_only_message_does_not_contribute_its_bare_id() {
        let extracted = keys("thing =\n    .label = Label\n");
        assert_eq!(extracted, BTreeSet::from(["thing.label".to_owned()]));
    }

    #[test]
    fn duplicate_keys_are_reported() {
        let (_, violations) = keys_of("test.ftl", "a = one\na = two\n");
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("duplicate key `a`"));
    }

    #[test]
    fn a_missing_key_is_reported_against_the_translation() {
        let reference = Bundles::from([(
            "app.ftl".to_owned(),
            BTreeSet::from(["a".to_owned(), "b".to_owned()]),
        )]);
        let actual = Bundles::from([("app.ftl".to_owned(), BTreeSet::from(["a".to_owned()]))]);

        let violations = compare("pl-PL", &reference, &actual);

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].file, "locales/pl-PL/app.ftl");
        assert!(violations[0].message.contains("missing key `b`"));
    }

    #[test]
    fn a_typo_in_a_translation_reports_both_halves() {
        let reference =
            Bundles::from([("app.ftl".to_owned(), BTreeSet::from(["quit".to_owned()]))]);
        let actual = Bundles::from([("app.ftl".to_owned(), BTreeSet::from(["quti".to_owned()]))]);

        let violations = compare("pl-PL", &reference, &actual);

        assert_eq!(violations.len(), 2, "{violations:?}");
        assert!(
            violations
                .iter()
                .any(|v| v.message.contains("missing key `quit`"))
        );
        assert!(
            violations
                .iter()
                .any(|v| v.message.contains("unknown key `quti`"))
        );
    }

    #[test]
    fn a_missing_bundle_file_is_reported() {
        let reference = Bundles::from([
            ("app.ftl".to_owned(), BTreeSet::from(["a".to_owned()])),
            ("hms.ftl".to_owned(), BTreeSet::new()),
        ]);
        let actual = Bundles::from([("app.ftl".to_owned(), BTreeSet::from(["a".to_owned()]))]);

        let violations = compare("pl-PL", &reference, &actual);

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("missing bundle"));
    }

    #[test]
    fn the_repositorys_locales_are_in_parity() {
        // The check that actually gates CI.
        let (summary, violations) = check(&crate::repo_root().join("locales")).unwrap();

        assert!(
            violations.is_empty(),
            "locale parity broken:\n{}",
            violations
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert_eq!(summary.locales.first().map(String::as_str), Some(REFERENCE));
        assert!(summary.locales.len() >= 2, "expected en-US and pl-PL");
        assert!(summary.reference_keys > 0);
    }

    #[test]
    fn locales_are_discovered_from_the_filesystem_not_a_list() {
        // Guards the "adding a language is adding a directory" promise: this
        // test would need editing if anyone hard-coded the locale list.
        let discovered = discover(&crate::repo_root().join("locales")).unwrap();
        assert!(discovered.contains(&"pl-PL".to_owned()));
    }
}
