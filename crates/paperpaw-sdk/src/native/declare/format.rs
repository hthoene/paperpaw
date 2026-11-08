//! A file format's declaration (`format.spec`, `docs/abi/document.md` 1).
use super::menus::menu_identifier;
use super::records_wire as wire;
use super::{Declarations, DeclareError, filled};
use paperpaw_native::host_glue::ERR_INVALID;

/// `prepared`: the first open converts into the cache and takes a while.
const PREPARED: u32 = 1;
/// `sniffs`: the format can tell its files by their first 512 bytes.
const SNIFFS: u32 = 1 << 1;
/// `folder`: a book is a folder.
const FOLDER: u32 = 1 << 2;
/// `whole_pages`: each section is a whole-page picture.
const WHOLE_PAGES: u32 = 1 << 3;
/// `siblings`: root 3 is the book's folder and below, read only.
const SIBLINGS: u32 = 1 << 4;

/// A file format a plugin reads through the stage `core.format`: the name the
/// format id `<plugin>.<name>` is made from, the title the user sees, the file
/// extensions it claims, how the host treats it, its priority among formats
/// claiming one extension and the callback token the host calls it with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FormatDecl<'a> {
    /// The relative name (`epub`): the id is `<plugin>.<name>`.
    pub name: &'a str,
    /// The title, a catalog key of this plugin.
    pub title: &'a str,
    /// Lowercase extensions without the dot, one per line (`fb2.zip` is one).
    pub extensions: &'a str,
    /// Higher wins when two formats claim one extension and the user chose none.
    pub priority: i32,
    /// The callback token the host calls the format with.
    pub token: u32,
    flags: u32,
}

impl<'a> FormatDecl<'a> {
    /// A format for these extensions, with priority 0 and no flag.
    #[must_use]
    pub const fn new(name: &'a str, title: &'a str, extensions: &'a str, token: u32) -> Self {
        Self {
            name,
            title,
            extensions,
            priority: 0,
            token,
            flags: 0,
        }
    }

    /// The priority among formats claiming one extension.
    #[must_use]
    pub const fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// The first open converts into the cache and takes a while.
    #[must_use]
    pub const fn prepared(mut self) -> Self {
        self.flags |= PREPARED;
        self
    }

    /// It can tell its files by their first 512 bytes, so a file with an
    /// unclaimed extension is offered to it.
    #[must_use]
    pub const fn sniffs(mut self) -> Self {
        self.flags |= SNIFFS;
        self
    }

    /// A book is a folder.
    #[must_use]
    pub const fn folder(mut self) -> Self {
        self.flags |= FOLDER;
        self
    }

    /// Each section is a whole-page picture (comics).
    #[must_use]
    pub const fn whole_pages(mut self) -> Self {
        self.flags |= WHOLE_PAGES;
        self
    }

    /// Root 3 is the book's folder and below, read only, not the file alone.
    #[must_use]
    pub const fn siblings(mut self) -> Self {
        self.flags |= SIBLINGS;
        self
    }
}

/// One extension as the host checks it: lowercase ASCII letters and digits with
/// inner dots (`fb2.zip`), no leading or trailing dot, no `..`, 1 to 31 bytes.
fn valid_extension(extension: &str) -> bool {
    (1..=wire::FORMAT_SPEC_EXTENSIONS_ITEM_MAX_BYTES).contains(&extension.len())
        && !extension.starts_with('.')
        && !extension.ends_with('.')
        && !extension.contains("..")
        && extension
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.')
}

/// The extensions as the contract spells them: one to 16 lines, each a valid
/// extension, none twice.
fn valid_extensions(list: &str) -> bool {
    let mut seen = 0;
    for extension in list.split(wire::FORMAT_SPEC_EXTENSIONS_SEPARATOR) {
        seen += 1;
        if !valid_extension(extension)
            || seen > wire::FORMAT_SPEC_EXTENSIONS_MAX_ITEMS
            || list
                .split(wire::FORMAT_SPEC_EXTENSIONS_SEPARATOR)
                .take(seen - 1)
                .any(|earlier| earlier == extension)
        {
            return false;
        }
    }
    true
}

impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    /// Declares a file format. Declaring it subscribes the plugin to
    /// `core.format` for this format.
    ///
    /// # Errors
    /// Refuses a name that is not a relative id, an empty or overlong title, and
    /// extensions that are empty, repeated, too many (16), too long (31 bytes) or
    /// not made of lowercase ASCII letters, digits and inner dots (no leading or
    /// trailing dot, no `..`).
    ///
    /// Only the host sees the whole plugin, so these surface when the batch is
    /// submitted: an extension or a name the plugin declared twice, more than 8
    /// formats for the plugin or 32 on the device (`ERR_FULL`), and a joined id
    /// `<plugin>.<name>` of more than 128 bytes.
    pub fn format(&mut self, format: FormatDecl<'_>) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(format.name, wire::FORMAT_SPEC_NAME_MAX_BYTES, "format name")?;
            filled(format.title, "format title")?;
            if format.title.len() > wire::FORMAT_SPEC_TITLE_MAX_BYTES
                || format.extensions.len() > wire::FORMAT_SPEC_EXTENSIONS_MAX_BYTES
                || !valid_extensions(format.extensions)
            {
                return Err(DeclareError::Encode(ERR_INVALID));
            }
            Ok(wire::FormatSpec {
                name: format.name,
                title: format.title,
                extensions: format.extensions,
                flags: format.flags,
                priority: format.priority,
                token: format.token,
            })
        })();
        self.accept_record(record)
    }
}
