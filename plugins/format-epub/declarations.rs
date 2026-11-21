//! The EPUB 2/3 books format's native declaration: one `core.format` provider for
//! .epub (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// The format `format-epub.epub`: sniffs (the `mimetype` entry) and is
/// `siblings`: root 3 is the book's folder, so one's own stylesheets beside the
/// book (`styles.css`, `<book>.css`) are applied after its own, as in Beta 3.
/// It is not `prepared`: the shared provider converts a book when it is opened (its
/// `prepare` call has nothing to do yet).
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(
        FormatDecl::new("epub", "format-epub.title", "epub", FORMAT_TOKEN)
            .sniffs()
            .siblings(),
    )?;
    d.submit()
}
