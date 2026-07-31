---
description: Run the full pre-push gate (fmt, clippy, tests, wasm purity, locale parity, cfg leak, frontend)
allowed-tools: Bash(cargo:*), Bash(pnpm:*)
---

Run the complete pre-push gate and fix whatever it finds.

```
cargo xtask check
```

That single command is the gate. It runs, in order:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo check -p bambu-proto --target wasm32-unknown-unknown`
5. `cargo xtask locale-check`
6. `cargo xtask cfg-check`
7. `pnpm run check` (prettier, eslint, svelte-check) — skipped with a visible
   message if pnpm is not installed

## Interpreting failures

Most are ordinary. These three mean something specific:

**`bambu-proto` fails the wasm build.** Something with I/O, a clock, randomness
or a platform API got into the pure crate. Do not add a shim or a feature flag
to make wasm happy — find what was added and move it to the crate that is
allowed to do it. This check exists to catch exactly that, at the moment it
happens.

**`cfg-check` reports a violation.** Platform-conditional code appeared outside
`src-tauri/` and `crates/bambu-store/`. The fix is almost never to add an
allow-list entry. It is to inject the platform difference through a trait —
`bambu-discovery` and `bambu-client` are already shaped for it. Read
`CLAUDE.md` § The one architectural rule before deciding otherwise; the
allow-list has two entries and adding a third is a design decision, not a
formality.

**`locale-check` reports missing or unknown keys.** A key was added to `en-US`
and not to the other locales, or a key was mistyped. Both directions are
reported: a "missing `foo`" plus an "unknown `fooo`" in the same file is a
typo, not two problems. If you genuinely cannot translate a string, copy the
English value across — that shows up in a diff, whereas a missing key does not.

## After it passes

Report what ran and what it found. If anything was skipped (pnpm missing, for
instance), say so explicitly rather than reporting a clean run.
