//! The HTML files format's native declaration: one `core.format` provider for
//! .html, .htm and .xhtml (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// The format `format-html.html`: siblings (the pictures beside the page). It is not `prepared`: the
/// shared provider converts a book when it is opened (its `prepare` call has
/// nothing to do yet).
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(
        FormatDecl::new(
            "html",
            "format-html.title",
            "html\nhtm\nxhtml",
            FORMAT_TOKEN,
        )
        .siblings(),
    )?;
    d.submit()
}
