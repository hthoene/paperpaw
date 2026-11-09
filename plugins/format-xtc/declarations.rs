//! The XTC format's native declaration: one `core.format` provider for `.xtc`
//! and `.xtch` (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// The format `format-xtc.xtc`: every section is a whole-page picture, and it
/// sniffs (`XTC\0`, `XTCH`), so a book with another extension is still found.
/// It is not `prepared`: pages are decoded when first shown, not on open.
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(
        FormatDecl::new("xtc", "format-xtc.title", "xtc\nxtch", FORMAT_TOKEN)
            .whole_pages()
            .sniffs(),
    )?;
    d.submit()
}
