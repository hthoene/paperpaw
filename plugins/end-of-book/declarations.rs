//! The plugin's native declarations: two switches, the screen and the book
//! events. The book field (`end-of-book.rating`) and its badge are not here:
//! the SDK has no builders for them yet (see `PORTING-7.md`).
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EventKind, Instances, Kind, ScreenDecl, SpecDecl,
};

/// The callback token of the book events.
pub const EVENTS_TOKEN: u32 = 1;
/// The screen's token; one instance at a time.
pub const SCREEN_TOKEN: u32 = 10;

/// A screen opens at the end of a book (`end-of-book.show`), and the book
/// is marked finished (`end-of-book.mark`); both on to begin with.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = groups::READING_BEHAVIOUR;
    let mut show = SpecDecl::new(
        "show",
        Kind::Bool(true),
        group.as_str(),
        "end-of-book.setting.show",
    );
    show.help = "end-of-book.setting.show.help";
    d.spec(show)?;
    let mut mark = SpecDecl::new(
        "mark",
        Kind::Bool(true),
        group.as_str(),
        "end-of-book.setting.mark",
    );
    mark.help = "end-of-book.setting.mark.help";
    d.spec(mark)?;
    d.submit()?;
    Ok(())
}

pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(
        ScreenDecl::new("screen", "end-of-book.title", SCREEN_TOKEN).instances(Instances::Single),
    )?;
    d.submit()?;
    Ok(())
}

/// The end of a book, and its closing (which records how far the book is
/// read, so the finished mark is said again).
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::BookFinished, EVENTS_TOKEN)?;
    d.subscribe_event(EventKind::BookClosed, EVENTS_TOKEN)?;
    d.submit()?;
    Ok(())
}
