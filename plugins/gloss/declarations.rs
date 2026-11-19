//! What Gloss declares: the size of the small text, and its subscription to
//! the reader's text stage. `build.rs` turns it into the image's batches.
use paperpaw_sdk::native::{
    declare::{Declarations, DeclareError, Kind, SpecDecl, SubscribeDecl},
    page_layers::TEXT_STAGE,
};

/// The size choices, smallest first (translation keys).
pub const SIZES: [&str; 3] = ["gloss.size.small", "gloss.size.medium", "gloss.size.large"];

/// The token of the subscription to the reader's text stage.
pub const TEXT_TOKEN: u32 = 1;

/// The size of the small text.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut size = SpecDecl::new(
        "size",
        Kind::Choice {
            default: "gloss.size.medium",
            choices: &SIZES,
        },
        "core.group.reading-aids",
        "gloss.setting.size",
    );
    size.help = "gloss.setting.size.help";
    d.spec(size)?;
    d.submit()
}

/// The stage the reader asks for each paragraph of a book.
pub fn subscriptions<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe(SubscribeDecl::new(TEXT_STAGE, TEXT_TOKEN))?;
    d.submit()
}
