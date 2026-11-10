//! The comic book archives format's native declaration: one `core.format` provider for
//! .cbz (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// The format `format-cbz.cbz`: whole pages (each section is a picture). It is not `prepared`: the
/// shared provider converts a book when it is opened (its `prepare` call has
/// nothing to do yet).
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(FormatDecl::new("cbz", "format-cbz.title", "cbz", FORMAT_TOKEN).whole_pages())?;
    d.submit()
}
