//! The book read last, as `state.ini` in the plugin's data folder keeps it
//! (`[reader]` / `last = <path>`): the same file and the same reading as
//! interface 6 (`lib.rs`: `last_in`; `screen/book.rs`: `remember_last`), with
//! every allocation fallible. The path is relative to the data folder
//! (`DataStorage`, root 0).
use alloc::string::String;

use paperpaw_sdk::config;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text;

/// The file, in the data folder.
pub const STATE: &str = "state.ini";
/// The most of it that is read; a book path is far shorter.
pub const MAX_STATE: usize = 4096;

/// The book kept in the text of `state.ini`: the last `last` of the section
/// `reader`; lines that do not parse are left out.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn last_in(state: &str) -> Result<Option<String>, Error> {
    let Some(entry) = config::parse(state)
        .filter_map(Result::ok)
        .filter(|e| e.section == "reader" && e.key == "last")
        .last()
    else {
        return Ok(None);
    };
    let value = entry.value.try_as_str().map_err(|_| Error::NoMemory)?;
    text::copy(&value).map(Some)
}

/// `value` as a file keeps it: as it is where it survives unquoted, else
/// quoted with `"`, `\`, line breaks and tabs escaped (as
/// `paperpaw_sdk::config::quote`).
fn quoted(value: &str) -> Result<String, Error> {
    let plain = !value.is_empty()
        && value.trim() == value
        && !value.starts_with('"')
        && !value.contains(['\n', '\r']);
    if plain {
        return text::copy(value);
    }
    let escapes = value
        .chars()
        .filter(|c| matches!(c, '"' | '\\' | '\n' | '\t'))
        .count();
    let mut out = String::new();
    out.try_reserve_exact(value.len() + escapes + 2)
        .map_err(|_| Error::NoMemory)?;
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    Ok(out)
}

/// The text of `state.ini` that says `path` was read last.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn state_text(path: &str) -> Result<String, Error> {
    text::concat(&["[reader]\nlast = ", &quoted(path)?, "\n"])
}
