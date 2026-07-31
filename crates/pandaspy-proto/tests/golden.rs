//! Golden tests over the recorded fixture corpus.
//!
//! Every file in `fixtures/reports/` is parsed, classified, and — when it
//! carries state — folded into a fresh accumulator and snapshotted. This is
//! the shared corpus that makes a single implementation honest: a firmware
//! change is a fixture, and a fixture that stops parsing the way it used to
//! is a failing test on every platform at once.
//!
//! To add a fixture, use `/fixture` (see `.claude/commands/fixture.md`).
//! To review snapshot changes: `cargo insta review` — and actually read the
//! diff; it is the parser telling you what it understood.

use pandaspy_proto::{Report, ReportKind, StateAccumulator};

#[test]
fn every_report_fixture_parses_and_matches_its_snapshot() {
    // The three-argument form: insta refuses `..` inside the *pattern*, so the
    // walk up to the repo root happens in the base path instead.
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reports");

    insta::glob!(&corpus, "*.json", |path| {
        let raw = std::fs::read(path)
            .unwrap_or_else(|err| panic!("cannot read fixture {}: {err}", path.display()));

        // Parsing must never fail: the corpus is the contract.
        let report = Report::parse(&raw)
            .unwrap_or_else(|err| panic!("fixture {} failed to parse: {err}", path.display()));

        match report.kind() {
            ReportKind::PrintPushStatus => {
                let mut accumulator = StateAccumulator::new();
                assert!(accumulator.apply(&report));
                let state = accumulator.state().unwrap_or_else(|err| {
                    panic!("fixture {} not viewable as state: {err}", path.display())
                });
                insta::assert_json_snapshot!(state);
            }
            ReportKind::Info => {
                let info = report
                    .info()
                    .unwrap_or_else(|| panic!("info fixture {} unreadable", path.display()));
                insta::assert_json_snapshot!(info);
            }
            other => {
                // Classification is still contract: snapshot what kind of
                // thing the parser decided this was.
                insta::assert_snapshot!(format!("{other:?}"));
            }
        }
    });
}
