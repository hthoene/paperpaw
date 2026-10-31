//! Finding a book without a keyboard: the letters of the alphabet, each
//! listing the books whose title (without "The") or author's surname
//! starts with it. Letters no book starts with are faint and skipped.
//! Above them, searching with the keyboard (on every device; with
//! buttons alone it is the slower way).
//!
//! Which letters have books is found in one pass over the catalog when it
//! opens.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
};

use paperpaw_sdk::library;

use super::index::{letter, sort_key, surname};
use super::shelf::{self, Kind};

/// The letters, `#` for titles starting otherwise.
const LETTERS: &[u8; 27] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ#";

pub(crate) struct FindScreen {
    style: UiStyle,
    header: ui::Header,
    /// The row that searches with the keyboard.
    search: Rect,
    /// The letters some book starts with, and their cells.
    letters: Vec<u8>,
    cells: Vec<Rect>,
    columns: usize,
    /// The note below the letters.
    note: Rect,
    /// 0 is the search, then the letters.
    focus: usize,
    shown: bool,
}

impl FindScreen {
    pub(crate) fn new(env: &Env<'_>, _: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let margin = style.margin();
        let gap = style.gap();
        let row = style.touch_row(u32::from(style.row())) as u16;
        let search = Rect::new(0, header.area().height + gap, display.width, row);
        // Which letters have books: by title (without "The") or surname.
        let mut has = [false; 27];
        library::service(env).each(env, &mut |_, line| {
            let by_title = match sort_key(&line.title)[0] {
                b @ b'a'..=b'z' => b.to_ascii_uppercase(),
                _ => b'#',
            };
            let by_author = letter(surname(&line.author));
            for (n, l) in LETTERS.iter().enumerate() {
                has[n] |= *l == by_title || (*l == by_author && by_author != b'#');
            }
        });
        let letters: Vec<u8> = LETTERS
            .iter()
            .zip(has)
            .filter_map(|(l, has)| has.then_some(*l))
            .collect();
        // Cells about 11 mm wide, whole columns within the margins.
        let width = display.width.saturating_sub(2 * margin);
        let columns = (u32::from(width) / env.metrics().mm(11).max(1)).clamp(5, 8) as u16;
        let cell = width / columns;
        let top = search.y + search.height + gap;
        let cells: Vec<Rect> = (0..letters.len() as u16)
            .map(|n| {
                Rect::new(
                    margin + (n % columns) * cell,
                    top + (n / columns) * cell,
                    cell,
                    cell,
                )
            })
            .collect();
        let bottom = cells.last().map_or(top, |c| c.y + c.height);
        // As high as its words need (two rows at least), up to the bottom.
        let words = ui::tr_input(env, "files.list.find.note");
        let high = (row * 2).max(ui::message_height(env, &style, display.width, words));
        let top = bottom + gap;
        let note = Rect::new(
            0,
            top,
            display.width,
            high.min(display.height.saturating_sub(top)),
        );
        Self {
            focus: usize::from(!letters.is_empty()),
            shown: ui::focus_shown(env),
            columns: usize::from(columns),
            style,
            header,
            search,
            letters,
            cells,
            note,
        }
    }

    /// Moves the focus `delta` places (a row of letters for the page
    /// buttons); the search is before the first letter.
    fn step(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let before = self.focus;
        if !self.shown {
            self.shown = true;
        } else {
            let len = self.letters.len() as isize + 1;
            self.focus = (self.focus as isize + delta).rem_euclid(len) as usize;
        }
        // The search or the letter the focus left, and where it came to.
        ui::ListChange::focus(self.place(before), self.place(self.focus)).request(cx);
    }

    /// Where place `index` is: the search, then the letters.
    fn place(&self, index: usize) -> Rect {
        match index.checked_sub(1) {
            Some(letter) => self.cells.get(letter).copied().unwrap_or(self.search),
            None => self.search,
        }
    }

    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        if index == 0 {
            cx.command(Command::run(crate::SEARCH));
        } else if let Some(letter) = self.letters.get(index - 1) {
            let letter = String::from(*letter as char);
            cx.command(shelf::open(Kind::Letter, &letter));
        }
    }
}

impl Screen for FindScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n().tr("files.list.find.title");
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(&mut ui, title, None, None);
        let Some(font) = style.font(env) else {
            return;
        };
        // The search, as a field.
        let focused = self.shown && self.focus == 0;
        let hint = env.i18n().tr("files.list.find.search");
        ui.canvas()
            .describe(Role::Button, hint, self.search)
            .focused(focused);
        if focused {
            ui.focus(self.search);
        } else {
            let field = ui::focus_rect(&style, self.search);
            let radius = style.radius(u32::from(field.height));
            ui.canvas()
                .stroke_round(field, u32::from(style.line()).max(1), radius, Color::BLACK);
        }
        let ink = style.focus_ink(focused);
        let margin = style.margin() as f32;
        let size = style.text() * ui::ICON_SCALE;
        let w = ui.icon("search", size, margin, self.search, ink);
        let x = margin + w + style.gap() as f32;
        let faint = style.focus_faint(focused, ui::Ink::DETAIL);
        ui::Label::new(hint)
            .px(style.text())
            .color(faint)
            .draw_from(&mut ui, x, self.search);
        // The letters, the focused one on a dark circle.
        let big = style.title() * 1.2;
        for (n, (cell, letter)) in self.cells.iter().zip(&self.letters).enumerate() {
            let focused = self.shown && self.focus == n + 1;
            let label = [*letter];
            let text = core::str::from_utf8(&label).unwrap_or("#");
            ui.canvas()
                .describe(Role::Button, text, *cell)
                .focused(focused);
            let ink = if focused {
                let inner = cell.inset(style.gap() / 3, style.gap() / 3);
                ui.canvas()
                    .fill_round(inner, u32::from(inner.height / 2), Color::BLACK);
                Color::WHITE
            } else {
                Color::BLACK
            };
            let w = font.measure(big, text);
            let x = f32::from(cell.x) + (f32::from(cell.width) - w) / 2.0;
            ui::Label::new(text)
                .px(big)
                .color(ink)
                .draw_from(&mut ui, x, *cell);
        }
        let note = ui::tr_input(env, "files.list.find.note");
        ui.message(self.note, note);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let columns = self.columns as isize;
        if [actions::LEFT, actions::PREVIOUS].contains(a) {
            self.step(-1, cx);
        } else if [actions::RIGHT, actions::NEXT].contains(a) {
            self.step(1, cx);
        } else if [actions::UP, actions::PREV_PAGE].contains(a) {
            self.step(-columns, cx);
        } else if [actions::DOWN, actions::NEXT_PAGE].contains(a) {
            self.step(columns, cx);
        } else if *a == actions::CONFIRM {
            self.choose(self.focus, cx);
        } else if *a == actions::BACK {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let point = gesture.start;
        if let Some(action) = self.header.tap(cx.env(), &self.style, point, None) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        if self.search.contains(point) {
            self.choose(0, cx);
        } else if let Some(n) = self.cells.iter().position(|c| c.contains(point)) {
            self.choose(n + 1, cx);
        }
        Handled::Yes
    }
}
