//! The EPUB with picture books and math format's native declaration: one `core.format` provider for
//! .epub, which wins over the built-in EPUB format once installed (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// Above the built-in EPUB format's 0: once installed, this reads the books (the
/// user's choice `core.formats.epub` still wins).
pub const PRIORITY: i32 = 10;

/// The format `format-epub-plus.epub`: sniffs (the `mimetype` entry), a book is the file itself; the
/// built-in EPUB format has priority 0, this one 10. It is not `prepared`: the shared provider
/// converts a book when it is opened. A fixed-layout book of pictures is `whole_pages` per
/// document (the document says it in its open answer).
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(
        FormatDecl::new("epub", "format-epub-plus.title", "epub", FORMAT_TOKEN)
            .sniffs()
            .priority(PRIORITY),
    )?;
    d.submit()
}
