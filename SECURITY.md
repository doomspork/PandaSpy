# Security

PandaSpy monitors Bambu Lab 3D printers on your local network. It handles two
things worth protecting — your printer **access codes** and the **trust
relationship** with each printer — and it makes some deliberate promises about
what it will and will not do with your network and your data. This document is
the security model: what PandaSpy guarantees, what it does not, and how to
report a problem.

## Reporting a vulnerability

Please report security issues **privately**, not in a public issue.

Use GitHub's private vulnerability reporting: the **Security** tab of this
repository → **Report a vulnerability**. That opens a private advisory visible
only to the maintainers.

Please include what you were doing, what you observed, and enough detail to
reproduce. We will acknowledge the report, investigate, and coordinate a fix and
disclosure timeline with you. Because PandaSpy is pre-1.0 and unsigned (see
below), fixes ship in the next tagged release rather than as back-ports.

## Supported versions

Pre-1.0. Only the latest released version receives security fixes. There is no
long-term-support branch yet.

## Privacy properties (what PandaSpy will not do)

These are design constraints, enforced in code and reviewed on every change —
not aspirations:

- **No cloud.** PandaSpy talks to printers on your local network. It uses no
  Bambu account, no Bambu Cloud API, and no relay through anyone's servers.
- **No telemetry, analytics, or crash reporting.** Of any kind. Nothing about
  your printers, your prints, or your usage leaves your machine.
- **Exactly two network destinations.** Printers on your LAN, and — only when
  you enable updates — the GitHub Releases endpoint to check for a new version.
  There is no third.

If you observe PandaSpy contacting anything else, treat it as a security bug and
report it.

## Access codes are passwords

A Bambu printer's LAN access code is printed on the printer's screen and is
effectively a password: anyone with it and network reach can read the printer's
full state. PandaSpy treats it accordingly.

- **Never stored in plaintext.** Access codes go to the operating system's
  secret store — **Keychain** on macOS, **Credential Manager** on Windows,
  **Secret Service** on Linux.
- **An encrypted fallback where no keyring exists.** On a headless or minimal
  Linux session with no Secret Service (no D-Bus), PandaSpy falls back to an
  authenticated-encrypted file rather than plaintext: a key derived with
  **Argon2id** from machine-bound material, sealing the codes with
  **ChaCha20-Poly1305**, with a fresh salt and nonce on every write. This is a
  deliberately _weaker_ guarantee than the OS keyring, and the settings screen
  says so when it is in use. See the threat model below.
- **Never written to config.** The on-disk config file (printer list, locale,
  preferences) is plain text and safe to inspect; it never contains an access
  code. A test (`printer_entries_never_carry_the_access_code`) guards this by
  construction.
- **Redacted in logs and debug output.** Credentials have a hand-written
  `Debug` that prints `<redacted>`, so an access code cannot reach a log file,
  a crash dump, or a GitHub issue through a stray `{:?}`.

### Encrypted-file fallback: threat model

Read this before relying on the fallback. It **raises the bar from "grep the
disk" to "run code as the target user", and no further.**

- **Protects against:** offline inspection of the disk, a backup, a snapshot,
  or a stolen drive; and another _local user_ reading the file, provided the
  file is owner-only (`0600`).
- **Does not protect against:** an attacker who can execute code _as the same
  user_. They can read the same machine-bound key material PandaSpy derives
  from, read the file, and derive the key exactly as PandaSpy does. Encryption
  cannot fix "the attacker is you." It also does not protect against an attacker
  who can substitute the machine key material.

> **Known gap:** creating the config directory `0600` (owner-only) needs
> per-OS code and is tracked as scaffolding work. Until it lands, the "other
> local users" guarantee above is conditional on the directory's default
> permissions. The OS keyring — used on every desktop platform — is unaffected;
> this caveat applies only to the headless-Linux fallback.

## Trust on first use (certificate pinning)

Bambu printers serve a **self-signed certificate** generated on the device.
There is no certificate authority to validate it against, so ordinary TLS
verification is meaningless here: it would either reject every printer or accept
every impostor. PandaSpy uses trust-on-first-use (TOFU) instead:

1. **First contact:** record the fingerprint (SHA-256 of the leaf certificate)
   the first time a printer is seen, and proceed.
2. **Every time after:** require the presented certificate to match the pinned
   fingerprint exactly.
3. **On a mismatch:** stop, and surface it to **you** as a decision — showing
   both the pinned and the presented fingerprint so you can compare them against
   the printer's screen. PandaSpy never silently accepts a changed certificate
   and never silently retries one.

A firmware reflash and a man-in-the-middle attacker look **identical** from
inside the process — both present a new certificate for a known serial. Only you
can tell them apart, which is why the mismatch is a prompt and not an automatic
action. If you did not just reflash or replace that printer, decline.

**The pin proves the certificate bytes; the handshake signature proves key
possession.** PandaSpy accepts the self-signed certificate _chain_ (it has to),
but it still verifies the TLS handshake signature against the certificate's
public key. Without that, an attacker could replay a printer's public
certificate — which is not secret — to satisfy the pin and then harvest the
access code you send. The signature check is what closes that hole, and the
access code is sent only _after_ the pin and signature both pass.

## Read-only by design

PandaSpy monitors; it does not control. It does not start, stop, pause, or
modify prints, and it does not upload files or G-code. Controlling a printer has
different and larger safety implications and is out of scope. A compromise of
PandaSpy exposes your printer's _state_ and its _access code_ — serious, but
bounded by the fact that PandaSpy itself never issues a command that moves the
machine.

## Code signing and updates

Release bundles are currently **unsigned**. On macOS, Gatekeeper requires a
right-click → Open on first launch; on Windows, SmartScreen may warn. The
in-app updater is wired but **disabled** until signing keys are provisioned,
because an unsigned auto-update is a worse risk than a manual one. When updates
are enabled, update manifests are cryptographically signed and PandaSpy verifies
that signature before installing. Until then, download releases only from this
project's official GitHub Releases page.
