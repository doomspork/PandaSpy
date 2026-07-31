# Fixtures

Recorded printer payloads. This corpus is the reason Spool can be one codebase
instead of three.

## Why this directory exists

The project this one replaces shipped the same MQTT codec twice — once in Swift
for macOS, once in C# for Windows, the second described in its own docs as "a
1:1 port" of the first. There was no shared corpus, so a firmware change got
fixed on one platform and silently missed on the other. Nobody noticed until a
user did.

Here, there is one parser, and this directory is its contract. A payload that
parses on macOS parses identically on Windows and Linux, because it is the same
code running against the same bytes in the same test.

## Layout

```
fixtures/
  reports/     MQTT report payloads   -> crates/bambu-proto/tests/golden.rs
```

Add a subdirectory when you add a payload category (requests, SSDP
announcements, HMS bursts), and a golden test alongside it.

## The rule

**A protocol change requires a fixture.** Not "should have" — requires. If you
cannot produce a payload that exhibits the behaviour, you do not yet understand
the behaviour well enough to encode it. Reviewers should reject protocol changes
that arrive without one.

Corollary: never edit a fixture to make a test pass. A fixture is a recording of
something a real printer really said. If the parser disagrees with it, the
parser is wrong. If the recording is wrong, delete it and record again.

## Recording one

Use `/fixture`, which walks through it. The short version:

1. Capture the raw payload from the printer's MQTT topic.
2. Redact it (below).
3. Save as `fixtures/reports/<model>-<situation>.json`, e.g.
   `p1s-mid-print-with-ams.json`. The filename should say what makes this
   payload worth keeping.
4. `cargo insta review` to accept the new snapshot, then commit fixture and
   snapshot together.

## Redaction — required before committing

Payloads contain things that identify a specific machine and its owner.
Every fixture must have these replaced **before** it reaches git history:

| Field                | Replace with                                          |
| -------------------- | ----------------------------------------------------- |
| Serial numbers       | `00M09A000000000` (same length, same shape, not yours) |
| Access codes / tokens| `00000000`                                             |
| Wi-Fi SSIDs          | `REDACTED-SSID`                                        |
| IP and MAC addresses | `192.168.0.2` / `00:00:00:00:00:00`                    |
| Bambu account ids    | `0000000`                                              |
| Filenames of prints  | `redacted.3mf` — model names can be identifying        |

Keep the *shape*: same length, same character class, same nesting. A redaction
that changes a 15-character serial into `"X"` stops testing the thing you
recorded.

Redaction is not reversible and not optional. Git history is forever, and a
fixture committed with a live access code means rotating that printer's
credentials.
