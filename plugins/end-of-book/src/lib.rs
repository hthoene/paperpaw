//! End of book (prototype): a screen when the last page of a book is
//! reached ([`events::BOOK_FINISHED`]), with three rows: the next book of
//! the series (or the library), the book's rating (one to five stars, a
//! book field of the library: `end-of-book.rating`, shown as a badge on
//! the book's row) and whether the book is marked finished. The book is
//! marked finished when the screen opens; the last row undoes it.
//!
//! Everything it needs the SDK offers: the event, the library service
//! (`query`, `set_finished`), book fields and badges, and
//! [`readers::open`].

#![no_std]

extern crate alloc;

mod next;
mod screen;

use alloc::borrow::Cow;
use alloc::boxed::Box;

use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::library::service::{BookBadge, BookField};
use paperpaw_sdk::library::{self, Badge};
use paperpaw_sdk::{
    Command, Id, Manifest, Params, Plugin, RegisterError, Registrar, ScreenSpec,
};

pub const PLUGIN: Id = Id::new_static("end-of-book");
pub(crate) const SCREEN: Id = Id::new_static("end-of-book.screen");
/// The book field the rating is kept in: "1" to "5".
pub(crate) const RATING: &str = "end-of-book.rating";

static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

#[derive(Debug)]
pub struct EndOfBook;

impl Plugin for EndOfBook {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "End of book", paperpaw_sdk::package_version!())
            .description(
                "A screen at the end of a book: the next book of the series, a rating, finished or not.",
            )
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.on(events::BOOK_FINISHED, |env, reading, out| {
            out.push(library::service::service(env).set_finished(env, &reading.path, true));
            out.push(Command::OpenScreen(
                SCREEN,
                Params::new().with(params::PATH, reading.path.clone()),
            ));
        })?;
        r.add_screen(ScreenSpec::new(
            SCREEN,
            "end-of-book.title",
            Box::new(|env, params| Box::new(screen::EndScreen::new(env, params))),
        ))?;
        r.add_book_field(BookField::new(Id::new_static(RATING), "end-of-book.rating").in_catalog())?;
        r.add_book_badge(BookBadge::new(
            Id::new_static("end-of-book.badge"),
            "end-of-book.rating",
            |_, record| {
                let stars = record.field(RATING)?;
                Some(Badge::Text(alloc::format!("{stars}/5")))
            },
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}
