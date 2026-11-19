//! LAB ONLY (#40, feature `turn-lab`): the page-turn lab's screen. A test
//! page (text, a row of grays, a black bar) that "turns" between two
//! versions with the page-turn refresh (`RefreshMode::Partial`), and four
//! rows to judge it: the preset (the choice setting the shell names, the
//! words and the list of choices come in the params), one page, a loop of
//! turns, back to the first preset. The shell shows a page turn with the
//! waveform the preset says; the time of the last turn it feeds back with
//! [`set_last_ms`].

use core::sync::atomic::{AtomicU32, Ordering};

use alloc::string::String;
use alloc::vec::Vec;

use super::paint::{self, Look};
use crate::{Ask, Place, Turn};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{
    Canvas, Color, Gesture, GestureKind, Handled, Hint, Id, Instant, Params, Point, Rect,
    RefreshMode, SettingValue,
};

/// The screen's id.
pub const SCREEN: Id = Id::new_static("core.turn-lab");
/// Its title, for lists of screens.
pub const TITLE: &str = "x4.turn-lab";

/// Params: the choice setting's id, its choices (comma separated) and their
/// names (`|` separated), and the words of the screen.
pub const SETTING: &str = "setting";
pub const CHOICES: &str = "choices";
pub const NAMES: &str = "names";
pub const WORDS: [&str; 8] = [
    "title", "preset", "turn", "loop", "reset", "last", "text-a", "text-b",
];

/// Turns of the loop.
const LOOP_TURNS: u8 = 20;
/// The rest between two turns of the loop, in milliseconds.
const PAUSE_MS: u64 = 250;
const ROWS: usize = 4;

static LAST_MS: AtomicU32 = AtomicU32::new(0);

/// The shell tells how long the panel took for the last page turn.
pub fn set_last_ms(ms: u32) {
    LAST_MS.store(ms, Ordering::Relaxed);
}

pub(crate) struct TurnLab {
    look: Look,
    setting: String,
    choices: Vec<String>,
    names: Vec<String>,
    words: [String; 8],
    /// The choice shown, and the row with the focus.
    current: usize,
    focus: usize,
    /// Which version of the page, and how many turns were made.
    page_b: bool,
    turns: u32,
    /// Turns the loop still makes.
    looping: u8,
    rows: [Rect; ROWS],
    page: Rect,
}

impl TurnLab {
    pub(crate) fn new(env: &Place<'_>, params: &Params) -> Self {
        let look = Look::of(env.device(), env.settings(), env.fonts());
        let get = |key| params.get(key).unwrap_or("");
        let setting = String::from(get(SETTING));
        let choices: Vec<String> = get(CHOICES).split(',').map(String::from).collect();
        let names = get(NAMES).split('|').map(String::from).collect();
        let current = choices
            .iter()
            .position(|c| c == env.settings().text(&setting))
            .unwrap_or(0);
        let words = WORDS.map(|key| String::from(get(key)));
        let area = env.content();
        let (margin, row) = (look.margin, look.row);
        let gap = look.gap;
        let width = area.width.saturating_sub(2 * margin);
        let controls = ROWS as u16 * (row + gap / 2) + margin;
        let top = area.height.saturating_sub(controls);
        let rows = core::array::from_fn(|n| {
            Rect::new(margin, top + n as u16 * (row + gap / 2), width, row)
        });
        Self {
            look,
            setting,
            choices,
            names,
            words,
            current,
            focus: 1,
            page_b: false,
            turns: 0,
            looping: 0,
            rows,
            page: Rect::new(0, 0, area.width, top.saturating_sub(gap)),
        }
    }

    fn name(&self) -> &str {
        self.names.get(self.current).map_or("?", String::as_str)
    }

    fn label(&self, row: usize) -> String {
        match row {
            0 => alloc::format!("< {}: {} >", self.words[1], self.name()),
            n => self.words[n + 1].clone(),
        }
    }

    pub(super) fn hint(&self, action: &Id) -> Hint {
        if *action == actions::CONFIRM {
            Hint::Text(self.label(self.focus))
        } else {
            Hint::only_back(action)
        }
    }

    /// Shows the other version of the page with the page-turn refresh.
    fn turn(&mut self, cx: &mut Turn<'_, '_>) {
        self.page_b = !self.page_b;
        self.turns += 1;
        cx.request_redraw();
        cx.set_refresh(RefreshMode::Partial);
    }

    fn rows_changed(&self, cx: &mut Turn<'_, '_>) {
        let first = self.rows[0];
        let last = self.rows[ROWS - 1];
        cx.request_redraw_area(Rect::new(
            0,
            first.y.saturating_sub(self.look.gap),
            self.page.width,
            last.y + last.height - first.y + 2 * self.look.gap,
        ));
        cx.request_refresh(RefreshMode::Fast);
    }

    /// The preset `by` steps on (the choices go round), set at once.
    fn step_preset(&mut self, by: isize, cx: &mut Turn<'_, '_>) {
        let count = self.choices.len().max(1) as isize;
        let to = (self.current as isize + by).rem_euclid(count) as usize;
        self.set_preset(to, cx);
    }

    fn set_preset(&mut self, to: usize, cx: &mut Turn<'_, '_>) {
        self.current = to;
        if let (Some(choice), Ok(id)) = (self.choices.get(to), Id::parse(&self.setting)) {
            cx.command(Ask::SetSetting(
                id,
                SettingValue::Text(choice.clone().into()),
            ));
        }
        self.rows_changed(cx);
    }

    fn activate(&mut self, row: usize, cx: &mut Turn<'_, '_>) {
        match row {
            0 => self.step_preset(1, cx),
            1 => self.turn(cx),
            2 => {
                self.looping = LOOP_TURNS;
                self.go_on(cx);
            }
            _ => {
                self.set_preset(0, cx);
                self.looping = 0;
            }
        }
    }

    /// One turn of the loop, and the time of the next.
    fn go_on(&mut self, cx: &mut Turn<'_, '_>) {
        if self.looping == 0 {
            return;
        }
        self.looping -= 1;
        self.turn(cx);
        if self.looping > 0 {
            let now = cx.env().now();
            cx.wake_at(Instant(now.0.saturating_add(PAUSE_MS)));
        }
    }

    pub(super) fn on_timer(&mut self, cx: &mut Turn<'_, '_>) {
        self.go_on(cx);
    }

    pub(super) fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        if self.looping > 0 {
            // Any key ends the loop; Back also leaves.
            self.looping = 0;
            return if *action == actions::BACK {
                Handled::No
            } else {
                Handled::Yes
            };
        }
        let focus = |by: isize, this: &mut Self, cx: &mut Turn<'_, '_>| {
            this.focus = (this.focus as isize + by).clamp(0, ROWS as isize - 1) as usize;
            this.rows_changed(cx);
        };
        if [actions::DOWN, actions::NEXT_PAGE].contains(action) {
            focus(1, self, cx);
        } else if [actions::UP, actions::PREV_PAGE].contains(action) {
            focus(-1, self, cx);
        } else if [actions::RIGHT, actions::NEXT].contains(action) {
            self.step_preset(1, cx);
        } else if [actions::LEFT, actions::PREVIOUS].contains(action) {
            self.step_preset(-1, cx);
        } else if *action == actions::CONFIRM {
            self.activate(self.focus, cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    /// A tap on a row is that row; a tap on the page turns it.
    pub(super) fn on_gesture(&mut self, gesture: Gesture, cx: &mut Turn<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        self.looping = 0;
        let hit = |point: Point| self.rows.iter().position(|r| r.contains(point));
        if let Some(row) = hit(gesture.start) {
            self.focus = row;
            self.activate(row, cx);
        } else if self.page.contains(gesture.start) {
            self.turn(cx);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    pub(super) fn draw(&self, canvas: &mut Canvas<'_>, env: &Place<'_>) {
        let area = env.content();
        canvas.fill(Rect::new(0, 0, area.width, area.height), Color::WHITE);
        let font = paint::font(env.fonts());
        let look = &self.look;
        for (n, row) in self.rows.iter().enumerate() {
            paint::button(
                canvas,
                font.as_ref(),
                look,
                *row,
                &self.label(n),
                n == self.focus,
            );
        }
        self.draw_page(canvas, font.as_ref());
    }

    /// The page: the line of the time, the text, the grays, the bar.
    fn draw_page(&self, canvas: &mut Canvas<'_>, font: Option<&paperpaw_sdk::FontHandle<'_>>) {
        let look = &self.look;
        let (x, w) = (look.margin, self.page.width.saturating_sub(2 * look.margin));
        let mut y = look.gap;
        if let Some(font) = font {
            let ms = LAST_MS.load(Ordering::Relaxed);
            let said = alloc::format!(
                "{}  |  {}: {} ms  |  #{}",
                self.name(),
                self.words[5],
                ms,
                self.turns
            );
            let head = Rect::new(x, y, w, look.row);
            paint::line_at(
                canvas,
                font,
                look.small,
                &said,
                f32::from(x),
                head,
                Color::BLACK,
            );
            y += look.row;
            // The text takes half of what is left.
            let text = if self.page_b {
                &self.words[7]
            } else {
                &self.words[6]
            };
            let line = font.line_metrics(look.text).line_height as u16;
            let room = self.page.height.saturating_sub(y) / 2;
            let fits = (room / line.max(1)) as usize;
            for (n, text_line) in paint::wrapped(font, look.text, text, f32::from(w))
                .iter()
                .take(fits)
                .enumerate()
            {
                let row = Rect::new(x, y + n as u16 * line, w, line);
                paint::line_at(
                    canvas,
                    font,
                    look.text,
                    text_line,
                    f32::from(x),
                    row,
                    Color::BLACK,
                );
            }
            y += room;
        }
        // Eight grays, light to dark on one version and dark to light on the
        // other, then a black bar on the left or the right.
        let left = self.page.height.saturating_sub(y);
        let strip = left * 2 / 5;
        let cell = w / 8;
        for n in 0..8u16 {
            let step = if self.page_b { 7 - n } else { n };
            let level = 255 - u8::try_from(u16::from(255u8) * step / 7).unwrap_or(255);
            canvas.fill(Rect::new(x + n * cell, y, cell, strip), Color::gray(level));
        }
        y += strip + look.gap;
        let bar = left.saturating_sub(strip + 2 * look.gap).min(left / 3);
        let bar_x = if self.page_b { x + w / 2 } else { x };
        canvas.fill(Rect::new(bar_x, y, w / 2, bar), Color::BLACK);
    }
}
