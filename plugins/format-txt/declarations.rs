//! The plain text format's native declaration: one `core.format` provider for
//! `.txt` and `.text` (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// The format `format-txt.txt`. It is `prepared`: a UTF-16 file is converted
/// into the cache on the first open (op 3); it does not sniff, so a file with
/// another extension is never offered to it.
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(FormatDecl::new("txt", "format-txt.title", "txt\ntext", FORMAT_TOKEN).prepared())?;
    d.submit()
}
