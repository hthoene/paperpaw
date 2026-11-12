//! The first-run help of the reading screen waits for the book (Beta 3's
//! `help::wait` and `help::ready`): the reader says the book shows as Beta 3's
//! `on_show` did, each time the reading screen comes to the top with its book
//! open and when a book that was prepared opens (`command.screen_ready`), so
//! the hints (the tap zones, or the buttons) never come over the words
//! *Preparing* or an error. The host raises `event.screen_ready` once an
//! instance and takes a second word as nothing; one it refuses (the screen is
//! not the plugin's own top one) is said again at the next show.
//!
//! The word counts only for a screen declared `waits_ready`, which the host
//! does not admit yet (`screen.spec` bit 3 is `ERR_UNSUPPORTED`): until it
//! does, the reading screen is declared without it, the host raises the event
//! after the screen's first draw itself, and the word here is taken as
//! nothing (`PORTING-7.md`).
use crate::native_view::View;
use paperpaw_sdk::native::{Host, commands::CommandSender};

/// Whether `view` shows a book: open, not prepared nor a message.
#[must_use]
pub fn shows_book(view: &View) -> bool {
    shows(view.book().is_some(), view.preparing())
}

/// Whether a screen with a book (`has_book`) that is not `preparing` shows it.
#[must_use]
pub const fn shows(has_book: bool, preparing: bool) -> bool {
    has_book && !preparing
}

/// Tells the host that the reading screen `view` shows its book, if it does: its
/// hints may show, the first time. A refusal is left for the next show.
pub fn say(host: &Host<'_>, view: &View) {
    if shows_book(view) {
        let mut scratch = [0; 32];
        CommandSender::new(host, &mut scratch).screen_ready().ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_book_on_show_lets_the_hints_come() {
        assert!(shows(true, false));
        // The book is prepared: the screen says "Preparing".
        assert!(!shows(true, true));
        // No book: the screen says why.
        assert!(!shows(false, false));
    }
}
