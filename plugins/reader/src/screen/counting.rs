//! The count of a book's pages while overlays cover the page (#117): the
//! menu bar, a panel. The page still shows below them, and the reader
//! goes on counting there (`Screen::times_under_overlays`), then tells
//! the menu bar the page text it now has ([`events::MENU_PAGE`]).

use alloc::format;
use alloc::string::String;

use paperpaw_sdk::builtin::events;
use paperpaw_sdk::event::ParamValue;
use paperpaw_sdk::{Cx, Env};

use super::book::Book;
use super::{ReaderScreen, STEP_DELAY_MS, STEP_DELAY_SMALL_MS};

impl Book {
    /// Whether its pages are still being counted.
    pub(super) fn counting(&self) -> bool {
        !self.index_stopped
            && self
                .index
                .as_ref()
                .is_some_and(|index| !index.is_complete())
    }

    /// Where one is in it, as the menu bar shows it: the page and the
    /// pages once counted, else the percentage.
    pub(super) fn menu_page(&self, env: &Env<'_>) -> String {
        let reading = self.reading(env);
        match (reading.page, reading.pages) {
            (Some(page), Some(pages)) => format!("{page} / {pages}"),
            _ => format!("{} %", reading.progress.to_param()),
        }
    }
}

impl ReaderScreen {
    /// A step of the count below overlays; where it ends, the menu bar
    /// (if one is open) and the status line show the pages. Turning pages
    /// by itself and the rest of its time wait until it is on top again.
    pub(super) fn count_under_overlays(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Ok(book) = &mut self.book else {
            return;
        };
        if book.index_step(&env) {
            let delay = match env.device().memory {
                paperpaw_sdk::MemoryClass::Small => STEP_DELAY_SMALL_MS,
                _ => STEP_DELAY_MS,
            };
            cx.wake_at(env.now().add_millis(delay));
            return;
        }
        let page = book.menu_page(&env);
        cx.command(events::MENU_PAGE.emit(events::MenuPage::new(
            String::from(crate::MENU.as_str()),
            page,
        )));
        self.update_status(cx);
    }
}
