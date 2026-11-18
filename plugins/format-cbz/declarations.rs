//! The comic book archives format's native declaration: one `core.format` provider for
//! .cbz (`docs/abi/document.md` section 1).
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{Declarations, DeclareError, FormatDecl, Kind, SpecDecl};

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

/// The choices of `format-cbz.direction`: as the book says, left to right,
/// right to left.
pub const DIRECTIONS: [&str; 3] = [
    "format-cbz.direction.book",
    "format-cbz.direction.ltr",
    "format-cbz.direction.rtl",
];

/// The way pages run and whether they are shown panel by panel (off to
/// begin with); each book may have its own.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = groups::READING_BEHAVIOUR;
    let mut direction = SpecDecl::new(
        "direction",
        Kind::Choice {
            default: DIRECTIONS[0],
            choices: &DIRECTIONS,
        },
        group.as_str(),
        "format-cbz.setting.direction",
    );
    direction.help = "format-cbz.setting.direction.help";
    direction.scoped = true;
    d.spec(direction)?;
    let mut panels = SpecDecl::new(
        "panels",
        Kind::Bool(false),
        group.as_str(),
        "format-cbz.setting.panels",
    );
    panels.help = "format-cbz.setting.panels.help";
    panels.scoped = true;
    d.spec(panels)?;
    d.submit()
}
