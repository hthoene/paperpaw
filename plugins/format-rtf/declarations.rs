//! The RTF format's native declaration: one `core.format` provider for
//! `.rtf` (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// The format `format-rtf.rtf`. It is `prepared`: the first open converts the
/// document into the cache, a few kilobytes a call (op 3). It sniffs (`{\rtf`
/// at byte 0), so a document with another extension is still found.
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(
        FormatDecl::new("rtf", "format-rtf.title", "rtf", FORMAT_TOKEN)
            .prepared()
            .sniffs(),
    )?;
    d.submit()
}
