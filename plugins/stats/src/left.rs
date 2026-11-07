//! Status items "Time left in chapter" and "Time left in book": the pages
//! still to read at one's reading speed, as recorded by the statistics.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::status::{StatusItemSpec, StatusValue, context};
use paperpaw_sdk::{Arg, Env, Id, Params, RegisterError, Registrar};

use crate::{fmt, store};

pub(crate) const CHAPTER: Id = Id::new_static("stats.status.left-chapter");
pub(crate) const BOOK: Id = Id::new_static("stats.status.left-book");
/// Days the reading speed is taken from.
const DAYS: i32 = 30;
/// Pages read before there is a speed to go by.
const MIN_PAGES: u32 = 10;
/// Pages turned after which the speed is worked out again.
const RECOUNT: u32 = 50;

/// Seconds a page, from the reading of the last [`DAYS`] days, if enough
/// was read.
fn speed(env: &Env<'_>) -> Option<f32> {
    let today = fmt::today(env)?;
    let (mut seconds, mut pages) = (0u64, 0u32);
    for day in store::days(env.storage(), |unix| fmt::day_of(env, unix)).unwrap_or_default() {
        if day.day > today - DAYS {
            seconds += u64::from(day.seconds);
            pages += day.stays;
        }
    }
    (pages >= MIN_PAGES).then(|| seconds as f32 / pages as f32)
}

/// Pages left in the book (`chapter` false) or in the chapter, from the
/// reader's context: the pages counted, the progress and where the
/// chapters begin.
fn pages_left(context: &Params, chapter: bool) -> Option<f32> {
    let pages: f32 = context.get(params::PAGES)?.parse().ok()?;
    let progress: f32 = context.get(params::PROGRESS)?.parse().ok()?;
    let at = progress * 10.0;
    let end = if chapter {
        context
            .get(context::CHAPTERS)
            .unwrap_or_default()
            .split(',')
            .filter_map(|mark| mark.trim().parse::<f32>().ok())
            .find(|&mark| mark > at)
            .unwrap_or(1000.0)
    } else {
        1000.0
    };
    Some(((end - at) / 1000.0 * pages).max(0.0))
}

/// The reading speed, worked out when a book is opened and now and then
/// while reading: never as status lines are drawn.
#[derive(Debug, Default)]
struct Speed {
    /// Seconds a page, once worked out (`Some(None)`: too little read).
    known: Option<Option<f32>>,
    /// The book it was worked out for.
    book: String,
    /// Pages turned since.
    turns: u32,
}

impl Speed {
    /// Whether the speed is to be worked out anew as the book at `path`
    /// is shown: not when the same book shows again (the reader says so
    /// after every menu).
    fn opened(&mut self, path: &str) -> bool {
        if self.known.is_some() && self.book == path {
            return false;
        }
        self.book = String::from(path);
        self.turns = 0;
        true
    }

    /// Whether the speed is to be worked out anew after a page was turned:
    /// every [`RECOUNT`] pages, every [`MIN_PAGES`] while there is none.
    fn turned(&mut self) -> bool {
        self.turns = self.turns.wrapping_add(1);
        let every = match self.known {
            Some(Some(_)) => RECOUNT,
            _ => MIN_PAGES,
        };
        self.turns.is_multiple_of(every)
    }
}

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    let shared: Rc<RefCell<Speed>> = Rc::default();
    let s = shared.clone();
    r.on(events::BOOK_OPENED, move |env, reading, _| {
        let mut s = s.borrow_mut();
        if s.opened(&reading.path) {
            s.known = Some(speed(env));
        }
    })?;
    let s = shared.clone();
    r.on(events::PAGE_TURNED, move |env, _, _| {
        let mut s = s.borrow_mut();
        if s.turned() {
            s.known = Some(speed(env));
        }
    })?;
    for (id, title, key, chapter) in [
        (
            CHAPTER,
            "stats.status.left-chapter",
            "stats.left.chapter",
            true,
        ),
        (BOOK, "stats.status.left-book", "stats.left.book", false),
    ] {
        let s = shared.clone();
        r.add_status_item(StatusItemSpec::new(
            id,
            title,
            Box::new(move |env, context| {
                let speed = s.borrow().known.flatten()?;
                let seconds = (pages_left(context, chapter)? * speed) as u64;
                let time = fmt::duration(env, seconds.max(60));
                Some(StatusValue::Text(
                    env.i18n().format(key, &[("time", Arg::Str(&time))]),
                ))
            }),
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_left_to_the_next_chapter() {
        let context = Params::new()
            .with(params::PAGES, "200")
            .with(params::PROGRESS, "25.0")
            .with(context::CHAPTERS, "0,100,300,700");
        assert_eq!(pages_left(&context, false), Some(150.0));
        // From 250 to 300 thousandths: 10 of 200 pages.
        assert_eq!(pages_left(&context, true), Some(10.0));
        // In the last chapter: to the end.
        let last = context.with(params::PROGRESS, "80.0");
        assert_eq!(pages_left(&last, true), Some(40.0));
        // Pages not counted yet: nothing to say.
        assert_eq!(
            pages_left(&Params::new().with(params::PROGRESS, "5"), false),
            None
        );
    }

    #[test]
    fn the_speed_is_worked_out_per_book_not_per_showing() {
        let mut speed = Speed::default();
        assert!(speed.opened("books/a.epub"));
        speed.known = Some(Some(30.0));
        // The same book after a menu: the speed stays.
        assert!(!speed.opened("books/a.epub"));
        assert!(speed.opened("books/b.epub"));
        speed.known = Some(Some(30.0));
        let recounts = (0..RECOUNT * 2).filter(|_| speed.turned()).count();
        assert_eq!(recounts, 2);
        // Without a speed yet, more often, but not at every page.
        speed.known = Some(None);
        let recounts = (0..MIN_PAGES * 2).filter(|_| speed.turned()).count();
        assert_eq!(recounts, 2);
    }
}
