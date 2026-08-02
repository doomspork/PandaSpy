# Contributing to PandaSpy

Thanks for your interest. PandaSpy is a Rust + Tauri v2 desktop tray app that
monitors Bambu Lab 3D printers over the LAN, targeting macOS, Windows and Linux
from **one** codebase.

This file is the short version. **[CLAUDE.md](CLAUDE.md) is the deep reference**
— it explains not just the rules below but _why_ each exists and how it fails
quietly if broken. Read it before a non-trivial change; several parts of this
repo (the release wiring, the placement of platform-conditional code, the serde
discipline) are easy to break in ways CI only catches late.

## Getting set up

```sh
pnpm install
pnpm tauri dev        # run the app with frontend hot reload
```

Per-platform prerequisites (Xcode CLT on macOS, the C++ Build Tools on Windows,
a list of `-dev` packages on Linux) are in CLAUDE.md § Running, testing and
building. The Rust toolchain is pinned in `rust-toolchain.toml` and the Node
version in `.nvmrc`; use those.

If you use [mise](https://mise.jdx.dev), `mise install` sets up Node and pnpm at
the pinned versions in one step (it reads `.nvmrc` and `mise.toml`); then run
`pnpm install`. Rust still comes from rustup via `rust-toolchain.toml` — mise
does not manage it, on purpose (see the comment in `mise.toml`).

## The gate

One command is the definition of "passing", and it is exactly what CI runs:

```sh
cargo xtask check
```

It runs fmt, clippy (`-D warnings`), the tests, the wasm purity check, locale
parity, the cfg check, and the frontend's format/lint/typecheck. **Run it before
opening a pull request.** `cargo deny check --workspace` is also part of CI; run
it if you touch dependencies (`cargo install cargo-deny --locked` first).

## Commit and PR conventions

- **[Conventional Commits](https://www.conventionalcommits.org/)**, enforced by
  commitlint on **both** the PR title and the commits inside it — either can
  become permanent history depending on the merge strategy.
- Scopes map to crates: `proto`, `discovery`, `client`, `store`, `tauri`,
  `xtask`, `ui`, `i18n`, `fixtures`, `ci`, `deps`, `release`. The full list is
  in `commitlint.config.js`; an unrecognised scope is rejected because it is
  almost always a typo or a sign the change belongs elsewhere.
- Only `feat`, `fix`, `perf` and `revert` reach the changelog. Commit bodies
  should say **why**; the diff already says what.
- Keep each commit building in isolation — don't split a change so that an
  intermediate commit fails to compile.
- CI must be green before merge.

## Rules that matter most

These are the ones that keep the project honest. CLAUDE.md has the full
reasoning; here is what you must not violate:

1. **One implementation, no platform forks.** Platform-conditional compilation
   (`#[cfg(target_os)]` and friends) is permitted **only** in `src-tauri/` and
   `crates/pandaspy-store/`. Anywhere else it means the design is wrong — inject
   the platform behaviour behind a trait instead of branching on the target.
   `cargo xtask cfg-check` enforces this.

2. **`pandaspy-proto` stays pure.** No I/O, no clock, no randomness, no platform
   APIs. It is compiled for `wasm32-unknown-unknown` in CI precisely to catch
   impurity. If that job fails, _move_ the offending code out — do not add a
   shim to make wasm happy.

3. **Parse leniently.** The printer protocol is undocumented and changes across
   firmware. Every wire field is `Option<T>`; unknown enum values are preserved
   in an `Unknown(...)` variant, never rejected; `#[serde(deny_unknown_fields)]`
   is banned. A strict parser turns a firmware update into a bricked app for
   every user at once.

4. **Any protocol change requires a fixture.** Not "should have" — requires. Add
   a recorded (and **redacted** — see `fixtures/README.md`) payload to
   `fixtures/`, and let the golden snapshot test show what the parser
   understood. Never edit a fixture to make a test pass: a fixture is a recording
   of something a real printer really said. Use the `/fixture` command for the
   guided version.

5. **Every user-facing string goes through Fluent, in every locale.** Strings
   live in `locales/<tag>/*.ftl` and are read by both Rust and the frontend from
   the same files. `en-US` is the reference; `cargo xtask locale-check` fails if
   any other locale is missing a key or defines an extra one. Adding a language
   is adding a directory — no code changes. Never build a sentence by
   concatenation; use Fluent selectors for anything with a count.

6. **Secrets and trust are not negotiable.** Access codes never touch config or
   logs (see [SECURITY.md](SECURITY.md)); certificate pin changes are surfaced to
   the user, never silently accepted.

## What PandaSpy is not

Please don't open PRs for these — they are deliberate non-goals: mobile, any
cloud/Bambu-account integration, controlling printers (start/stop/modify a
print), slicing or model management, and telemetry of any kind. PandaSpy is
**read-only, local-network, desktop** monitoring.

One more: **do not port or translate the BambuBar project's source.** Implement
from observed protocol behaviour and public documentation only. The whole point
of this repository's structure is to avoid the duplicated, drift-prone codebases
that motivated it.
