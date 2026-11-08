//! Every book with reading time, the last one read first.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

use crate::store::{self, Book};
use crate::{BOOK, fmt, totals};

pub(crate) struct BookList {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// Books and their reading time.
    books: Vec<(Book, u64)>,
}

impl BookList {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height - top);
        let books = store::books(env.storage());
        // Interface 6, replaced by the shell: a refused allocation lists no books.
        let seconds = totals::seconds_per_book(env.storage(), &books).unwrap_or_default();
        let mut books: Vec<(Book, u64)> = books
            .into_iter()
            .zip(seconds)
            .filter(|(_, s)| *s > 0)
            .collect();
        paperpaw_sdk::sort::sort_by_key(&mut books, |(b, _)| core::cmp::Reverse(b.last_open));
        Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            books,
        }
    }

    fn open(&self, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some((book, _)) = self.books.get(index) {
            let params = Params::new().with(params::ID, format!("{}", book.id));
            cx.command(Command::OpenScreen(BOOK, params));
        }
    }
}

impl Screen for BookList {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.books.len();
        let title = env.i18n().tr("stats.books");
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        if len == 0 {
            ui.message(self.list.area(), env.i18n().tr("stats.no-books"));
            return;
        }
        let books = &self.books;
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            let (book, seconds) = &books[index];
            let mut note = authors(&book.authors);
            if book.last_open > 0 {
                let day = fmt::day_of(env, book.last_open as u32);
                let last = env.i18n().format(
                    "stats.last-read",
                    &[("date", Arg::Str(&fmt::date(env, day)))],
                );
                if !note.is_empty() {
                    note.push_str(" · ");
                }
                note.push_str(&last);
            }
            let value = fmt::duration(env, *seconds);
            ui::Row::value(&book.title, &value)
                .note(&note)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.books.len();
        let page = self.list.rows() as isize;
        let a = action;
        let delta = if let Some(delta) = ui::list_step(a, page) {
            delta
        } else if *a == actions::CONFIRM {
            self.open(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        let change = self.list.move_focus_in(cx.env(), delta, len);
        change.request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(self.books.len());
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, self.books.len()) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        self.open(index, cx);
        Handled::Yes
    }
}

/// Authors as shown: on one line, nothing for KOReader's `N/A`.
pub(crate) fn authors(authors: &str) -> String {
    if authors == "N/A" {
        return String::new();
    }
    authors.split('\n').collect::<Vec<_>>().join(", ")
}
