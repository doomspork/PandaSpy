# PandaSpy

<p align="center">
  <img src="assets/mascot.png" alt="The PandaSpy mascot: a detective panda in a fedora and trench coat, inspecting a 3D-printer nozzle through a magnifying glass" width="220" />
</p>

A cross-platform desktop tray application that monitors Bambu Lab 3D printers
over the local network.

PandaSpy lives in the menu bar / system tray, finds printers on your LAN, and
shows print progress, temperatures, filament and errors — with no cloud account
and no round-trip through anyone's servers.

> **Status: pre-release.** The full monitoring stack — discovery, the MQTT
> client, the state parser, the tray shell and the window UI — is implemented
> and builds on all four targets. It has **not** had a signed public release
> yet (bundles are unsigned; the in-app updater is wired but off until signing
> keys exist), and the protocol layer is currently validated against
> _synthetic_ fixtures pending captures from real hardware. Expect to verify
> against your own printer.

## What it does

- **Finds your printers.** SSDP discovery across every usable network interface
  (not just the default one — VPNs, Docker bridges and multi-NIC machines are
  exactly where "use the default interface" finds nothing), with a subnet-probe
  fallback. When nothing turns up, it tells you _why_ rather than showing an
  empty list.
- **Monitors, live.** Per-printer cards show connection state, print progress,
  the current stage, nozzle/bed/chamber temperatures, layer and time remaining,
  and the task name. Four or more printers switch to a compact, expandable
  layout; drag to reorder.
- **AMS and filament.** Units and trays with empty slots preserved, filament
  type and colour, remaining level, and the active tray highlighted.
- **Errors in plain language.** Printer health (HMS) codes are resolved to
  readable descriptions from an embedded copy of Bambu's public table — no
  network lookup — with a wiki link for anything unrecognised.
- **Read-only, on purpose.** PandaSpy never starts, stops, pauses or modifies a
  print. An accidental click in a tray popover should not be able to kill a
  14-hour job. This is enforced in the protocol layer, not just the UI.
- **Localised.** Every string ships in English and Polish from a single shared
  set of translation files; adding a language is adding a directory.

## Privacy & security

PandaSpy talks to printers on your LAN and, only if you enable updates, to
GitHub Releases. That is the complete list of network destinations — **no cloud,
no Bambu account, no telemetry, no analytics, no crash reporting.**

Printer access codes are passwords and are treated as such: stored in the OS
keyring (Keychain / Credential Manager / Secret Service), or an
authenticated-encrypted file where no keyring exists — never in plaintext,
never in config, redacted in logs. Because printers serve a self-signed
certificate, PandaSpy pins the certificate it sees on first contact and treats
any later change as a decision only you can make. The full model is in
[SECURITY.md](SECURITY.md).

## Platforms

| Target                | Runner used in CI  |
| --------------------- | ------------------ |
| macOS (Apple silicon) | `macos-14`         |
| macOS (Intel)         | `macos-14` (cross) |
| Windows (x86-64)      | `windows-latest`   |
| Linux (x86-64)        | `ubuntu-22.04`     |

Linux is built on Ubuntu 22.04 deliberately: a newer builder would raise the
glibc floor and lock out users on stable distributions.

Because bundles are not yet code-signed, first launch needs a right-click → Open
on macOS, and Windows SmartScreen may warn.

## Quick start

```sh
pnpm install
pnpm tauri dev        # run the app with hot reload
pnpm tauri build      # a release bundle for this platform
cargo xtask check     # the pre-push gate (fmt, clippy, tests, locale parity, …)
```

Full setup, per-platform prerequisites and the architectural rules live in
[CLAUDE.md](CLAUDE.md). Read that before making changes — several parts of this
repo (the release wiring and the placement of platform-conditional code in
particular) are easy to break in ways CI catches late.

## Contributing

Contributions are welcome — start with [CONTRIBUTING.md](CONTRIBUTING.md). In
short: commits and PR titles follow
[Conventional Commits](https://www.conventionalcommits.org/) (scopes map to
crate names) and CI enforces one gate, `cargo xtask check`.

## Licence

MIT — see [LICENSE](LICENSE).

PandaSpy is an independent project. It is not affiliated with, endorsed by, or
supported by Bambu Lab. It is not a fork or port of any other monitoring tool;
the protocol handling is implemented from observed behaviour and public
documentation.
