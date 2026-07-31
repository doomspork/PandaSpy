//! Delta-sequence replay — the regression guard for the whole project.
//!
//! Each directory under `fixtures/sequences/` is a recorded conversation:
//! `00-*.json` is a pushall snapshot, everything after it a sparse delta,
//! applied in filename order. The final accumulated state is snapshotted and
//! key merge invariants are asserted explicitly.
//!
//! This is the test that catches the classic failure mode of this protocol —
//! a delta that silently discards state a snapshot delivered — so: any change
//! to merge behaviour must show up here as a reviewed snapshot diff, and new
//! merge edge cases get a new sequence directory, not a unit test alone.

use std::path::Path;

use pandaspy_proto::{
    ActiveTray, GcodeState, PrintStage, PrinterState, PrinterStatus, Report, StateAccumulator,
};

/// Replay every message in a sequence directory, in filename order.
fn replay(sequence: &str) -> (StateAccumulator, PrinterState) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/sequences")
        .join(sequence);

    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("cannot read sequence {}: {err}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    assert!(
        files.len() >= 2,
        "a sequence needs a pushall and at least one delta"
    );

    let mut accumulator = StateAccumulator::new();
    for file in &files {
        let raw = std::fs::read(file).unwrap();
        let report = Report::parse(&raw)
            .unwrap_or_else(|err| panic!("{} failed to parse: {err}", file.display()));
        assert!(
            accumulator.apply(&report),
            "{} is not a state report — sequences must contain only push_status",
            file.display()
        );
    }

    let state = accumulator
        .state()
        .expect("accumulated state must be viewable");
    (accumulator, state)
}

#[test]
fn p1s_print_lifecycle_replays_to_the_expected_final_state() {
    let (accumulator, state) = replay("p1s-print-lifecycle");
    assert_eq!(accumulator.reports_applied(), 7);

    // ── Fields the deltas changed ────────────────────────────────────────
    assert_eq!(state.gcode_state, Some(GcodeState::Finish));
    assert_eq!(state.status(), Some(PrinterStatus::Finished));
    assert_eq!(state.stage(), Some(PrintStage::Idle));
    assert_eq!(state.progress_percent(), Some(100));
    assert_eq!(state.layer_progress(), Some((137, 137)));

    // ── Absent-means-unchanged: set once by the pushall, never repeated ──
    assert_eq!(state.subtask_name.as_deref(), Some("benchy"));
    assert_eq!(state.total_layer_num, Some(137));
    assert_eq!(state.chamber_temper, Some(33.0));

    // ── The nested ams delta switched trays without destroying the units ─
    let ams = state.ams.as_ref().expect("ams survived the deltas");
    assert_eq!(
        ams.active_tray(),
        Some(ActiveTray::Slot { unit: 0, slot: 2 })
    );
    assert_eq!(ams.units.len(), 1, "unit list came only from the pushall");
    assert_eq!(ams.units[0].trays.len(), 4);
    assert_eq!(
        state.active_filament().and_then(|t| t.tray_type.as_deref()),
        Some("PETG")
    );

    // ── Present-and-null cleared exactly one field ───────────────────────
    assert_eq!(state.wifi_signal, None, "explicit null must clear");
    assert_eq!(state.sdcard, Some(true), "its neighbours must survive");

    // The full final state, reviewed as a snapshot. If a merge change alters
    // ANY field of the outcome, it shows up here.
    insta::assert_json_snapshot!("p1s-print-lifecycle-final", state);
}

#[test]
fn replaying_a_sequence_is_deterministic() {
    // Same fixture bytes, same result, every time, on every platform — the
    // property the whole single-implementation argument rests on.
    let (_, first) = replay("p1s-print-lifecycle");
    let (_, second) = replay("p1s-print-lifecycle");
    assert_eq!(first, second);
}

#[test]
fn intermediate_states_are_sane_mid_sequence() {
    // Replay only the first three messages and verify the mid-print picture:
    // catches a merge that only *ends* right by accident.
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sequences/p1s-print-lifecycle");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();

    let mut accumulator = StateAccumulator::new();
    for file in files.iter().take(3) {
        let raw = std::fs::read(file).unwrap();
        accumulator.apply(&Report::parse(&raw).unwrap());
    }

    let state = accumulator.state().unwrap();
    assert_eq!(state.status(), Some(PrinterStatus::Printing));
    assert_eq!(state.progress_percent(), Some(42), "02-delta applied");
    assert_eq!(state.bed_temper, Some(60.1), "01-delta applied");
    assert_eq!(
        state.wifi_signal.as_deref(),
        Some("-52dBm"),
        "not yet nulled at this point in the sequence"
    );
}
