//! Golden tests over the recorded SSDP datagram corpus.
//!
//! Every file in `fixtures/ssdp/` is a complete datagram, byte for byte —
//! CRLF where a printer sends CRLF, LF where the corpus is pinning our
//! tolerance of it. Each one is parsed and snapshotted together with the
//! [`DiscoveredPrinter`] it resolves to, so a change in what the dialect
//! means shows up as a diff rather than as a printer that quietly stops
//! appearing in the list.
//!
//! Fixtures are read as bytes and never as strings: line endings are the
//! thing under test, and `.gitattributes` marks `fixtures/**` as `-text` so
//! git cannot normalise them out from under us.
//!
//! The corpus is the contract. Never edit a fixture to make a test pass — if
//! the parser disagrees with a recording, the parser is wrong. Review changes
//! with `cargo insta review` and actually read the diff.
//!
//! [`DiscoveredPrinter`]: pandaspy_discovery::DiscoveredPrinter

use std::net::SocketAddr;
use std::path::Path;

use pandaspy_discovery::ssdp::SsdpAnnouncement;

/// The datagram's sender, fixed so snapshots stay deterministic. Fixtures
/// that omit `Location` fall back to this address, which is the whole point
/// of `synthetic-notify-no-location-h2d.txt`.
const SENDER: &str = "192.168.0.2:2021";

#[test]
fn every_ssdp_fixture_parses_and_matches_its_snapshot() {
    // The three-argument form: insta refuses `..` inside the *pattern*, so the
    // walk up to the repo root happens in the base path instead.
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ssdp");
    let sender: SocketAddr = SENDER.parse().expect("the fixed sender address is valid");

    insta::glob!(&corpus, "*.txt", |path| {
        let raw = std::fs::read(path)
            .unwrap_or_else(|err| panic!("cannot read fixture {}: {err}", path.display()));

        match SsdpAnnouncement::parse(&raw) {
            Some(announcement) => {
                let printer = announcement.to_printer(sender);
                insta::assert_json_snapshot!(serde_json::json!({
                    "announcement": announcement,
                    "printer": printer,
                }));
            }
            // A rejection is part of the contract too: the socket sees plenty
            // of traffic that is not ours, and the corpus records that we say
            // so rather than inventing a printer from it.
            None => insta::assert_snapshot!("not ssdp"),
        }
    });
}
