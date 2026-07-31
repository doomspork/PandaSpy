---
description: Add a new language to PandaSpy
argument-hint: '<BCP-47 tag, e.g. de-DE>'
allowed-tools: Bash(cargo:*), Bash(mkdir:*), Bash(cp:*), Read, Write, Edit
---

Add the locale **$ARGUMENTS** to PandaSpy.

Adding a language requires adding files and nothing else. There is no list of
supported languages in Rust, in TypeScript or in the build config — both sides
discover locales by scanning `locales/`. If this procedure ever seems to
require a code change, that is a bug in the plumbing, not a step you missed.

## 1. Create the directory

```
mkdir -p locales/$ARGUMENTS
cp locales/en-US/*.ftl locales/$ARGUMENTS/
```

Use the full BCP-47 tag with region (`de-DE`, not `de`). Negotiation already
matches a bare `de` from the OS to `de-DE`, so the region costs nothing and
leaves room for `de-AT` later.

## 2. Translate the values, never the keys

Edit each `.ftl` in the new directory. Change what is to the right of the `=`.
Leave message ids, term ids and attribute names exactly as they are.

Things that stay untranslated:

- `-brand-name = PandaSpy`. It is a product name.
- Anything in `hms.ftl` you cannot verify. A guessed translation of a printer
  error is worse than an English one, because the user will act on it.

Things to get right rather than approximate:

- Use Fluent selectors for plurals rather than concatenating a number and a
  noun. Languages with case systems and multi-form plurals cannot be assembled
  from fragments, which is the whole reason this project uses Fluent.
- Keep placeables (`{ $count }`, `{ -brand-name }`) intact and move them where
  the target language wants them.

## 3. Check parity

```
cargo xtask locale-check
```

This fails if the new locale is missing any key `en-US` defines, **or** defines
one it does not. The second direction catches typos in key names, which is the
mistake this step actually exists to find.

If a string genuinely has no translation yet, copy the English value across.
That is visible in a diff and can be found later; a missing key is neither.

## 4. Verify both sides pick it up

```
cargo test -p pandaspy           # Rust: tray menu and notifications
pnpm run check                # frontend
```

Then run the app and confirm the new language is selected when the OS asks for
it. There is nothing to register — if the directory is there and parity passes,
both sides already have it.

## 5. Commit

```
feat(i18n): add German (de-DE)
```

Scope is `i18n`. Mention in the body whether the translation is complete or
whether some values are still English placeholders — that is the thing a future
contributor most needs to know.
