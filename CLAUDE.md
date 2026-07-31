# CLAUDE.md

Guidance for Claude Code, and for anyone else picking this repository up cold.

PandaSpy is a cross-platform desktop tray application that monitors Bambu Lab 3D
printers over the local network. Rust + Tauri v2, targeting macOS (aarch64 and
x86-64), Windows (x86-64) and Linux (x86-64).

**Current state: M2 (discovery) complete.** `pandaspy-proto` implements the
wire contract, the state merge, the AMS model and HMS resolution;
`pandaspy-discovery` implements multi-interface SSDP, the subnet-probe
fallback and structured zero-result diagnostics. Both are tested against a
synthetic fixture corpus (real captures pending — see `fixtures/README.md`).
The client, persistence and the real UI are not implemented; those
placeholders are marked `TODO(scaffold)`. Protocol facts asserted from
community documentation rather than a capture are marked `TODO(fixture)`.

---

## The one architectural rule

> **Platform-conditional compilation is permitted only in `src-tauri/` and
> `crates/pandaspy-store/`. Anywhere else, it means the design is wrong.**

That covers `#[cfg(target_os)]`, `cfg!(target_os)`, `#[cfg(unix)]`,
`#[cfg(windows)]`, `target_family`, `target_arch`, and
`[target.'cfg(…)'.dependencies]` tables in `Cargo.toml`. It applies to `xtask/`
too.

`cargo xtask cfg-check` enforces it, and CI runs it on every pull request. The
check blanks comments and string literals before scanning, so documentation
that discusses the rule does not trip it.

### Why it exists

There is an existing MIT-licensed project, BambuBar, that solves this problem
with two entirely separate codebases: Swift/AppKit for macOS and C#/.NET for
Windows. Its own documentation describes the Windows MQTT codec as "a 1:1 port"
of the Swift one. The MQTT codec, SSDP discovery, subnet probe, certificate
pinning, status parser and HMS resolver all exist twice, with no shared test
corpus. A firmware change gets fixed in one and silently missed in the other.
Nobody notices until a user does.

PandaSpy is not a fork or a port of that project — implement from protocol
behaviour and public documentation only, never by translating its source. The
layout here exists specifically to make that duplication structurally
impossible. One protocol implementation, one fixture corpus, one test run that
covers every platform at once.

The rule is what keeps it that way, because divergence never arrives as a
decision. It arrives as one `#[cfg(target_os = "windows")]` that seemed
reasonable at the time.

### When you actually need platform-specific behaviour

Inject it, do not branch on it.

`pandaspy-discovery` and `pandaspy-client` are already shaped for this: sockets sit
behind `SsdpSocket`/`SsdpStack`/`PortProbe`, and pins behind `PinStore`. Tests
supply fakes. The _real_ implementations come in two flavours, and the
distinction matters:

- **Portable I/O lives with its algorithm crate** (`pandaspy-discovery::net` is
  tokio + socket2 + rustls using cross-platform APIs only). One socket
  implementation, compiled identically for all three platforms, is the
  single-codebase argument applied to I/O.
- **Platform-tuned I/O, if a platform ever genuinely needs it, is injected by
  `src-tauri`** through the same traits. If multicast needs different setup on
  Windows, that belongs in a Windows implementation supplied by `src-tauri`,
  not in a `cfg` inside the discovery crate. (So far, none has been needed —
  even SO_REUSEPORT was avoided rather than platform-gated.)

Where the difference is genuinely runtime rather than compile-time, probe at
runtime. `xtask/src/main.rs` does exactly this to find pnpm, which is
`pnpm.cmd` on Windows: it tries each name and handles `NotFound`. That is not
only rule-compliant, it is more correct — the right executable name depends on
how pnpm was installed, not on what the binary was compiled for.

`pandaspy-store` is the one exception because macOS Keychain, Windows Credential
Manager and Linux Secret Service are genuinely three different things, and no
abstraction makes "Keychain" and "Credential Manager" the same word. Even there
the allowance is narrow: it covers backend _selection_ and platform naming, not
logic. See `crates/pandaspy-store/src/secrets.rs` for the shape.

Adding a third entry to the allow-list is a design decision to argue about in a
pull request, not a formality.

---

## Crate boundaries

```
crates/
  pandaspy-proto/       pure: wire contract, report accumulator, AMS + HMS model
  pandaspy-discovery/   SSDP + subnet probe, I/O behind traits
  pandaspy-client/      TLS + MQTT session, TOFU pinning, reconnect/backoff
  pandaspy-store/       config + secrets, behind swappable backends
src-tauri/           thin: tray, window, commands, event bridge
src/                 SvelteKit frontend
locales/             Fluent .ftl, shared by Rust AND the frontend
fixtures/            recorded printer payloads (redacted) for golden tests
xtask/               repo automation (sources; manifest is the repo root)
```

### `pandaspy-proto` — pure

No I/O, no async runtime, no clock, no randomness, no platform APIs. Given the
same bytes it produces the same value on every platform, forever.

This is enforced structurally, not by review: CI builds the crate for
`wasm32-unknown-unknown`, a target where sockets, files and the system clock do
not exist. If that job fails, something impure got in. **Find it and move it —
do not add a shim or a feature flag to make wasm happy.** The check has no value
if it can be worked around.

Purity has API consequences worth knowing before you reach for a clock:
`PrinterState::eta(now)` takes the current time as a parameter, and the two
HMS/error text tables are snapshots of Bambu's public table embedded at
compile time (`assets/hms/*.json`) — resolution never touches the network.
That last part is a privacy commitment, not an optimisation: PandaSpy speaks
to printers on the LAN and to nothing else.

The crate is read-only by construction. `wire::Request` can express exactly
two commands — `pushall` and `get_version` — so print control cannot be sent
by accident from anywhere in the app. Widening that enum is a product
decision, not a refactor.

### `pandaspy-discovery` — finding printers

SSDP first, subnet probe as the fallback. Bambu printers announce on
`239.255.255.250:2021` (their port, not the standard 1900) and answer
M-SEARCH; the engine runs a passive listener and per-interface active
searches simultaneously, then falls back to knocking on TCP 8883 across the
local subnet and reading the TLS certificate — its CN is the device serial.

Three design decisions to know before touching it:

- **Every usable interface, always.** The passive listener joins the
  multicast group on each one; each gets its own search socket. Multi-homed
  machines (VPN, Docker bridge, two NICs) are where "use the default
  interface" silently finds nothing — the failure mode this crate exists to
  prevent. Partial failure is tolerated and recorded per interface.
- **Diagnostics are a product feature, not logging.** Every run returns
  `DiscoveryDiagnostics`; `verdict()` collapses it to found / no-usable-
  interface / permission-denied / no-response for the troubleshooting UI.
  Only sends, receipts and failures count as evidence — a socket that merely
  _opened_ proves nothing, which is exactly how macOS Local Network denial
  hides (sockets open, joins succeed, sends die with "no route to host").
- **The engine is generic over `SsdpStack` + `SsdpSocket` + `PortProbe` +
  `InterfaceSource`** and is tested end to end against scripted fakes under
  a paused tokio clock (`tests/engine.rs`) — deterministic and instant. The
  real transports live in `net.rs`, portable APIs only. The subnet walk
  clamps anything wider than /24 to our own /24 and caps total targets: a
  fallback, not a port scanner.

The transport traits return `impl Future<Output = …> + Send` rather than using
`async fn`. That is deliberate: `async fn` in a public trait leaves the future's
auto-traits unspecified, so callers could not spawn the result on a
multi-threaded runtime. The cost is that the traits are not `dyn`-safe, which
is fine — the engine is generic, and there is one real implementation plus
scripted fakes.

### `pandaspy-client` — talking to a printer

Connection lifecycle, not payload semantics. It moves bytes and manages
reconnects; it does not know what a nozzle is.

Trust is on-first-use, because printers serve a self-signed certificate
generated on the device. There is no chain to validate, so ordinary TLS
verification either rejects every printer or accepts every impostor. Instead:
record the fingerprint the first time, require it to match forever after, and
surface a change to the user as a decision rather than retrying it. A firmware
reflash and an attacker look identical from inside the process; only the user
can tell them apart.

`Backoff` is deliberately jitter-free. Jitter needs randomness and randomness
makes a scheduler untestable, so the caller applies jitter to the returned
`Duration`.

### `pandaspy-store` — persistence

Config and secrets are separate concerns and stay separate. Config is plain
text on disk and safe to inspect; access codes go to the OS secret store. See
the test `printer_entries_never_carry_the_access_code`, which guards that by
construction.

### `src-tauri` — the platform layer

Tray, window, commands, the Rust↔frontend event bridge, and the concrete
implementations of the domain crates' traits. Keep it thin. If you are writing
something a test would want to run without a GUI, it is in the wrong crate.

---

## Serde discipline

Bambu's protocol is undocumented, differs between printer models, and changes
without notice across firmware releases. **A strict parser is a liability**: it
turns a firmware update into a bricked app for every user at once, on a
schedule you do not control.

Three rules, no exceptions:

**Every field is `Option<T>`.** The printer sends one large report on connect
and then partial reports containing only what changed. `None` means "the
printer did not mention this", which is _different_ from "the printer said
zero". Collapsing the two makes a paused print look like a finished one.

**Unknown enum values are preserved, not rejected.** Every wire enum keeps an
`Unknown(String)` variant that round-trips the raw value:

```rust
#[derive(Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum JobStage { Idle, Running, /* … */ Unknown(String) }
```

The `from`/`into` pair is what makes this work — serde treats the enum as a
plain string and the conversions decide how to read it. `#[serde(other)]` is
not a substitute: it collapses every unrecognised value into one
indistinguishable variant, so logs and the UI can no longer show what the
printer actually said.

**`#[serde(deny_unknown_fields)]` is banned.** Use `#[serde(default)]` on the
struct so missing keys deserialise to `None` rather than failing.

Two implementation consequences in `pandaspy-proto`:

**Scalars are parsed leniently** (`src/de.rs`). The same field arrives as
`28.5`, `"28.5"` or `""` depending on model and firmware; ids are strings on
X1-era firmware and integers on A1-era; bitmasks are hex strings. An
unreadable scalar becomes `None`, never a parse failure.

**The merge is defined over JSON documents, not typed structs**
(`src/merge.rs`). The printer sends one `pushall` snapshot, then sparse
deltas. Reports deep-merge into an accumulated document
(`StateAccumulator`), and the typed `PrinterState` is deserialised as a
_view_ of that document. Absent keys mean "unchanged"; a present `null`
clears; objects recurse; arrays replace wholesale. This is what makes the
"absent ≠ null" distinction real, and it means fields the typed view does
not model yet still accumulate faithfully (visible via
`StateAccumulator::document()`).

The tests in `src/state.rs`, `src/de.rs` and `src/merge.rs` pin these
behaviours; `tests/sequence.rs` replays pushall + delta conversations from
`fixtures/sequences/` and is the regression guard for the merge.

One connection-layer contract: deltas lost while disconnected are
unrecoverable, so the client must `StateAccumulator::reset()` on reconnect
and request a fresh `pushall`.

---

## Fixtures and snapshot testing

`fixtures/` holds recorded printer payloads. It is the shared corpus that makes
a single implementation honest, and the direct answer to the BambuBar drift
story above.

**Any protocol change requires a fixture.** Not "should have" — requires. If
you cannot produce a payload that exhibits the behaviour, you do not yet
understand the behaviour well enough to encode it. Reviewers should reject
protocol changes that arrive without one.

`crates/pandaspy-proto/tests/golden.rs` parses every file in `fixtures/reports/`
and snapshots the result with `insta`. Workflow:

```sh
cargo test -p pandaspy-proto
cargo insta review          # read the diff properly, then accept
```

Read the snapshot diff rather than accepting it reflexively. It is the parser
telling you what it understood, and it is the last easy moment to notice a
field being silently dropped.

**Never edit a fixture to make a test pass.** A fixture is a recording of
something a real printer really said. If the parser disagrees with it, the
parser is wrong. If the recording is wrong, delete it and record again.

Redaction rules — serials, access codes, SSIDs, addresses — are in
`fixtures/README.md` and are **mandatory before staging**. Git history is
forever; a fixture committed with a live access code means rotating that
printer's credentials. Use `/fixture` for the guided version.

Two fixture categories exist today:

- `fixtures/reports/*.json` — single report-topic messages, in real wire
  format (`{"print": {...}}` envelopes), golden-tested by `tests/golden.rs`.
- `fixtures/sequences/<name>/NN-*.json` — a pushall followed by deltas,
  replayed in filename order by `tests/sequence.rs`. Merge edge cases get a
  new sequence, not just a unit test.

Every current fixture is **synthetic** (prefixed `synthetic-`), assembled
from community protocol documentation to exercise the model matrix until
real captures arrive. When a real capture of the same situation lands,
delete the synthetic file rather than keeping both — a fixture corpus that
mixes recordings with guesses is worse than one that is honestly partial.

---

## Internationalisation

`locales/<bcp-47-tag>/<bundle>.ftl` is the single source of truth for every
user-facing string, and **both sides of the app read the same files**:

- Rust (`src-tauri/src/i18n.rs`) embeds them with `include_dir` for the tray
  menu and OS notifications.
- The frontend (`src/lib/i18n.ts`) inlines them with a Vite glob.

There is no generated string table and no copy step. `en-US` is the reference
locale; `cargo xtask locale-check` fails if any other locale is missing a key it
defines, **or defines one it does not** — the second direction is what catches
typos in key names.

**Adding a language is adding a directory.** No Rust, TypeScript or build config
changes; both sides discover locales by scanning. Use `/newlocale`. If that ever
seems to need a code change, the plumbing has regressed.

Two things to know:

- `src-tauri/build.rs` emits `cargo:rerun-if-changed=../locales`. Cargo cannot
  see through `include_dir!`, so without it an edited translation would not
  trigger a rebuild — a bug that looks exactly like "my translation is being
  ignored". Do not remove that line.
- Both sides set `useIsolating(false)`. Fluent otherwise wraps placeables in
  invisible Unicode directional isolate marks, which break string comparison in
  tests and menu items. Turn it back on when PandaSpy ships an RTL locale.

Proper nouns (`PandaSpy`, `Keychain`, `Credential Manager`) are not translated.
Never assemble a sentence by concatenation — use Fluent selectors, because
Polish plural rules cannot be expressed as `"Found " + n + " printers"`.

---

## Running, testing and building

### The gate

```sh
cargo xtask check
```

One command, and it is the same definition of "passing" that CI uses: fmt,
clippy with `-D warnings`, tests, the wasm purity check, locale parity, the cfg
check, and the frontend's format/lint/typecheck. Run it before pushing. `/check`
does this and explains the non-obvious failures.

Individual pieces:

```sh
cargo test --workspace
cargo xtask locale-check
cargo xtask cfg-check
cargo check -p pandaspy-proto --target wasm32-unknown-unknown
cargo deny --workspace check     # needs `cargo install cargo-deny --locked`
pnpm run check                   # prettier + eslint + svelte-check
```

### Day to day

```sh
pnpm install
pnpm tauri dev        # app with frontend hot reload
pnpm tauri build      # release bundle for the host platform
```

`pnpm dev` alone runs only the frontend on <http://localhost:1420>, which is
useful for UI work but has no Tauri commands available.

If TypeScript cannot resolve `$lib` or `$app/*`, run `pnpm svelte-kit sync` —
`.svelte-kit/tsconfig.json` is generated and not committed.

### Per-platform prerequisites

**macOS** — Xcode Command Line Tools (`xcode-select --install`). Nothing else.
Bundles land in `target/release/bundle/{macos,dmg}/`. Builds are unsigned for
now, so Gatekeeper needs a right-click → Open on a fresh download.

**Windows** — Visual Studio Build Tools with the "Desktop development with C++"
workload, and the `x86_64-pc-windows-msvc` Rust toolchain. WebView2 ships with
Windows 11 and recent Windows 10; older installs need the Evergreen runtime.
Bundles land in `target/release/bundle/{msi,nsis}/`.

**Linux** —

```sh
sudo apt-get install -y \
  build-essential file libayatana-appindicator3-dev libgtk-3-dev \
  librsvg2-dev libssl-dev libwebkit2gtk-4.1-dev libxdo-dev patchelf
```

Bundles land in `target/release/bundle/{deb,appimage}/`. The tray needs a
StatusNotifierItem host; on desktops without one the icon will not appear and
`tray::install` reports it.

CI builds Linux on **ubuntu-22.04 deliberately, never `ubuntu-latest`**. The
glibc a binary links against becomes the minimum its users need, so a newer
builder silently drops everyone on a stable distribution.

### Toolchain

`rust-toolchain.toml` pins an exact stable release, and CI reads the version out
of that file rather than repeating it. Bumping Rust is a one-line change there,
as its own `chore(deps):` commit. The Node version lives in `.nvmrc` for the
same reason.

---

## Commit conventions

[Conventional Commits](https://www.conventionalcommits.org/), enforced by
commitlint on both pull-request titles and the commits inside them. Both,
because either can become the permanent history: a squash merge keeps the
title, a merge commit keeps the commits, and Release Please reads whichever
survives.

Scopes map to crates:

| Scope       | Covers                                |
| ----------- | ------------------------------------- |
| `proto`     | `crates/pandaspy-proto`               |
| `discovery` | `crates/pandaspy-discovery`           |
| `client`    | `crates/pandaspy-client`              |
| `store`     | `crates/pandaspy-store`               |
| `tauri`     | `src-tauri`                           |
| `xtask`     | `xtask`                               |
| `ui`        | the SvelteKit frontend                |
| `i18n`      | `locales/` and the Fluent plumbing    |
| `fixtures`  | the recorded payload corpus           |
| `ci`        | workflows and the composite action    |
| `deps`      | dependency bumps                      |
| `release`   | Release Please, versioning, packaging |

The scope list is in `commitlint.config.js`. A scope is optional; an
unrecognised one is rejected, because it is nearly always a typo or a sign the
change belongs somewhere else.

Only `feat`, `fix`, `perf` and `revert` appear in the changelog. Commit bodies
should say _why_; the diff already says what.

---

## Versioning and releases

Both halves of this are easy to break, and the failure modes are quiet.

### One version, one place

**`src-tauri/Cargo.toml` holds the app version.** Nothing else does:

- `src-tauri/tauri.conf.json` has **no** `version` key and inherits it.
- the root `package.json` has **no** `version` field.

The `0.1.0` in `Spool_0.1.0_aarch64.dmg` is never typed into the Tauri config.
If you find yourself adding a version number to a second file, stop.

Release Please keeps every crate and `Cargo.lock` in step. Three details make
that work, and each one silently breaks it if changed:

1. **The workspace root is also a package** (`xtask`). Release Please's `rust`
   strategy throws `is not a package manifest` on a virtual manifest, so the
   root needs a `[package]` section. Rather than invent a dummy crate, the root
   _is_ the xtask crate — which is why `xtask/` has sources but no `Cargo.toml`.
2. **`workspace.members` is listed explicitly, never globbed.** Release Please
   reads that list verbatim and cannot expand `crates/*`; a globbed member is
   skipped without a warning and its version drifts.
3. **Every crate declares a literal `version = "x.y.z"`.**
   `version.workspace = true` would defeat the rewriter. Internal dependencies
   in `[workspace.dependencies]` are path-only with no version, so there is no
   pin that can go stale.

The package path in `release-please-config.json` is `"."`, not `"src-tauri"`,
and that is not cosmetic. A non-root path makes Release Please filter commits by
that path, so a `feat(proto):` commit touching only `crates/pandaspy-proto` would
produce no release and no changelog entry. A non-root path also registers the
lockfile updater at `<path>/Cargo.lock`, which does not exist in a workspace —
so `Cargo.lock` would never be bumped and the next `cargo build --locked` would
fail on a version mismatch.

`ci.yml` runs `cargo metadata --locked`, which is what catches that whole class
of regression.

### Release ordering

The build workflow **attaches to a release that already exists**. It never
creates one — two things creating releases is how you end up with `v0.4.0` and
`v0.4.0-1`.

```
push to main
  └─ release-please.yml opens or updates a release PR
       └─ you merge it
            └─ release-please.yml tags and creates a DRAFT release
                 └─ you press Publish            ← a human, on purpose
                      └─ release: published fires
                           └─ release.yml resolves the release id
                                └─ build.yml builds all four targets
                                     └─ tauri-action attaches them via releaseId
```

The human step is not ceremony. A release created by `GITHUB_TOKEN` does not
trigger other workflows — a GitHub rule, not an oversight — so the chain needs a
person in it regardless. Making it a deliberate Publish also means the draft
gets read before it goes out.

Note that a draft release does not create the git tag until it is published.

To rebuild a release's artifacts, run **Release** via `workflow_dispatch` with
the tag. To test packaging without any release, run **Build** via
`workflow_dispatch`; bundles come back as workflow artifacts.

---

## CI

| Workflow             | Trigger                       | Does                                                                                           |
| -------------------- | ----------------------------- | ---------------------------------------------------------------------------------------------- |
| `ci.yml`             | PR, push to main, merge queue | fmt, clippy, nextest, wasm purity, repo rules, cargo-deny, frontend; calls `build.yml` on main |
| `build.yml`          | reusable / dispatch           | the four-target bundle matrix                                                                  |
| `release-please.yml` | push to main                  | release PR, tag, draft release                                                                 |
| `release.yml`        | release published / dispatch  | attaches bundles to the existing release                                                       |
| `commitlint.yml`     | PR, merge queue               | Conventional Commits on the title and the commits                                              |

`.github/actions/setup` is the one place that knows about the Rust toolchain,
Node/pnpm and the Tauri Linux libraries. Fix dependency problems there, not in
individual jobs.

`--locked` is on every cargo invocation on purpose — see the versioning section.

The cross-platform bundle matrix runs only on pushes to main, so pull requests
get fast feedback while "does it still build on Windows?" is answered before a
release rather than during one. Remove the `if` on the `bundles` job to run it
on pull requests too.

---

## Non-goals

The maintainer's list for the MVP. Do not build these:

- **Print control of any kind — pause, resume, stop.** Deliberately excluded:
  an accidental click in a tray popover killing a 14-hour print is a bad
  first impression. Read-only for v1. The protocol layer enforces this
  structurally — see `wire::Request`.
- **Camera / RTSP streaming.**
- **FTPS file browsing or gcode upload.**
- **Bambu Cloud accounts.** PandaSpy talks to printers on the local network.
  No relay through anyone's servers.
- **Mobile.** Desktop only. `cargo tauri icon` generates iOS and Android
  assets; they were deleted rather than committed.
- **AMS filament editing.** Display only.
- **Multi-user.**
- **Forking or porting BambuBar.** Implement from protocol behaviour and
  public documentation. Do not translate its source.
- **Per-platform codebases or per-platform behaviour.** See the architectural
  rule. One implementation, one corpus. (The tray/popover presentation does
  differ per platform by design — that is `src-tauri`'s job and stays there.)
- **Telemetry, analytics or crash reporting of any kind.** The only network
  peers PandaSpy will ever have are printers on the LAN and the GitHub
  Releases update check. This is a stated privacy property, not a default.

---

## Known gaps for the next agent

Everything below is deliberate scaffolding debt, not oversight.

- **No code signing.** `build.yml` has clearly marked `TODO(signing)` blocks for
  `APPLE_CERTIFICATE`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_TEAM_ID`,
  `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` and the
  Windows secrets. These are being provisioned separately.
- **Updater is off.** `latest.json` generation is wired but gated behind the
  repository variable `PANDASPY_UPDATER_ENABLED`, because it needs signed bundles.
  Turning it on also needs a `plugins.updater` public key in `tauri.conf.json`.
- **Confirm the bundle identifier before the first release.** It is currently
  `io.github.seancallan.pandaspy`. Changing it after a release breaks updates and
  orphans user data on macOS.
- **macOS tray icon should be a template image.** The colour app icon is used
  for now; it looks right on Windows and Linux but will not tint with the menu
  bar. `src-tauri/app-icon.png` is the source; regenerate with
  `pnpm tauri icon src-tauri/app-icon.png -o src-tauri/icons`.
- **macOS activation policy is not set.** A menu-bar app usually wants
  `ActivationPolicy::Accessory` so it has no Dock icon. Left as-is so the
  scaffold is obviously launchable.
- **The CSP allows `'unsafe-inline'` for scripts and styles**, because
  SvelteKit emits an inline boot script. Tightening it means configuring
  SvelteKit's `kit.csp` hashes and reconciling them with Tauri's CSP. Worth
  doing before the app handles printer credentials.
- **`pandaspy-proto`, `pandaspy-discovery` and `pandaspy-client` are not yet
  dependencies of `src-tauri`.** Add each in the commit that first wires it in,
  so `cargo deny` reviews its dependency tree in context.
- **`cargo deny` MUST be run with `--workspace`.** The repo root is a real
  package (`xtask`), not a virtual manifest, so a bare `cargo deny check`
  takes `xtask` as the sole graph root and silently audits ~26 crates —
  none of rustls, ring, tokio or the Tauri tree. `ci.yml` passes
  `--workspace`; the local command in this file does too. A green audit that
  looked at 4% of the tree is the failure mode to fear here.
- **16 `unmaintained` advisories are ignored in `deny.toml`, all via Tauri**
  (the GTK3 binding family + `proc-macro-error` on Linux, and the retired
  `rust-unic` crates via `tauri-utils`→`urlpattern`). Every one is a
  no-fix-available notice, not a vulnerability. Re-audit the whole `ignore`
  list on any Tauri version bump — an ignore that outlives its cause is a
  silenced alarm, and cargo-deny flags a stale entry as
  `advisory-not-detected` (a warning, so read the warnings after a bump).
- **No MQTT dependency yet.** TLS arrived with M2 and it is `rustls` with the
  **ring** provider on purpose — one crypto stack, no cmake/nasm build
  dependency, and `deny.toml` denies `openssl-sys` and `native-tls` outright,
  because a system trust store means three different certificate behaviours
  on three platforms. When M3 adds an MQTT client it must sit on the same
  rustls configuration. The certificate-verification-disabling verifier in
  `pandaspy-discovery::net` is for _reading_ a printer's identity during
  discovery only; the MQTT path gets TOFU pinning, never that verifier.
- **The macOS Local Network permission symptom is a heuristic.**
  `DiscoveryVerdict::PermissionDenied` treats all-sends-failing with
  `HostUnreachable` as denial (plus real `PermissionDenied` kinds). Verify
  the exact symptom from a bundled build in M5, when
  `NSLocalNetworkUsageDescription` is wired into Info.plist — it is marked
  `TODO(fixture)` in `diag.rs`.
