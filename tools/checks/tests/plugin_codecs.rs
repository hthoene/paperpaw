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

/// Files that import the enum bare today: `declare/format.rs` builds a `FormatSpec` and
/// hands it to `accept_record`, which encodes through the enum (a plugin that declares a
/// format links that encoder; moving it to a typed row is open).
const MAY_IMPORT_THE_ENUM: &[&str] = &["crates/paperpaw-sdk/src/native/declare/format.rs"];

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

/// A bare `use ...::Record;` (alone or in a list) lets a later `Record::Name(..)` or
/// `Record<'_>` reach the whole codec without the `wire::Record::` the check above looks
/// for. Name the enum through its module (`wire::Record`) where a file must, which this
/// check and a reader can see.
#[test]
fn the_record_enum_is_not_imported_bare() {
    let mut found = Vec::new();
    for (path, file) in rust_files(&root()) {
        if !plugin_side(&path) {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("readable");
        let mut statement = String::new();
        let mut first_line = 0;
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            if statement.is_empty() {
                let trimmed = code.trim_start();
                let trimmed = trimmed
                    .strip_prefix("pub(crate) ")
                    .or_else(|| trimmed.strip_prefix("pub(super) "))
                    .or_else(|| trimmed.strip_prefix("pub "))
                    .unwrap_or(trimmed);
                if !trimmed.starts_with("use ") {
                    continue;
                }
                first_line = number + 1;
            }
            statement.push_str(code);
            statement.push(' ');
            if code.contains(';') {
                let names_the_enum = statement
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .any(|word| word == "Record");
                if names_the_enum && !MAY_IMPORT_THE_ENUM.contains(&path.as_str()) {
                    found.push(format!(
                        "{path}:{first_line}: a bare `Record` import; write `wire::Record` where the enum is matched"
                    ));
                }
                statement.clear();
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}
