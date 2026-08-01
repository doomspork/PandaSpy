//! Rust half of PandaSpy's internationalisation.
//!
//! The tray menu and OS notifications are built in Rust, so Rust needs the same
//! strings the window does. It reads them from the same place: the repository's
//! `locales/` directory, embedded into the binary at compile time.
//!
//! There is no generated string table, no build script that copies files
//! around, and no list of supported languages in code. Adding
//! `locales/de-DE/*.ftl` is sufficient — [`Localiser::new`] finds it because it
//! walks the embedded directory.
//!
//! See `src/lib/i18n.ts` for the frontend half, and `cargo xtask locale-check`
//! for the parity guarantee that lets both sides assume a key exists.

use std::collections::BTreeMap;
use std::fmt;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use include_dir::{Dir, include_dir};
use unic_langid::LanguageIdentifier;

/// The repository's `locales/` tree, baked into the binary.
///
/// `build.rs` emits a `rerun-if-changed` for this path — without it Cargo would
/// not notice a translation edit and would happily hand you a stale binary.
static LOCALES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../locales");

/// Fallback for anything the user's system asks for that we do not have.
pub const DEFAULT_LOCALE: &str = "en-US";

/// Every translation, with one of them selected.
pub struct Localiser {
    bundles: BTreeMap<String, FluentBundle<FluentResource>>,
    active: String,
}

impl fmt::Debug for Localiser {
    /// `FluentBundle` is not `Debug`, and dumping every message would be
    /// useless anyway. Show what actually matters when something is wrong:
    /// which locales loaded, and which one is in use.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Localiser")
            .field("available", &self.bundles.keys().collect::<Vec<_>>())
            .field("active", &self.active)
            .finish()
    }
}

impl Localiser {
    /// Load every embedded locale.
    ///
    /// Malformed `.ftl` content is reported to stderr and skipped rather than
    /// panicking: a broken translation must not stop the app from starting in
    /// English. CI catches those before they ship.
    #[must_use]
    pub fn new() -> Self {
        let mut bundles = BTreeMap::new();

        for entry in LOCALES.dirs() {
            let Some(tag) = entry.path().file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Ok(langid) = tag.parse::<LanguageIdentifier>() else {
                eprintln!("[i18n] skipping locales/{tag}: not a valid BCP-47 tag");
                continue;
            };

            let mut bundle = FluentBundle::new_concurrent(vec![langid]);
            // Fluent wraps placeables in Unicode directional isolate marks by
            // default. Correct for bidirectional text, but they are invisible
            // characters that break `assert_eq!` and menu-item comparisons.
            // Turn this back on when PandaSpy ships an RTL locale.
            bundle.set_use_isolating(false);

            let mut loaded_any = false;
            for file in entry.files() {
                if file.path().extension().and_then(|ext| ext.to_str()) != Some("ftl") {
                    continue;
                }
                let Some(source) = file.contents_utf8() else {
                    eprintln!("[i18n] {} is not valid UTF-8", file.path().display());
                    continue;
                };

                match FluentResource::try_new(source.to_owned()) {
                    Ok(resource) => {
                        if let Err(errors) = bundle.add_resource(resource) {
                            for error in errors {
                                eprintln!("[i18n] {}: {error}", file.path().display());
                            }
                        }
                        loaded_any = true;
                    }
                    Err((_, errors)) => {
                        for error in errors {
                            eprintln!("[i18n] {}: {error:?}", file.path().display());
                        }
                    }
                }
            }

            if loaded_any {
                bundles.insert(tag.to_owned(), bundle);
            }
        }

        debug_assert!(
            bundles.contains_key(DEFAULT_LOCALE),
            "the default locale must always be embedded"
        );

        Self {
            active: DEFAULT_LOCALE.to_owned(),
            bundles,
        }
    }

    /// Locale tags that actually have translations, sorted.
    pub fn available(&self) -> Vec<String> {
        self.bundles.keys().cloned().collect()
    }

    /// The locale currently in use.
    pub fn active(&self) -> &str {
        &self.active
    }

    /// Pick the best available locale for a list of user preferences.
    ///
    /// Exact match wins; failing that, the primary language subtag does, so a
    /// system reporting `pl` gets `pl-PL`. Everything else falls back to
    /// [`DEFAULT_LOCALE`].
    #[must_use]
    pub fn negotiate(&self, requested: &[String]) -> String {
        for want in requested {
            if let Some(tag) = self
                .bundles
                .keys()
                .find(|tag| tag.eq_ignore_ascii_case(want))
            {
                return tag.clone();
            }
        }

        for want in requested {
            let primary = primary_subtag(want);
            if let Some(tag) = self
                .bundles
                .keys()
                .find(|tag| primary_subtag(tag).eq_ignore_ascii_case(primary))
            {
                return tag.clone();
            }
        }

        DEFAULT_LOCALE.to_owned()
    }

    /// Select a locale. Unknown tags fall back rather than failing — the caller
    /// is usually passing through something the OS said.
    pub fn set_active(&mut self, tag: &str) {
        self.active = if self.bundles.contains_key(tag) {
            tag.to_owned()
        } else {
            DEFAULT_LOCALE.to_owned()
        };
    }

    /// Resolve a message id in the active locale, falling back to
    /// [`DEFAULT_LOCALE`], then to the id itself.
    ///
    /// Returning the raw id is deliberate. A tray menu reading `tray-quit` is
    /// impossible to miss; an empty menu item looks like a rendering glitch and
    /// gets shipped.
    #[must_use]
    pub fn get(&self, id: &str) -> String {
        self.get_with(id, None)
    }

    /// As [`Self::get`], with Fluent arguments.
    #[must_use]
    pub fn get_with(&self, id: &str, args: Option<&FluentArgs<'_>>) -> String {
        for tag in [self.active.as_str(), DEFAULT_LOCALE] {
            let Some(bundle) = self.bundles.get(tag) else {
                continue;
            };
            let Some(pattern) = bundle.get_message(id).and_then(|message| message.value()) else {
                continue;
            };

            let mut errors = Vec::new();
            let formatted = bundle.format_pattern(pattern, args, &mut errors);
            for error in &errors {
                eprintln!("[i18n] {tag}/{id}: {error}");
            }
            return formatted.into_owned();
        }

        eprintln!("[i18n] missing key: {id}");
        id.to_owned()
    }
}

impl Default for Localiser {
    fn default() -> Self {
        Self::new()
    }
}

fn primary_subtag(tag: &str) -> &str {
    tag.split(['-', '_']).next().unwrap_or(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_locales_are_embedded() {
        // Exact-set guard against a locale silently vanishing from the embed.
        // The plumbing discovers locales by walking the directory, so adding a
        // language needs no production change — only this expected list grows.
        let localiser = Localiser::new();
        assert_eq!(localiser.available(), vec!["en-US", "pl-PL", "zh-CN"]);
    }

    #[test]
    fn strings_come_from_the_shared_locales_directory() {
        // Not a copy, not a generated table — the same file the frontend reads.
        let mut localiser = Localiser::new();

        assert_eq!(localiser.get("tray-quit"), "Quit");
        localiser.set_active("pl-PL");
        assert_eq!(localiser.get("tray-quit"), "Zakończ");
    }

    #[test]
    fn terms_are_resolved_inside_messages() {
        let localiser = Localiser::new();
        // `tray-show = Show { -brand-name }` must not leak the placeable.
        assert_eq!(localiser.get("tray-show"), "Show PandaSpy");
    }

    #[test]
    fn an_exact_tag_wins() {
        let localiser = Localiser::new();
        assert_eq!(localiser.negotiate(&["pl-PL".to_owned()]), "pl-PL");
    }

    #[test]
    fn a_bare_language_matches_a_regional_translation() {
        // macOS and Linux both report bare language tags in some configurations.
        let localiser = Localiser::new();
        assert_eq!(localiser.negotiate(&["pl".to_owned()]), "pl-PL");
    }

    #[test]
    fn tag_matching_ignores_case_and_separator() {
        let localiser = Localiser::new();
        assert_eq!(localiser.negotiate(&["PL_pl".to_owned()]), "pl-PL");
    }

    #[test]
    fn an_unsupported_language_falls_back_rather_than_failing() {
        let localiser = Localiser::new();
        assert_eq!(localiser.negotiate(&["de-DE".to_owned()]), DEFAULT_LOCALE);
    }

    #[test]
    fn preferences_are_tried_in_order() {
        let localiser = Localiser::new();
        assert_eq!(
            localiser.negotiate(&["de-DE".to_owned(), "pl-PL".to_owned()]),
            "pl-PL"
        );
    }

    #[test]
    fn a_missing_key_returns_the_id_so_it_is_impossible_to_miss() {
        let localiser = Localiser::new();
        assert_eq!(localiser.get("no-such-key"), "no-such-key");
    }

    #[test]
    fn a_key_missing_only_from_a_translation_falls_back_to_english() {
        // Parity is enforced by `cargo xtask locale-check`, but the runtime must
        // degrade gracefully anyway — the check cannot cover a locale added by a
        // user's own build.
        let mut localiser = Localiser::new();
        localiser.set_active("pl-PL");
        assert_eq!(localiser.get("window-title"), "PandaSpy");
    }
}
