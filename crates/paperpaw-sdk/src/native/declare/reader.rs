//! A reading app's declaration (`reader.spec`, `docs/abi/document.md` 1).
use super::menus::{menu_full_id, menu_identifier};
use super::records_wire as wire;
use super::{Declarations, DeclareError};
use paperpaw_native::host_glue::ERR_INVALID;

/// A reading app (interface 6 `ReaderSpec`): the name its id
/// `<plugin>.<name>` is made from, the plugin's own screen that shows a book,
/// and the formats it reads (format ids, one per line; empty: every format).
/// The host counts a book's reading session only for a reader it knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReaderDecl<'a> {
    /// The relative name (`reader`): the id is `<plugin>.<name>`.
    pub name: &'a str,
    /// The full id of one of the plugin's own screens (`reader.screen`).
    pub screen: &'a str,
    /// Format ids, one per line, at most 8; empty for every format.
    pub formats: &'a str,
}

impl<'a> ReaderDecl<'a> {
    /// A reader of every format, shown by `screen`.
    #[must_use]
    pub const fn new(name: &'a str, screen: &'a str) -> Self {
        Self {
            name,
            screen,
            formats: "",
        }
    }

    /// Only these formats (format ids, one per line, at most 8).
    #[must_use]
    pub const fn formats(mut self, formats: &'a str) -> Self {
        self.formats = formats;
        self
    }
}

/// The formats as the contract spells them: none, or one to 8 lines, each a
/// full id.
fn valid_formats(list: &str) -> bool {
    list.is_empty()
        || (list.split(wire::READER_SPEC_FORMATS_SEPARATOR).count()
            <= wire::READER_SPEC_FORMATS_MAX_ITEMS
            && list
                .split(wire::READER_SPEC_FORMATS_SEPARATOR)
                .all(|format| {
                    menu_full_id(
                        format,
                        wire::READER_SPEC_FORMATS_ITEM_MAX_BYTES,
                        "reader format",
                    )
                    .is_ok()
                }))
}

impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    /// Declares a reading app: the host then knows the plugin's `screen` as a
    /// reader, counts the reading sessions of the books it opens and lists it
    /// to openers (`core.document` op 21).
    ///
    /// # Errors
    /// Refuses a name that is not a relative id, a screen that is not a full
    /// id, and formats that are not full ids or more than 8.
    ///
    /// Only the host sees the whole plugin, so these surface when the batch is
    /// submitted: a screen that is not the plugin's own, more than 4 readers
    /// for the plugin or 16 on the device (`ERR_FULL`).
    pub fn reader(&mut self, reader: ReaderDecl<'_>) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(reader.name, wire::READER_SPEC_NAME_MAX_BYTES, "reader name")?;
            menu_full_id(
                reader.screen,
                wire::READER_SPEC_SCREEN_MAX_BYTES,
                "reader screen",
            )?;
            if reader.formats.len() > wire::READER_SPEC_FORMATS_MAX_BYTES
                || !valid_formats(reader.formats)
            {
                return Err(DeclareError::Encode(ERR_INVALID));
            }
            Ok(wire::ReaderSpec {
                name: reader.name,
                screen: reader.screen,
                formats: reader.formats,
                flags: 0,
            })
        })();
        self.accept_record(record)
    }
}
