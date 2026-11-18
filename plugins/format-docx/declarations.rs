//! The Word documents format's native declaration: one `core.format` provider
//! for `.docx` (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// The format `format-docx.docx`: the first open converts the document into
/// the cache (`prepared`, op 3). It does not sniff: the extension names it.
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(FormatDecl::new("docx", "format-docx.title", "docx", FORMAT_TOKEN).prepared())?;
    d.submit()
}
