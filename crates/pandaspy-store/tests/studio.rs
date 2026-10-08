//! Golden test for the Bambu Studio import against `fixtures/studio/`.

use std::path::PathBuf;

use pandaspy_store::read_studio_config;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/studio")
        .join(name)
}

#[test]
fn every_printer_with_a_code_is_imported_even_without_an_address() {
    let printers = read_studio_config(&fixture("macos-three-printers-one-lan-ip.conf")).unwrap();

    let summary: Vec<_> = printers
        .iter()
        .map(|p| {
            (
                p.serial.0.as_str(),
                p.access_code.as_deref(),
                p.address.map(|a| a.to_string()),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("03900D000000001", Some("00000000"), None),
            ("20P5AJ000000002", Some("00000000"), None),
            (
                "20P9AJ000000003",
                Some("00000000"),
                Some("192.168.0.2".to_owned())
            ),
        ]
    );
}
