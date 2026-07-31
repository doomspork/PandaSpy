# Locales

[Fluent](https://projectfluent.org/) `.ftl` files. **This directory is the only
place user-facing strings live.**

## The contract

* One directory per locale, named with a BCP-47 tag: `en-US`, `pl-PL`.
* Inside it, one `.ftl` file per bundle: `app.ftl`, `hms.ftl`.
* **`en-US` is the reference locale.** Every other locale must define exactly
  the same set of keys — messages, terms and attributes alike.
* Both sides of the app read these same files. `fluent-rs` embeds them into the
  Rust binary for the tray menu and OS notifications; `@fluent/bundle` loads
  them in the frontend via a Vite glob. There is no second copy anywhere.

## Adding a language

Add a directory. That is the whole procedure — no Rust, TypeScript or build
config changes. Both sides discover locales by scanning this tree.

Run `/newlocale` for a guided version, or by hand:

```sh
mkdir -p locales/de-DE
cp locales/en-US/*.ftl locales/de-DE/
# translate the values, leave the keys alone
cargo xtask locale-check
```

## Adding a string

1. Add it to `locales/en-US/<bundle>.ftl` with a comment explaining the context
   a translator would otherwise have to guess.
2. Add it to **every** other locale. `cargo xtask locale-check` — and CI — fail
   otherwise, on purpose: a half-translated release is worse than an untranslated
   one because nobody notices.
3. If you genuinely cannot translate it, copy the English value across. That is
   visible in a diff; a missing key is not.

## What does not belong here

* Proper nouns (`Spool`, `Keychain`, `Credential Manager`). Translating a
  product name makes an error message harder to act on.
* Log messages and error strings that only developers read.
* Anything assembled by string concatenation. Use Fluent placeables and
  selectors instead — Polish has grammatical cases and plural rules that
  `"Found " + n + " printers"` cannot express.
