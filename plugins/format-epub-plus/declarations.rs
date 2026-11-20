//! The EPUB with picture books and math format's native declaration: one `core.format` provider for
//! .epub, which wins over the built-in EPUB format once installed (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl, SubscribeDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;
/// The callback token of the conversion hook `core.convert` (`docs/abi/document.md` 4.1).
pub const CONVERT_TOKEN: u32 = 2;

/// Above the built-in EPUB format's 0: once installed, this reads the books (the
/// user's choice `core.formats.epub` still wins).
pub const PRIORITY: i32 = 10;

/// The format `format-epub-plus.epub`: it reads only the fixed-layout books of pictures. It
/// `asks`: before it opens a book the host asks it whether it reads the book (`#242`), and any
/// other EPUB goes on to the built-in EPUB format, which has priority 0, this one 10. A book is
/// the file itself; the format is not `prepared`. A fixed-layout book of pictures is
/// `whole_pages` per document (the document says it in its open answer).
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(
        FormatDecl::new("epub", "format-epub-plus.title", "epub", FORMAT_TOKEN)
            .asks()
            .priority(PRIORITY),
    )?;
    d.submit()
}

/// The conversion hook: this plugin converts the `<math>` elements the EPUB formats hand on
/// (`core.convert`, #429); it carries no EPUB reader of its own for them.
pub fn stages<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe(SubscribeDecl::new(
        paperpaw_sdk::native::convert::STAGE,
        CONVERT_TOKEN,
    ))?;
    d.submit()
}
