//! The report merge — the part of this protocol that is easy to get subtly
//! wrong.
//!
//! A printer sends one `pushall` full snapshot and then a stream of sparse
//! deltas. X1-series firmware re-sends nearly everything every time; P1 and A1
//! firmware sends only what changed. Naive field assignment (`state.x =
//! delta.x`) silently discards data every time a delta omits a field.
//!
//! The merge is defined over JSON documents, not over typed structs, and that
//! is a deliberate architectural choice:
//!
//! * **absent** — the delta does not mention the key: the old value is kept.
//! * **present and null** — the delta explicitly nulls the key: the value is
//!   cleared. Distinct from absent, and the distinction is load-bearing.
//! * **present** — objects merge recursively; **everything else, including
//!   arrays, replaces wholesale**.
//!
//! Arrays replace rather than merge because there is no sound way to merge
//! them without understanding each one's identity semantics — an AMS `tray`
//! array merged by index against a reordered delta would corrupt slots. The
//! firmware sends complete arrays within whatever subtree it includes, so
//! replacement is both safe and what the printer expects. (This matches the
//! behaviour long verified across firmwares by community integrations.)
//!
//! Merging at the JSON layer also means fields this crate does not model yet
//! are still accumulated faithfully — they survive into
//! [`crate::StateAccumulator::document`] for diagnostics, instead of being
//! silently dropped by a typed struct that never knew about them.

use serde_json::Value;

/// Merge `delta` into `base` with the semantics above.
pub fn deep_merge(base: &mut Value, delta: &Value) {
    match (base, delta) {
        (Value::Object(base_map), Value::Object(delta_map)) => {
            for (key, delta_value) in delta_map {
                match base_map.get_mut(key) {
                    // Object onto object: recurse.
                    Some(base_value) if base_value.is_object() && delta_value.is_object() => {
                        deep_merge(base_value, delta_value);
                    }
                    // Everything else — scalars, arrays, nulls, or a type
                    // change — replaces.
                    _ => {
                        base_map.insert(key.clone(), delta_value.clone());
                    }
                }
            }
        }
        (base_slot, delta_value) => *base_slot = delta_value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::deep_merge;

    fn merged(base: Value, delta: Value) -> Value {
        let mut base = base;
        deep_merge(&mut base, &delta);
        base
    }

    #[test]
    fn absent_means_unchanged() {
        let out = merged(
            json!({"bed_temper": 60.0, "layer_num": 12}),
            json!({"layer_num": 13}),
        );
        assert_eq!(out, json!({"bed_temper": 60.0, "layer_num": 13}));
    }

    #[test]
    fn present_and_null_means_cleared_which_is_not_the_same_as_absent() {
        // The distinction the merge exists to model. `{}` keeps the value;
        // `{"x": null}` clears it.
        let kept = merged(json!({"wifi_signal": "-52dBm"}), json!({}));
        assert_eq!(kept, json!({"wifi_signal": "-52dBm"}));

        let cleared = merged(
            json!({"wifi_signal": "-52dBm"}),
            json!({"wifi_signal": null}),
        );
        assert_eq!(cleared, json!({"wifi_signal": null}));
    }

    #[test]
    fn nested_objects_merge_without_clobbering_siblings() {
        // A delta that changes only `ams.tray_now` must not destroy the
        // sibling `ams.ams` unit array delivered by the pushall.
        let base = json!({"ams": {"ams": [{"id": "0"}], "tray_now": "1"}});
        let out = merged(base, json!({"ams": {"tray_now": "2"}}));
        assert_eq!(out, json!({"ams": {"ams": [{"id": "0"}], "tray_now": "2"}}));
    }

    #[test]
    fn arrays_replace_wholesale() {
        let out = merged(
            json!({"hms": [{"attr": 1, "code": 2}, {"attr": 3, "code": 4}]}),
            json!({"hms": []}),
        );
        assert_eq!(out, json!({"hms": []}), "an emptied array must empty");
    }

    #[test]
    fn a_type_change_replaces_rather_than_erroring() {
        // Firmware changes types across versions. The merge must not care.
        let out = merged(json!({"x": {"a": 1}}), json!({"x": "gone"}));
        assert_eq!(out, json!({"x": "gone"}));
    }

    #[test]
    fn a_full_snapshot_over_existing_state_wins_everywhere_it_speaks() {
        let base = json!({"layer_num": 900, "gcode_state": "RUNNING"});
        let out = merged(
            base,
            json!({"layer_num": 0, "gcode_state": "IDLE", "mc_percent": 0}),
        );
        assert_eq!(
            out,
            json!({"layer_num": 0, "gcode_state": "IDLE", "mc_percent": 0})
        );
    }
}
