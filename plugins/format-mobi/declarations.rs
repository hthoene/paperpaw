//! The Mobipocket format's native declaration: one `core.format` provider for
//! `.mobi`, `.azw` and `.azw3` (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// The format `format-mobi.mobi`. It is `prepared`: the first open converts the
/// book into the cache, a text record a call (op 3). It sniffs (`BOOKMOBI` at
/// byte 60), so a book with another extension is still found.
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(
        FormatDecl::new("mobi", "format-mobi.title", "mobi\nazw\nazw3", FORMAT_TOKEN)
            .prepared()
            .sniffs(),
    )?;
    d.submit()
}
