# PandaSpy

A cross-platform desktop tray application that monitors Bambu Lab 3D printers
over the local network.

PandaSpy lives in the menu bar / system tray, discovers printers on your LAN, and
shows print progress, temperatures and errors without a cloud round-trip.

> **Status: scaffolding.** The repository currently builds an empty shell on
> macOS, Windows and Linux. No printer protocol, discovery or UI is implemented
> yet.

## Platforms

| Target                | Runner used in CI |
| --------------------- | ----------------- |
| macOS (Apple silicon) | `macos-14`        |
| macOS (Intel)         | `macos-13`        |
| Windows (x86-64)      | `windows-latest`  |
| Linux (x86-64)        | `ubuntu-22.04`    |

Linux is built on Ubuntu 22.04 deliberately: a newer builder would raise the
glibc floor and lock out users on stable distributions.

## Quick start

```sh
pnpm install
pnpm tauri dev        # run the app
cargo xtask check     # the pre-push gate (fmt, clippy, tests, locale parity)
```

Full setup, per-platform build instructions and the architectural rules live in
[CLAUDE.md](CLAUDE.md). Read that before making changes — several parts of this
repo (the release wiring and the placement of platform-conditional code in
particular) are easy to break in ways CI catches late.

## Contributing

Commits and pull-request titles follow
[Conventional Commits](https://www.conventionalcommits.org/) and are enforced by
CI. Scopes map to crate names; see CLAUDE.md § Commit conventions.

## Licence

MIT — see [LICENSE](LICENSE).

PandaSpy is an independent project. It is not affiliated with, endorsed by, or
supported by Bambu Lab.
