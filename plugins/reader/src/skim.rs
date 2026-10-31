//! Skimming through the book (an overlay at the bottom): where one is,
//! a bar across the book with its chapters (tap anywhere on it to go
//! there), and buttons for the chapter before and after, back to where
//! skimming began, and a page number on the keyboard. With buttons, the
//! page buttons go ten pages, left and right a chapter, up and down move
//! to the buttons below the bar and back to the bar, confirm presses the
//! one moved to (on the bar, stays) and back returns to where one was. While no button has
//! the focus, the bar has it (shown as the style shows a focus): left and
//! right move along it and confirm stays.
//!
//! It fills the slot `core.skim`: it asks the reader to move with the
//! event `core.skim` and is told where the book is with `core.skimmed`.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen,
};

/// The buttons: chapter back, where one was, chapter on, page number.
const BUTTONS: [&str; 4] = [
    "reader.skim.chapter-back",
    "reader.skim.return",
    "reader.skim.chapter-on",
    "reader.skim.page",
];
const MOVES: [&str; 4] = ["chapter:-1", "back", "chapter:+1", "number"];

pub(crate) struct SkimScreen {
    style: UiStyle,
    region: Rect,
    path: String,
    /// Where the book is: label, chapter and thousandths.
    label: String,
    chapter: String,
    at: u32,
    marks: Vec<u32>,
    /// The bar and the buttons.
    bar: Rect,
    buttons: [Rect; 4],
    /// The button the buttons moved to (none at first and after a touch).
    focus: Option<usize>,
    /// The focus shows: buttons used, or no touch screen.
    keys: bool,
}

impl SkimScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        // Its bar and buttons are controls: as large at every size.
        let (margin, gap, row) = (style.margin(), style.gap(), style.control_row());
        // The buttons' row grows with them on touch screens.
        let button_row = style.touch_row(u32::from(row)) as u16;
        // Four in a row where their names fit whole, else two rows of two,
        // else one below the other: a name is never cut for the room.
        let width = display.width.saturating_sub(2 * margin);
        let fits = |columns: u16| {
            let room = width.saturating_sub((columns - 1) * gap) / columns;
            style.font(env).is_none_or(|font| {
                BUTTONS.iter().all(|key| {
                    font.measure(style.text(), env.i18n().tr(key)) + 2.0 * style.gap() as f32
                        <= f32::from(room)
                })
            })
        };
        let columns: u16 = [4, 2].into_iter().find(|&n| fits(n)).unwrap_or(1);
        let lines = 4 / columns;
        // Each line a touch row high; one below the other (the largest
        // text) each as high as its button, a line higher for each more
        // line a long name takes.
        let slots: [u16; 4] = if columns == 1 {
            BUTTONS.map(|key| ui::button_height_for(env, &style, width, env.i18n().tr(key)))
        } else {
            [button_row; 4]
        };
        let line_height = |line: u16| {
            (0..columns)
                .map(|c| slots[usize::from(line * columns + c)])
                .max()
                .unwrap_or(0)
        };
        let buttons_height = (0..lines).map(line_height).sum::<u16>() + gap * (lines - 1);
        // Its buttons keep the side margin from the bottom edge too.
        let height = gap + row * 2 + buttons_height + ui::edge_room(&style, button_row);
        let region = Rect::new(
            0,
            display.height.saturating_sub(height),
            display.width,
            height,
        );
        let bar = Rect::new(margin, region.y + gap + row, width, row);
        let button_width = width.saturating_sub((columns - 1) * gap) / columns;
        let buttons_y = region.y + gap + row * 2;
        let buttons = core::array::from_fn(|i| {
            let (column, line) = (i as u16 % columns, i as u16 / columns);
            let top: u16 = (0..line).map(|l| line_height(l) + gap).sum();
            Rect::new(
                margin + column * (button_width + gap),
                buttons_y + top,
                button_width,
                line_height(line),
            )
        });
        let mut screen = Self {
            focus: None,
            keys: ui::focus_shown(env),
            style,
            region,
            path: params.get(params::PATH).unwrap_or_default().to_string(),
            label: String::new(),
            chapter: String::new(),
            at: 0,
            marks: events::Skimmed::from_params(params)
                .and_then(|s| s.marks)
                .unwrap_or_default()
                .split(',')
                .filter_map(|m| m.trim().parse().ok())
                .collect(),
            bar,
            buttons,
        };
        if let Some(skimmed) = events::Skimmed::from_params(params) {
            screen.update(&skimmed);
        }
        screen
    }

    /// Takes over where the book is now.
    fn update(&mut self, skimmed: &events::Skimmed) {
        self.label.clone_from(&skimmed.page);
        self.chapter.clone_from(&skimmed.chapter);
        self.at = skimmed.permille.min(1000);
    }

    fn go(&self, to: &str, cx: &mut Cx<'_, '_>) {
        cx.command(events::SKIM.emit(events::Skim::new(self.path.clone(), to.to_string())));
    }

    fn press(&self, index: usize, cx: &mut Cx<'_, '_>) {
        match MOVES[index] {
            "number" => {
                cx.command(Command::Back);
                cx.command(Command::run(crate::actions::GO_TO_PAGE));
            }
            "back" => {
                self.go("back", cx);
                cx.command(Command::Back);
            }
            to => self.go(to, cx),
        }
    }

    /// What a focus on button `focus`, or on the bar where none, draws
    /// over: the bar's row widened as its focus is, or the button and its
    /// frame.
    fn focus_area(&self, focus: Option<usize>) -> Rect {
        let style = &self.style;
        match focus.and_then(|i| self.buttons.get(i)) {
            Some(slot) => ui::button_reach(style, ui::button_in_slot(style, *slot)),
            None => {
                let side = style.margin() / 2;
                Rect::new(
                    self.bar.x.saturating_sub(side),
                    self.bar.y,
                    self.bar.width + 2 * side,
                    self.bar.height,
                )
            }
        }
    }

    /// The side of the knob that says where one is.
    fn knob(&self) -> u16 {
        (self.bar.height / 2).max(8)
    }

    /// The bar's track, where its line is drawn: inside the bar by half a
    /// knob and a little more, so that the knob stays inside it (and
    /// inside its frame) at either end.
    fn track(&self) -> Rect {
        let style = self.style;
        let h = (u32::from(style.line()) * 3).max(3) as u16;
        let inset = (self.knob() / 2 + style.gap() / 2).min(self.bar.width / 4);
        Rect::new(
            self.bar.x + inset,
            self.bar.y + (self.bar.height - h) / 2,
            self.bar.width - 2 * inset,
            h,
        )
    }
}

impl Screen for SkimScreen {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let r = self.region;
        let mut ui = ui::Ui::new(canvas, env, &style);
        ui.canvas().fill(r, Color::WHITE);
        ui.canvas().fill(
            Rect::new(r.x, r.y, r.width, (u32::from(style.line()) * 2) as u16),
            Color::BLACK,
        );
        if ui.font().is_none() {
            return;
        }
        // Where one is: the page, then the chapter.
        let x = f32::from(style.margin());
        let info = Rect::new(
            style.margin(),
            r.y + style.gap(),
            r.width.saturating_sub(2 * style.margin()),
            style.row(),
        );
        let label_end = ui::Label::new(&self.label)
            .px(style.text())
            .color(Color::BLACK)
            .draw_from(&mut ui, x, info);
        if !self.chapter.is_empty() {
            let at = label_end + style.gap() as f32 * 2.0;
            ui::Label::new(&self.chapter)
                .px(style.small())
                .color(style.faint(ui::Ink::DETAIL))
                .draw_from(&mut ui, at, info);
        }
        // The bar has the focus while no button has it, shown as the
        // style shows a focus (its own row widened by what the focus takes
        // in, so that it ends where the buttons do).
        let on_bar = self.keys && self.focus.is_none();
        if on_bar {
            let side = style.margin() / 2;
            let around = Rect::new(
                self.bar.x.saturating_sub(side),
                self.bar.y,
                self.bar.width + 2 * side,
                self.bar.height,
            );
            ui.focus(around);
        }
        let ink = style.focus_ink(on_bar);
        // The bar: read up to here, chapters as ticks, a knob where one is.
        let track = self.track();
        let width = u32::from(track.width);
        let done = (width * self.at / 1000) as u16;
        let canvas = ui.canvas();
        if on_bar && style.dark_focus() {
            // Gray does not show on the dark focus: a thin line of its ink.
            let thin = (style.line()).max(1);
            let y = track.y + (track.height - thin) / 2;
            canvas.fill(Rect::new(track.x, y, track.width, thin), ink);
        } else {
            canvas.fill_tint(track, Color::gray(170));
        }
        canvas.fill(Rect::new(track.x, track.y, done, track.height), ink);
        let tick = track.height * 3;
        for &mark in self.marks.iter().filter(|&&m| m > 0 && m < 1000) {
            let tx = track.x + (width * mark / 1000) as u16;
            canvas.fill(
                Rect::new(
                    tx,
                    track.y + track.height / 2 - tick / 2,
                    (style.line()).max(1),
                    tick,
                ),
                ink,
            );
        }
        let knob = self.knob();
        let kx = (track.x + done).saturating_sub(knob / 2);
        let ky = self.bar.y + (self.bar.height - knob) / 2;
        canvas.fill(Rect::new(kx, ky, knob, knob), ink);
        canvas
            .describe(paperpaw_sdk::semantics::Role::Slider, &self.label, self.bar)
            .focused(on_bar);
        // The buttons.
        for (i, (slot, key)) in self.buttons.iter().zip(BUTTONS).enumerate() {
            ui.button(
                ui::button_in_slot(&style, *slot),
                env.i18n().tr(key),
                false,
                self.focus == Some(i),
            );
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        // A direction after touching shows the focus (turning pages, also
        // by swiping, does not; nor what closes the screen).
        let directions = [
            actions::PREVIOUS,
            actions::NEXT,
            actions::LEFT,
            actions::RIGHT,
            actions::UP,
            actions::DOWN,
        ];
        if !self.keys && directions.contains(a) {
            self.keys = true;
            cx.request_redraw_area(self.region);
            cx.request_refresh(RefreshMode::Fast);
        }
        if *a == actions::PREV_PAGE || *a == actions::PREVIOUS {
            self.go("page:-10", cx);
        } else if *a == actions::NEXT_PAGE || *a == actions::NEXT {
            self.go("page:+10", cx);
        } else if *a == actions::LEFT {
            self.go("chapter:-1", cx);
        } else if *a == actions::RIGHT {
            self.go("chapter:+1", cx);
        } else if *a == actions::UP || *a == actions::DOWN {
            // The bar, then the buttons, then the bar again.
            let len = BUTTONS.len();
            let down = *a == actions::DOWN;
            let left = self.focus_area(self.focus);
            self.focus = match self.focus {
                None if down => Some(0),
                None => Some(len - 1),
                Some(at) if down => (at + 1 < len).then_some(at + 1),
                Some(at) => at.checked_sub(1),
            };
            ui::ListChange::focus(left, self.focus_area(self.focus)).request(cx);
        } else if *a == actions::CONFIRM {
            match self.focus {
                Some(i) => self.press(i, cx),
                // Stays where it is.
                None => cx.command(Command::Back),
            }
        } else if *a == actions::BACK {
            self.go("back", cx);
            cx.command(Command::Back);
        } else if *a == actions::MENU {
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
        let at = gesture.start;
        if let Some(i) = self.buttons.iter().position(|b| b.contains(at)) {
            self.focus = None;
            self.keys = false;
            self.press(i, cx);
        } else if self.bar.contains(at) {
            let track = self.track();
            let x = (at.x - i32::from(track.x)).clamp(0, i32::from(track.width));
            let permille = x as u32 * 1000 / u32::from(track.width.max(1));
            self.go(&alloc::format!("permille:{permille}"), cx);
        } else if !self.region.contains(at) {
            // The page: done skimming, staying here.
            cx.command(Command::Back);
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(skimmed) = events::SKIMMED.read(event, params)
            && skimmed.path == self.path
        {
            self.update(&skimmed);
            cx.request_redraw_area(self.region);
            cx.request_refresh(RefreshMode::Fast);
        }
    }
}
