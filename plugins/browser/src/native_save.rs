//! Where a download goes once it is whole: books into the library
//! (`library.import`, the host catalogues them), fonts and other files from
//! the data folder to the card (`core.files` move). Each takes a free name;
//! nothing a user keeps is replaced.
use alloc::string::String;

use paperpaw_sdk::native::{
    Error, Host,
    files::Files,
    library::{Existing, Import, Library},
    storage::Root,
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer},
    text::concat,
};

/// The card's fonts folder, where the host reads fonts from.
pub const FONTS: &str = "fonts";
/// The card's folder for other files.
pub const DOWNLOADS: &str = "Downloads";
/// How many names are tried before a download is refused.
const TRIES: u32 = 99;

/// The `n`th name for `name`: `name` itself for 1, then `stem (2).ext`,
/// `stem (3).ext` and so on (a leading dot is no extension).
///
/// # Errors
/// [`Error::NoMemory`] where the name cannot be made.
pub fn numbered(name: &str, n: u32) -> Result<String, Error> {
    if n <= 1 {
        return paperpaw_ui_kit::text::copy(name);
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    let mut digits = [0u8; 10];
    let mut at = digits.len();
    let mut rest = n;
    while rest > 0 {
        at -= 1;
        digits[at] = b'0' + u8::try_from(rest % 10).unwrap_or(0);
        rest /= 10;
    }
    let number =
        core::str::from_utf8(digits.get(at..).unwrap_or(&[])).map_err(|_| Error::Invalid)?;
    concat(&[stem, " (", number, ")", extension])
}

/// Imports the download `from` (root 0) into the library as `name`, or the
/// next free name; the book's path in the library.
///
/// # Errors
/// The host's refusal: `Invalid` for a name the card does not take, `Full`,
/// `Busy`, `Denied` and the like.
pub fn book(host: &Host<'_>, library: &Library, from: &str, name: &str) -> Result<String, Error> {
    let (mut input, mut out) = (buffer(2_048)?, buffer(SERVICE_BUFFER)?);
    let file = Import::new(from, name).existing(Existing::FreeName);
    let path = library.import(host, &file, &mut input, &mut out)?;
    paperpaw_ui_kit::text::copy(path)
}

/// Moves the download `from` (root 0) into the card's `folder` under `name`
/// or the next free name; the name taken.
///
/// # Errors
/// `State` when ninety-nine names are taken; the host's refusal otherwise
/// (`Invalid` for a name the card does not take, `Full`, `Busy`, `Denied`).
pub fn card_file(
    host: &Host<'_>,
    files: &Files,
    from: &str,
    folder: &str,
    name: &str,
) -> Result<String, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    files.make_dir(host, Root::Files, folder, &mut input, &mut out)?;
    for n in 1..=TRIES {
        let candidate = numbered(name, n)?;
        let target = concat(&[folder, "/", &candidate])?;
        match files.move_entry(
            host,
            Root::Data,
            from,
            Root::Files,
            &target,
            false,
            &mut input,
            &mut out,
        ) {
            Ok(()) => return Ok(candidate),
            // Taken: the next one.
            Err(Error::State) => {}
            Err(error) => return Err(error),
        }
    }
    Err(Error::State)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use crate::fail_alloc::sweep;

    #[test]
    fn names_are_numbered_before_the_extension() {
        assert_eq!(numbered("a.epub", 1).unwrap(), "a.epub");
        assert_eq!(numbered("a.epub", 2).unwrap(), "a (2).epub");
        assert_eq!(numbered("a.b.ttf", 10).unwrap(), "a.b (10).ttf");
        assert_eq!(numbered("notes", 3).unwrap(), "notes (3)");
        assert_eq!(numbered(".hidden", 2).unwrap(), ".hidden (2)");
        assert_eq!(numbered("é.txt", 99).unwrap(), "é (99).txt");
    }

    #[test]
    fn numbering_refuses_every_allocation_in_turn() {
        sweep(|| numbered("a.epub", 2).map(|name| name.len()));
        sweep(|| numbered("a.epub", 1).map(|name| name.len()));
    }
}
