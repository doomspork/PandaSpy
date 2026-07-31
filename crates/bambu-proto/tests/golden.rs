//! Golden tests over the recorded fixture corpus.
//!
//! Every file in `fixtures/reports/` is parsed into a [`PrinterState`] and
//! snapshotted. This is the shared corpus that makes a single implementation
//! honest: a firmware change is a fixture, and a fixture that stops parsing the
//! way it used to is a failing test on every platform at once.
//!
//! To add a fixture, use `/fixture` (see `.claude/commands/fixture.md`).
//! To review snapshot changes: `cargo insta review`.

use bambu_proto::PrinterState;

#[test]
fn every_report_fixture_parses_and_matches_its_snapshot() {
    // The three-argument form: insta refuses `..` inside the *pattern*, so the
    // walk up to the repo root happens in the base path instead.
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reports");

    insta::glob!(&corpus, "*.json", |path| {
        let raw = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("cannot read fixture {}: {err}", path.display()));

        // Parsing must never fail: the corpus is the contract.
        let state: PrinterState = serde_json::from_str(&raw)
            .unwrap_or_else(|err| panic!("fixture {} failed to parse: {err}", path.display()));

        insta::assert_json_snapshot!(state);
    });
}
