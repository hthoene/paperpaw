//! End of book: a screen when the last page of a book is reached
//! ([`events::BOOK_FINISHED`]), with three rows:
//!
//! - the next book of the series, the first one after this one that is
//!   not read to the end and still on the card (the library when there is
//!   none, or the book has no series);
//! - the book's rating, one to five (`4 / 5`; Select goes on, after five
//!   it is not rated again): a book field of the library
//!   (`end-of-book.rating`), which shows on the book's information page and
//!   as a badge (`4/5`) on its row, and can be filtered and sorted by;
//! - whether the book counts as finished.
//!
//! Two settings: whether the screen opens (`end-of-book.show`, on to begin
//! with) and whether the book is marked finished (`end-of-book.mark`, on
//! to begin with: the third row says *Yes*; off, *No*). The row changes it.
//! What the row says is kept in memory while the book is open and put in
//! the library again when the book closes, because the reader's closing
//! writes how far the book is read as it does for any book.
//!
//! The screen asks the library service (`query`, `set_finished`), writes
//! the rating with [`library::set_field`] and opens the next book with
//! `readers::open`: nothing else of the SDK is needed.

#![no_std]

extern crate alloc;

mod next;
mod screen;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, groups, params};
use paperpaw_sdk::library::service::{BookBadge, BookField};
use paperpaw_sdk::library::{self, Badge};
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::{
    Command, Id, Manifest, Params, Plugin, RegisterError, Registrar, ScreenSpec, SettingSpec,
};

pub const PLUGIN: Id = Id::new_static("end-of-book");
pub(crate) const SCREEN: Id = Id::new_static("end-of-book.screen");
/// The book field the rating is kept in: "1" to "5".
pub(crate) const RATING: &str = "end-of-book.rating";
/// Setting: a screen opens at the end of a book.
pub const SHOW: Setting<bool> = Setting::new("end-of-book.show");
/// Setting: the book is marked finished at its end.
pub const MARK: Setting<bool> = Setting::new("end-of-book.mark");

static SETTINGS: [SettingSpec; 2] = [
    SettingSpec::switch(
        SHOW.static_str(),
        "end-of-book.setting.show",
        true,
        groups::READING_BEHAVIOUR,
    )
    .description("end-of-book.setting.show.help"),
    SettingSpec::switch(
        MARK.static_str(),
        "end-of-book.setting.mark",
        true,
        groups::READING_BEHAVIOUR,
    )
    .description("end-of-book.setting.mark.help"),
];

static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

/// The book whose end was reached, and whether it counts as finished
/// (what the screen's third row says), until the book closes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Session {
    pub path: String,
    pub finished: bool,
}

pub(crate) type Kept = Rc<RefCell<Option<Session>>>;

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
        r.add_settings(&SETTINGS)?;
        let kept: Kept = Rc::default();
        {
            let kept = Rc::clone(&kept);
            r.on(events::BOOK_FINISHED, move |env, reading, out| {
                // A book that is not on the card any more has no end to
                // speak of.
                if !env.storage().exists(&reading.path) {
                    return;
                }
                let finished = env.settings().read(&MARK);
                *kept.borrow_mut() = Some(Session {
                    path: reading.path.clone(),
                    finished,
                });
                out.push(library::service::service(env).set_finished(env, &reading.path, finished));
                if env.settings().read(&SHOW) {
                    out.push(Command::OpenScreen(
                        SCREEN,
                        Params::new().with(params::PATH, reading.path.clone()),
                    ));
                }
            })?;
        }
        {
            // The reader's closing records how far the book is read: said
            // again what the book is.
            let kept = Rc::clone(&kept);
            r.on(events::BOOK_CLOSED, move |env, reading, out| {
                let session = kept.borrow_mut().take();
                if let Some(session) = session.filter(|s| s.path == reading.path)
                    && env.storage().exists(&session.path)
                {
                    out.push(library::service::service(env).set_finished(
                        env,
                        &session.path,
                        session.finished,
                    ));
                }
            })?;
        }
        {
            let kept = Rc::clone(&kept);
            r.add_screen(ScreenSpec::new(
                SCREEN,
                "end-of-book.title",
                Box::new(move |env, params| {
                    Box::new(screen::EndScreen::new(env, params, Rc::clone(&kept)))
                }),
            ))?;
        }
        r.add_book_field(
            BookField::new(Id::new_static(RATING), "end-of-book.rating").in_catalog(),
        )?;
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

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}
