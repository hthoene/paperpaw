//! The pictures format's native declaration: one `core.format` provider for
//! `.jpg`, `.jpeg`, `.png` and `.gif` (`docs/abi/document.md` section 1).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl};

/// The callback token the host calls the format with.
pub const FORMAT_TOKEN: u32 = 1;

/// The format `format-images.pictures`: every section is a whole-page picture,
/// and root 3 is the folder of the opened picture (`siblings`), whose pictures
/// follow it as pages. It is not `prepared` (pictures are decoded when shown)
/// and does not sniff.
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(
        FormatDecl::new(
            "pictures",
            "format-images.title",
            "jpg\njpeg\npng\ngif",
            FORMAT_TOKEN,
        )
        .whole_pages()
        .siblings(),
    )?;
    d.submit()
}
