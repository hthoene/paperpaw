//! A plugin must not link all of the records codec (flash gate, decision 136): the SDK's
//! facades decode and encode the record types they use (`RawRecord::decode::<T>()`,
//! `WireEncode`), never through the `Record` enum, whose decoder and encoder reach every
//! record type of the wire (161 KB in End of book before this check). The check reads the
//! SDK's plugin side (`crates/paperpaw-sdk/src/native`, tests and the generated codec
//! excepted) for the calls that take that way. `tools/plugin-symbols.sh` makes the same
//! check on a built plugin.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_checks::{root, rust_files};

/// What reaches the whole codec: the enum's generic decoders and encoders.
const FORBIDDEN: &[&str] = &[
    "record_decode_one(",
    "record_decode_prefix(",
    "records_decode(",
    "records_decode_single(",
    "records_encode(",
    "records_encode_counted(",
    "records_encode_counted_iter(",
    "record_encode_one(",
    "record_framed_len(",
    "record_decode(",
    "RawRecord::record(",
    "wire::Record::",
];

/// Files of the plugin side that may name the enum: a `match` on it links no codec
/// (`declare/uix.rs` checks fields of a declaration and encodes through the typed row).
const MAY_MATCH_THE_ENUM: &[&str] = &["crates/paperpaw-sdk/src/native/declare/uix.rs"];

fn plugin_side(path: &str) -> bool {
    path.starts_with("crates/paperpaw-sdk/src/native/")
        && !path.contains("test")
        && !path.ends_with("/records_wire.rs")
        && !path.contains("/wire/")
}

#[test]
fn facades_do_not_reach_the_whole_records_codec() {
    let mut found = Vec::new();
    for (path, file) in rust_files(&root()) {
        if !plugin_side(&path) {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("readable");
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            for call in FORBIDDEN {
                if code.contains(call)
                    && !(*call == "wire::Record::" && MAY_MATCH_THE_ENUM.contains(&path.as_str()))
                {
                    found.push(format!(
                        "{path}:{}: `{call}` links every record's codec; use `RawRecord::decode::<T>()` or a typed `WireEncode` row",
                        number + 1
                    ));
                }
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}
