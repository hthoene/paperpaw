//! The Markdown files format's native declaration: one `core.format` provider for
//! .md and .markdown (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// The format `format-markdown.markdown`: siblings (the pictures beside the file). It is not `prepared`: the
/// shared provider converts a book when it is opened (its `prepare` call has
/// nothing to do yet).
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(
        FormatDecl::new(
            "markdown",
            "format-markdown.title",
            "md\nmarkdown",
            FORMAT_TOKEN,
        )
        .siblings(),
    )?;
    d.submit()
}
