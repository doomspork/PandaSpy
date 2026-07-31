---
description: Record, redact and commit a new printer payload fixture
argument-hint: "[what the payload shows, e.g. 'P1S mid-print with AMS']"
allowed-tools: Bash(cargo:*), Read, Write, Edit
---

Add a recorded printer payload to `fixtures/` so the parser has a contract for
it. Situation being captured: **$ARGUMENTS**

Read `fixtures/README.md` first — it holds the redaction table this procedure
depends on.

## Why the ceremony

Any change to protocol parsing requires a fixture. Not "should have" —
requires. The project this one replaces had two implementations of the same
codec and no shared corpus, so a firmware change got fixed on one platform and
silently missed on the other. This directory is what makes that impossible
here: one parser, one corpus, one test that runs on every platform at once.

## 1. Capture

Subscribe to the printer's MQTT report topic and capture one raw payload.
Anything that speaks MQTT over TLS works; `mosquitto_sub` with
`--insecure -u bblp -P <access-code>` is the usual choice. Capture the whole
message, not an excerpt — the fields you did not think were interesting are
often the ones that change.

Prefer a payload that shows something the corpus does not already cover. A
seventh idle-printer report teaches the parser nothing.

## 2. Redact — before the file is ever staged

Apply every row of the table in `fixtures/README.md`: serial numbers, access
codes, SSIDs, IP and MAC addresses, account ids, print filenames.

Keep the **shape**. Same length, same character class, same nesting. A
15-character serial replaced by `"X"` stops testing what you recorded.

This is not reversible and not optional. Git history is forever, and a fixture
committed with a live access code means that printer's credentials have to be
rotated.

## 3. Save

`fixtures/reports/<model>-<situation>.json` — e.g. `p1s-mid-print-with-ams.json`.
The filename should say what makes this payload worth keeping.

If this is the first real recording, delete `fixtures/reports/scaffold-minimal.json`;
it is a placeholder whose field names do not match the wire format.

## 4. Accept the snapshot

```
cargo test -p pandaspy-proto
cargo insta review
```

Read the snapshot diff properly. It is the parser telling you what it
understood, and it is the last point at which "that field is being silently
dropped" is easy to notice.

## 5. Commit

Fixture and snapshot in the same commit, scoped `fixtures` or `proto`:

```
feat(proto): parse AMS slot state

Adds fixtures/reports/p1s-mid-print-with-ams.json, recorded from
firmware 01.07.00.00, redacted per fixtures/README.md.
```

Mention the firmware version in the body if you know it. Six months from now
that line is the only way to tell whether a parsing difference is a bug or a
firmware change.

## Never

Do not edit an existing fixture to make a test pass. A fixture is a recording
of something a real printer really said. If the parser disagrees with it, the
parser is wrong. If the recording is wrong, delete it and record again.
