//! The guide: which page is shown, what has the focus, what an input means,
//! and how a page is painted. The meaning of an input is a plain value
//! ([`Act`]) the plugin carries out, so it is tested without the host.
use crate::native_layout::{
    Answer, Content, END_LINES, HELP_LINES, Layout, Measure, Sizes, layout,
};
use crate::native_steps::{Control, NewValue, Page, Setting, Step};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error,
        callbacks::{Input, InputGestureKind},
        fonts::Font,
    },
};
use paperpaw_ui_kit::{
    Paint, Style, geometry, paint,
    text::{fit, label, wrap_with},
};

/// What can be chosen on a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The answer's card.
    Card,
    Next,
    /// *Skip* on the first page, *Back* on the others.
    Other,
}

/// Where the last frame drew each target, with the room of its focus ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drawn {
    /// Where the frame's region began on the screen.
    pub origin: (i32, i32),
    card: Option<Rect>,
    next: Rect,
    other: Rect,
}

impl Drawn {
    /// The targets of `layout`, drawn in the look `style` by the frame that
    /// began at `origin`.
    #[must_use]
    pub fn new(origin: (i32, i32), layout: &Layout, style: &Style) -> Self {
        Self {
            origin,
            card: layout.card,
            next: ring_around(layout.next, style),
            other: ring_around(layout.other, style),
        }
    }

    /// Where `target` was drawn, if it was.
    #[must_use]
    pub fn area(&self, target: Target) -> Option<Rect> {
        match target {
            Target::Card => self.card,
            Target::Next => Some(self.next),
            Target::Other => Some(self.other),
        }
    }
}

/// The texts of the guide that no step brings, translated.
#[derive(Debug, Default, Clone)]
pub struct Texts {
    pub next: String,
    pub done: String,
    pub skip: String,
    pub skip_short: String,
    pub back: String,
    pub open: String,
    pub on: String,
    pub off: String,
    pub sample: String,
    pub end_title: String,
    pub end_help: String,
    /// The key bar's words while a number is changed.
    pub keep: String,
    pub cancel: String,
}

/// What an input means, for the plugin to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Nothing for the guide.
    Ignore,
    /// Taken, nothing to do.
    Keep,
    /// The focus moved (or showed) from the first target to the second.
    Focus(Target),
    /// Go to this page; past the last, the setup is done.
    Turn(usize),
    /// Ask whether to skip the setup (*Back* on the first page).
    AskSkip,
    /// Set the shown setting to this.
    Set(NewValue),
    /// Move the number shown by this many steps: at once, or while it is
    /// changed with the buttons only on the page until it is kept.
    Step(i64),
    /// Start changing the number with the buttons ([`Guide::editing`]).
    Edit,
    /// Stop changing the number: keep the value shown (`true`) or go back
    /// to the one before.
    EndEdit(bool),
    /// Open the screen of a screen step ([`Guide::screen_step`]).
    OpenScreen,
    /// Open the editor of the shown setting ([`Guide::editor`]).
    OpenEditor,
    /// Open the list of the options of the shown setting to choose from.
    OpenList,
}

/// The guide's state.
#[derive(Debug)]
pub struct Guide {
    pub steps: Vec<Step>,
    pub page: usize,
    /// The setting of the page shown, if it asks for one.
    pub setting: Option<Setting>,
    /// What a screen step says about itself (its plugin's `<id>.help`); empty
    /// for a page with a setting, which has the setting's own.
    pub help: String,
    pub focus: Target,
    /// Whether the focus shows (once a button was used; at once without touch).
    pub shown: bool,
    /// The number's value before, while it is changed with the buttons
    /// (Select on its card, as in Beta 3 and on the settings pages).
    pub editing: Option<i64>,
    /// Whether a screen was opened from the page (a list, an editor, a
    /// step's screen): its setting is read again once the guide shows.
    pub away: bool,
    pub texts: Texts,
    /// The screen's size in pixels and its dots per inch.
    pub display: (u16, u16, u16),
    /// Where the last frame drew the targets; none before the first.
    pub drawn: Option<Drawn>,
}

impl Guide {
    #[must_use]
    pub fn new(steps: Vec<Step>, texts: Texts, display: (u16, u16, u16), touch: bool) -> Self {
        Self {
            steps,
            page: 0,
            setting: None,
            help: String::new(),
            focus: Target::Next,
            shown: !touch,
            editing: None,
            away: false,
            texts,
            display,
            drawn: None,
        }
    }

    /// Shows page `page`, the `gone` steps from it left out, with its setting.
    /// A page turned to has the focus on *Next*; the page shown, read again
    /// (back from the list of its choice, the language changed), keeps it
    /// where it was, as in Beta 3: on the card the answer was just chosen in.
    pub fn commit(&mut self, page: usize, gone: usize, (setting, help): (Option<Setting>, String)) {
        let end = page.saturating_add(gone).min(self.steps.len());
        self.steps.drain(page.min(end)..end);
        if page != self.page || gone > 0 {
            self.focus = Target::Next;
        }
        self.page = page;
        self.editing = None;
        self.setting = setting;
        self.help = help;
    }

    /// Takes the steps' titles from `fresh`, the steps read again (in the
    /// language just chosen); a step not among them keeps its own.
    pub fn retitle(&mut self, fresh: Vec<Step>) {
        for again in fresh {
            if let Some(step) = self.steps.iter_mut().find(|step| step.id == again.id) {
                step.title = again.title;
            }
        }
    }

    /// Whether `page` is the last one, past all the steps.
    #[must_use]
    pub fn at_end(&self) -> bool {
        self.page >= self.steps.len()
    }

    /// The step of the page shown; `None` at the end.
    #[must_use]
    pub fn step(&self) -> Option<&Step> {
        self.steps.get(self.page)
    }

    /// The question and its explanation.
    #[must_use]
    pub fn words(&self) -> (&str, &str) {
        let (Some(step), setting) = (self.step(), self.setting.as_ref()) else {
            return (&self.texts.end_title, &self.texts.end_help);
        };
        let title = if step.title.is_empty() {
            setting.map_or("", |s| s.title.as_str())
        } else {
            &step.title
        };
        (
            title,
            setting.map_or(self.help.as_str(), |s| s.help.as_str()),
        )
    }

    /// The pixel size of a text-size setting's sample: only for the reader's
    /// text size, which is in points.
    #[must_use]
    pub fn sample_size(&self) -> Option<f32> {
        let setting = self.setting.as_ref()?;
        if setting.key != paperpaw_sdk::builtin::settings::FONT_SIZE.as_str() {
            return None;
        }
        let Control::Number { value, .. } = setting.control else {
            return None;
        };
        let dpi = f32::from(self.display.2.max(1));
        Some(value as f32 * dpi / 72.0)
    }

    /// What the page asks for.
    #[must_use]
    pub fn answer(&self) -> Answer {
        if self.at_end() {
            return Answer::None;
        }
        match self.setting.as_ref().map(|s| &s.control) {
            Some(Control::Number { .. }) => Answer::Number {
                sample: self.sample_size(),
            },
            Some(Control::Choice { options, .. }) if options.len() == 2 => Answer::Two,
            _ => Answer::Row,
        }
    }

    /// The page's content for the layout.
    #[must_use]
    pub fn content(&self) -> Content<'_> {
        let (title, help) = self.words();
        Content {
            title,
            help,
            help_lines: if self.at_end() { END_LINES } else { HELP_LINES },
            answer: self.answer(),
        }
    }

    /// What can be chosen on the page, top to bottom: up to three, and how
    /// many.
    #[must_use]
    pub fn targets(&self, layout: &Layout) -> ([Target; 3], usize) {
        if layout.card.is_some() {
            ([Target::Card, Target::Next, Target::Other], 3)
        } else {
            ([Target::Next, Target::Other, Target::Other], 2)
        }
    }

    fn next_label(&self) -> &str {
        if self.at_end() {
            &self.texts.done
        } else {
            &self.texts.next
        }
    }

    fn other_label(&self, tight: bool) -> &str {
        match (self.page, tight) {
            (0, false) => &self.texts.skip,
            (0, true) => &self.texts.skip_short,
            _ => &self.texts.back,
        }
    }

    /// What choosing the card does, as on the settings pages: its editor; a
    /// choice of more than two opens the list of its options; a switch or a
    /// choice of two changes at once. A number changes with its − and +.
    fn card_act(&self) -> Act {
        if self.screen_step().is_some() {
            return Act::OpenScreen;
        }
        let Some(setting) = &self.setting else {
            return Act::Keep;
        };
        if setting.editor.is_some() {
            return Act::OpenEditor;
        }
        match &setting.control {
            Control::Number { .. } | Control::Value(_) => Act::Keep,
            Control::Choice { options, .. } if options.len() > 2 => Act::OpenList,
            _ => setting.next(1).map_or(Act::Keep, Act::Set),
        }
    }

    /// The screen of the page, if it is a screen step.
    #[must_use]
    pub fn screen_step(&self) -> Option<&str> {
        match self.step()? {
            Step {
                page: Page::Screen(screen),
                ..
            } => Some(screen),
            _ => None,
        }
    }

    /// The editor of the setting shown: `(is_slot, id)`.
    #[must_use]
    pub fn editor(&self) -> Option<(bool, &str)> {
        let (slot, id) = self.setting.as_ref()?.editor.as_ref()?;
        Some((*slot, id))
    }

    fn choose(&self, target: Target) -> Act {
        match target {
            Target::Next => Act::Turn(self.page + 1),
            Target::Other if self.page == 0 => Act::AskSkip,
            Target::Other => Act::Turn(self.page - 1),
            Target::Card => self.card_act(),
        }
    }

    /// Moves the focus by `delta` among the targets, or shows it first.
    fn moved(&self, layout: &Layout, delta: isize) -> Act {
        if !self.shown {
            return Act::Focus(self.focus);
        }
        let (targets, count) = self.targets(layout);
        let targets = &targets[..count];
        let at = targets.iter().position(|t| *t == self.focus).unwrap_or(0) as isize;
        let to = (at + delta).rem_euclid(count as isize) as usize;
        Act::Focus(targets[to])
    }

    /// What `input` means on the page laid out as `layout`.
    #[must_use]
    pub fn decide(&self, layout: &Layout, input: &Input<'_>) -> Act {
        match *input {
            Input::Action { action, .. } => self.on_action(layout, action),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.on_tap(layout, position),
            // Another gesture (a swipe, a long press) is not the guide's, as
            // in Beta 3.
            _ => Act::Ignore,
        }
    }

    fn on_action(&self, layout: &Layout, action: &str) -> Act {
        if self.editing.is_some() {
            return Self::on_edit_action(action, self.page);
        }
        match action {
            "core.confirm" if !self.shown => Act::Focus(self.focus),
            "core.confirm"
                if self.focus == Target::Card && matches!(self.answer(), Answer::Number { .. }) =>
            {
                Act::Edit
            }
            "core.confirm" => self.choose(self.focus),
            // The arrows only move the focus, as in Beta 3: on a device with
            // left and right only, they are its way off the card (#476).
            "core.down" | "core.right" | "core.next" => self.moved(layout, 1),
            "core.up" | "core.left" | "core.previous" => self.moved(layout, -1),
            "core.next-page" => Act::Turn(self.page + 1),
            "core.prev-page" if self.page > 0 => Act::Turn(self.page - 1),
            "core.back" if self.page == 0 => Act::AskSkip,
            "core.back" => Act::Turn(self.page - 1),
            _ => Act::Ignore,
        }
    }

    /// The buttons while a number is changed, as in Beta 3: the arrows move
    /// it, Select keeps it, Back goes back to the value before (and only
    /// that: one key, one step); the page buttons keep it and turn the page.
    /// Any other (*Power*, the menu) is not the guide's.
    fn on_edit_action(action: &str, page: usize) -> Act {
        match action {
            "core.right" | "core.up" | "core.next" => Act::Step(1),
            "core.left" | "core.down" | "core.previous" => Act::Step(-1),
            "core.confirm" => Act::EndEdit(true),
            "core.back" => Act::EndEdit(false),
            "core.next-page" => Act::Turn(page + 1),
            "core.prev-page" if page > 0 => Act::Turn(page - 1),
            _ => Act::Ignore,
        }
    }

    /// Chooses option `index` of a choice of two, tapped.
    fn pick(&self, index: usize) -> Act {
        match self.setting.as_ref().map(|s| &s.control) {
            Some(Control::Choice { options, at }) if options.len() == 2 => {
                if *at == Some(index) {
                    Act::Keep
                } else {
                    Act::Set(NewValue::Text(index))
                }
            }
            _ => Act::Keep,
        }
    }

    fn on_tap(&self, layout: &Layout, position: Point) -> Act {
        // A finger ends changing with the buttons, keeping the value shown.
        if self.editing.is_some() {
            return Act::EndEdit(true);
        }
        let at = Point::new(position.x, position.y);
        if layout.next.contains(at) {
            return self.choose(Target::Next);
        }
        if layout.other.contains(at) {
            return self.choose(Target::Other);
        }
        if let Some((less, more)) = layout.stepper {
            for (rect, delta) in [(less, -1), (more, 1)] {
                if rect.contains(at) {
                    return Act::Step(delta);
                }
            }
        }
        if let Some((first, second)) = layout.segments {
            for (rect, index) in [(first, 0), (second, 1)] {
                if rect.contains(at) {
                    return self.pick(index);
                }
            }
        }
        if layout.card.is_some_and(|card| card.contains(at)) {
            return self.card_act();
        }
        Act::Keep
    }

    // ---- painting

    /// The label of the card for a page that is not a setting.
    fn card_label(&self) -> &str {
        &self.texts.open
    }

    /// Prepares the page's drawing: in `ops` what is drawn in the UI font, in
    /// `book` what is drawn in the reading font (the sample of the text size,
    /// which looks as the book will, as in Beta 3).
    ///
    /// # Errors
    /// [`Error::NoMemory`] or a refused font reply.
    pub fn prepare(
        &self,
        ops: &mut Vec<Paint>,
        book: &mut Vec<Paint>,
        fonts: &Fonts<'_, '_>,
        style: &Style,
        layout: &Layout,
    ) -> Result<(), Error> {
        let ink = style.ink;
        paint::push(
            ops,
            Paint::Fill(Rect::new(0, 0, self.display.0, self.display.1), style.paper),
        )?;
        self.dots(ops, layout, ink)?;
        let (title, help) = self.words();
        lines(
            ops,
            fonts.ui,
            title,
            style.title * 1.3,
            layout.title,
            ink,
            2,
        )?;
        let help_lines = if self.at_end() { END_LINES } else { HELP_LINES };
        lines(
            ops,
            fonts.ui,
            help,
            style.small,
            layout.help,
            style.muted,
            help_lines,
        )?;
        if let Some(card) = layout.card {
            self.card(ops, book, fonts, style, layout, card)?;
        }
        let next = (layout.next, self.next_label(), self.focus == Target::Next);
        let other = (
            layout.other,
            self.other_label(layout.tight),
            self.focus == Target::Other,
        );
        for (rect, name, focused) in [next, other] {
            button(
                ops,
                fonts.ui,
                style,
                rect,
                name,
                focused && self.shown,
                false,
            )?;
        }
        Ok(())
    }

    fn dots(&self, ops: &mut Vec<Paint>, layout: &Layout, ink: Color) -> Result<(), Error> {
        let area = layout.dots;
        let count = self.steps.len() + 1;
        let r = (i32::from(area.height) / 2).max(3);
        let gap = r * 4;
        let span = i32::try_from(count).unwrap_or(i32::MAX).saturating_sub(1) * gap;
        let mut x = i32::from(area.x) + i32::from(area.width) / 2 - span / 2;
        let y = i32::from(area.y) + r;
        for i in 0..count {
            let d = u16::try_from(r * 2).unwrap_or(6);
            let at = Rect::new(
                geometry::clamp_u16(i64::from(x - r)),
                geometry::clamp_u16(i64::from(y - r)),
                d,
                d,
            );
            let fill = if i <= self.page { None } else { Some(2) };
            paint::push(ops, Paint::Focus(at, u32::from(d / 2), fill, ink))?;
            x += gap;
        }
        Ok(())
    }

    #[expect(clippy::too_many_lines, reason = "one arm a kind of answer")]
    fn card(
        &self,
        ops: &mut Vec<Paint>,
        book: &mut Vec<Paint>,
        fonts: &Fonts<'_, '_>,
        style: &Style,
        layout: &Layout,
        card: Rect,
    ) -> Result<(), Error> {
        let ink = style.ink;
        let focused = self.shown && self.focus == Target::Card;
        let line = u32::from(style.line).max(1);
        if focused {
            paperpaw_ui_kit::focus::ring(ops, style, card)?;
        } else if layout.segments.is_none() {
            paint::push(ops, Paint::Focus(card, style.corner, Some(line), ink))?;
        }
        let row = Rect::new(card.x, card.y, card.width, style.row.min(card.height));
        let setting = self.setting.as_ref();
        if let (Some((first, second)), Some(setting)) = (layout.segments, setting)
            && let Control::Choice { options, at } = &setting.control
        {
            for (index, (rect, option)) in [first, second].into_iter().zip(options).enumerate() {
                button(
                    ops,
                    fonts.ui,
                    style,
                    rect,
                    &option.title,
                    false,
                    *at == Some(index),
                )?;
            }
            return Ok(());
        }
        let Some(setting) = setting else {
            return opens(ops, fonts.ui, style, card, self.card_label(), ink);
        };
        match &setting.control {
            Control::Switch(on) => {
                let inner = Rect::new(
                    card.x.saturating_add(style.margin),
                    row.y,
                    card.width.saturating_sub(style.margin.saturating_mul(2)),
                    row.height,
                );
                let track_h = (inner.height / 2).max(8);
                let track_w = track_h.saturating_mul(2);
                let track = Rect::new(
                    (inner.x.saturating_add(inner.width)).saturating_sub(track_w),
                    inner
                        .y
                        .saturating_add(inner.height.saturating_sub(track_h) / 2),
                    track_w,
                    track_h,
                );
                let radius = u32::from(track_h / 2);
                let knob = track_h.saturating_sub(6).max(4);
                let knob_x = if *on {
                    (track.x.saturating_add(track.width)).saturating_sub(knob + 3)
                } else {
                    track.x.saturating_add(3)
                };
                let knob_rect = Rect::new(knob_x, track.y.saturating_add(3), knob, knob);
                if *on {
                    paint::push(ops, Paint::Focus(track, radius, None, ink))?;
                    paint::push(
                        ops,
                        Paint::Focus(knob_rect, u32::from(knob / 2), None, style.paper),
                    )?;
                } else {
                    paint::push(ops, Paint::Focus(track, radius, Some(line), ink))?;
                    paint::push(ops, Paint::Focus(knob_rect, u32::from(knob / 2), None, ink))?;
                }
                let room = Rect::new(
                    inner.x,
                    inner.y,
                    track.x.saturating_sub(inner.x).saturating_sub(style.gap),
                    inner.height,
                );
                let x = f32::from(room.x);
                label(
                    ops,
                    fonts.ui,
                    &setting.title,
                    style.text,
                    x,
                    room,
                    ink,
                    Some(room),
                )
            }
            Control::Number { .. } => {
                let (Some((less, more)), shown) = (
                    layout.stepper,
                    setting.label(&self.texts.on, &self.texts.off)?,
                ) else {
                    return Ok(());
                };
                // Filled while the buttons change it, as in Beta 3.
                let editing = self.editing.is_some();
                button(ops, fonts.ui, style, less, "−", false, editing)?;
                button(ops, fonts.ui, style, more, "+", false, editing)?;
                let between = Rect::new(
                    less.x.saturating_add(less.width),
                    less.y,
                    more.x.saturating_sub(less.x.saturating_add(less.width)),
                    less.height,
                );
                centered(ops, fonts.ui, &shown, style.title, between, ink)?;
                if let (Some(size), Some(reading)) = (self.sample_size(), fonts.reading) {
                    let top = less.y.saturating_add(less.height).saturating_add(style.gap);
                    let area = Rect::new(
                        card.x.saturating_add(style.margin),
                        top,
                        card.width.saturating_sub(style.margin.saturating_mul(2)),
                        (card.y.saturating_add(card.height))
                            .saturating_sub(top.saturating_add(style.gap)),
                    );
                    let height = geometry::line_px(reading, size)?.max(1);
                    let fits = usize::from(area.height / height).max(1);
                    lines(
                        book,
                        reading,
                        &self.texts.sample,
                        size,
                        Rect::new(
                            area.x,
                            area.y,
                            area.width,
                            height.saturating_mul(u16::try_from(fits).unwrap_or(1)),
                        ),
                        ink,
                        fits,
                    )?;
                }
                Ok(())
            }
            Control::Value(_) if setting.editor.is_some() => {
                let shown = setting.label(&self.texts.on, &self.texts.off)?;
                opens(ops, fonts.ui, style, card, &shown, ink)
            }
            // More than two options: the card opens their list, shown with an arrow.
            Control::Choice { .. } => {
                let shown = setting.label(&self.texts.on, &self.texts.off)?;
                opens(ops, fonts.ui, style, card, &shown, ink)
            }
            _ => {
                let shown = setting.label(&self.texts.on, &self.texts.off)?;
                centered(ops, fonts.ui, &shown, style.text, row, ink)
            }
        }
    }
}

/// The fonts a page is drawn with.
pub struct Fonts<'a, 'h> {
    pub ui: &'a Font<'h>,
    /// The reading font, for the sample of the text size.
    pub reading: Option<&'a Font<'h>>,
}

/// `text` wrapped in `area`, left aligned, at most `max` lines.
fn lines(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    words: &str,
    size: f32,
    area: Rect,
    color: Color,
    max: usize,
) -> Result<(), Error> {
    let height = geometry::line_px(font, size)?.max(1);
    let wrapped = wrap_with(
        |l| font.measure_text(size, l),
        words,
        f32::from(area.width),
        max,
    )?;
    let mut y = area.y;
    for part in &wrapped {
        let row = Rect::new(area.x, y, area.width, height);
        label(
            ops,
            font,
            part,
            size,
            f32::from(area.x),
            row,
            color,
            Some(row),
        )?;
        y = y.saturating_add(height);
    }
    Ok(())
}

/// `words` centred in `area` on one line, shortened to fit.
fn centered(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    words: &str,
    size: f32,
    area: Rect,
    color: Color,
) -> Result<(), Error> {
    let fitted = fit(font, size, words, f32::from(area.width))?;
    let wide = font.measure_text(size, &fitted)?;
    let x = f32::from(area.x) + (f32::from(area.width) - wide).max(0.0) / 2.0;
    label(ops, font, &fitted, size, x, area, color, Some(area))
}

/// What opens a screen, in the card: its words and an arrow at the left of the
/// card's middle, as in Beta 3.
fn opens(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    card: Rect,
    words: &str,
    ink: Color,
) -> Result<(), Error> {
    let mut shown = String::new();
    shown
        .try_reserve(words.len().saturating_add(4))
        .map_err(|_| Error::NoMemory)?;
    shown.push_str(words);
    shown.push_str(" ›");
    let inner = Rect::new(
        card.x.saturating_add(style.margin),
        card.y,
        card.width.saturating_sub(style.margin.saturating_mul(2)),
        card.height,
    );
    let fitted = fit(font, style.text, &shown, f32::from(inner.width))?;
    label(
        ops,
        font,
        &fitted,
        style.text,
        f32::from(inner.x),
        inner,
        ink,
        Some(inner),
    )
}

/// A button: the one every screen draws, highlighted where `chosen` or `focused`.
fn button(
    ops: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    name: &str,
    focused: bool,
    chosen: bool,
) -> Result<(), Error> {
    paperpaw_ui_kit::parts::button(ops, font, style, rect, name, focused || chosen)?;
    if focused {
        // A ring round the filled button, as in Beta 3: where it has the focus
        // (not where it is only chosen) a button is seen to be the one Select
        // presses, with or without a fill.
        let outer = ring_around(rect, style);
        let radius = if style.pill {
            u32::from(outer.height) / 2
        } else {
            style.corner.saturating_add(u32::from(ring_room(style)))
        };
        let line = u32::from(style.line).max(1);
        paint::push(ops, Paint::Focus(outer, radius, Some(line), style.ink))?;
    }
    Ok(())
}

/// How far the focus ring of a button lies outside it.
fn ring_room(style: &Style) -> u16 {
    style.line.max(1).saturating_mul(2)
}

/// The focus ring round the button `rect`.
fn ring_around(rect: Rect, style: &Style) -> Rect {
    let grow = ring_room(style);
    Rect::new(
        rect.x.saturating_sub(grow),
        rect.y.saturating_sub(grow),
        rect.width.saturating_add(grow.saturating_mul(2)),
        rect.height.saturating_add(grow.saturating_mul(2)),
    )
}

/// The host's font as the layout measures it.
pub struct HostMeasure<'a, 'h> {
    pub ui: &'a Font<'h>,
    pub reading: Option<&'a Font<'h>>,
    pub sample: &'a str,
}

impl Measure for HostMeasure<'_, '_> {
    fn line(&self, size: f32) -> u16 {
        geometry::line_px(self.ui, size).unwrap_or((size * 1.3) as u16)
    }

    fn lines(&self, size: f32, words: &str, width: u16, max: usize) -> usize {
        wrap_with(
            |l| self.ui.measure_text(size, l),
            words,
            f32::from(width),
            max,
        )
        .map_or(1, |lines| lines.len().max(1))
    }

    fn sample(&self, size: f32, width: u16) -> u16 {
        let Some(font) = self.reading else {
            return (size * 1.3) as u16 * 2;
        };
        let lines = wrap_with(
            |l| font.measure_text(size, l),
            self.sample,
            f32::from(width),
            64,
        )
        .map_or(1, |lines| lines.len().max(1));
        geometry::line_px(font, size)
            .unwrap_or((size * 1.3) as u16)
            .saturating_mul(u16::try_from(lines).unwrap_or(u16::MAX))
    }
}

/// The sizes of the page from the style and the screen.
#[must_use]
pub fn sizes(style: &Style, display: (u16, u16, u16), small_line: u16) -> Sizes {
    Sizes {
        width: display.0,
        height: display.1,
        margin: style.margin,
        gap: style.gap,
        row: style.row,
        row_small: (style.row - style.row / 4)
            .max(small_line.saturating_add(style.gap.saturating_mul(2))),
        title: style.title,
        small: style.small,
    }
}

/// The page laid out.
#[must_use]
pub fn lay_out(guide: &Guide, style: &Style, m: &impl Measure) -> Layout {
    let small = m.line(style.small);
    layout(&sizes(style, guide.display, small), m, &guide.content())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_steps::Choice;

    fn step(id: &str, page: Page) -> Step {
        Step {
            id: id.into(),
            title: String::new(),
            order: 0,
            page,
        }
    }

    fn guide(touch: bool) -> Guide {
        let steps = alloc::vec![
            step("a", Page::Setting("core.language".into())),
            step("b", Page::Screen("wifi.page".into())),
        ];
        Guide::new(steps, Texts::default(), (480, 800, 220), touch)
    }

    fn two() -> Setting {
        Setting {
            key: "core.language".into(),
            title: "Language".into(),
            help: String::new(),
            unit_one: String::new(),
            unit: String::new(),
            editor: None,
            control: Control::Choice {
                options: ["en", "de"]
                    .iter()
                    .map(|v| Choice {
                        value: (*v).into(),
                        title: (*v).into(),
                    })
                    .collect(),
                at: Some(0),
            },
        }
    }

    fn laid(guide: &Guide) -> Layout {
        let sizes = Sizes {
            width: 480,
            height: 800,
            margin: 16,
            gap: 8,
            row: 56,
            row_small: 40,
            title: 24.0,
            small: 16.0,
        };
        struct Fixed;
        impl Measure for Fixed {
            fn line(&self, size: f32) -> u16 {
                (size * 1.3) as u16
            }
            fn lines(&self, _: f32, _: &str, _: u16, _: usize) -> usize {
                1
            }
            fn sample(&self, size: f32, _: u16) -> u16 {
                (size * 1.3) as u16
            }
        }
        layout(&sizes, &Fixed, &guide.content())
    }

    fn action(name: &str) -> Input<'_> {
        Input::Action {
            now_ms: 0,
            action: name,
        }
    }

    fn tap(rect: Rect) -> Input<'static> {
        Input::Gesture {
            now_ms: 0,
            position: Point::new(
                i32::from(rect.x) + i32::from(rect.width) / 2,
                i32::from(rect.y) + i32::from(rect.height) / 2,
            ),
            kind: InputGestureKind::Tap,
        }
    }

    #[test]
    fn next_is_always_right_and_back_asks_on_the_first_page() {
        let g = guide(false);
        let l = laid(&g);
        assert_eq!(g.decide(&l, &action("core.confirm")), Act::Turn(1));
        assert_eq!(g.decide(&l, &action("core.back")), Act::AskSkip);
        assert_eq!(g.decide(&l, &tap(l.other)), Act::AskSkip);
        assert_eq!(g.decide(&l, &tap(l.next)), Act::Turn(1));
    }

    #[test]
    fn later_pages_go_back_and_the_end_finishes() {
        let mut g = guide(false);
        g.page = 1;
        let l = laid(&g);
        assert_eq!(g.decide(&l, &action("core.back")), Act::Turn(0));
        assert_eq!(g.decide(&l, &tap(l.other)), Act::Turn(0));
        g.page = 2;
        assert!(g.at_end());
        assert_eq!(g.decide(&laid(&g), &action("core.confirm")), Act::Turn(3));
    }

    #[test]
    fn the_focus_walks_card_next_other_and_wraps() {
        let mut g = guide(false);
        g.setting = Some(two());
        let l = laid(&g);
        assert_eq!(g.decide(&l, &action("core.up")), Act::Focus(Target::Card));
        g.focus = Target::Card;
        assert_eq!(g.decide(&l, &action("core.down")), Act::Focus(Target::Next));
        g.focus = Target::Other;
        assert_eq!(g.decide(&l, &action("core.down")), Act::Focus(Target::Card));
    }

    #[test]
    fn with_touch_the_first_button_only_shows_the_focus() {
        let g = guide(true);
        let l = laid(&g);
        assert!(!g.shown);
        assert_eq!(g.decide(&l, &action("core.down")), Act::Focus(Target::Next));
        assert_eq!(
            g.decide(&l, &action("core.confirm")),
            Act::Focus(Target::Next)
        );
    }

    #[test]
    fn two_options_are_picked_by_select_or_by_tap_and_the_arrows_move_on() {
        let mut g = guide(false);
        g.setting = Some(two());
        g.focus = Target::Card;
        let l = laid(&g);
        let (first, second) = l.segments.unwrap();
        // Left and right leave the card (#476: on the X4 they are the only
        // arrows, picking with them kept the focus there).
        assert_eq!(
            g.decide(&l, &action("core.right")),
            Act::Focus(Target::Next)
        );
        assert_eq!(
            g.decide(&l, &action("core.left")),
            Act::Focus(Target::Other)
        );
        assert_eq!(g.decide(&l, &tap(second)), Act::Set(NewValue::Text(1)));
        assert_eq!(g.decide(&l, &tap(first)), Act::Keep);
        assert_eq!(
            g.decide(&l, &action("core.confirm")),
            Act::Set(NewValue::Text(1))
        );
    }

    #[test]
    fn a_screen_step_opens_its_screen_from_the_card() {
        let mut g = guide(false);
        g.page = 1;
        g.focus = Target::Card;
        let l = laid(&g);
        assert_eq!(g.decide(&l, &action("core.confirm")), Act::OpenScreen);
    }

    #[test]
    fn a_number_is_changed_after_select_and_back_only_ends_that() {
        let mut g = guide(false);
        g.page = 1;
        g.setting = Some(Setting {
            control: Control::Number {
                value: 12,
                min: 8,
                max: 24,
                step: 2,
            },
            ..two()
        });
        g.focus = Target::Card;
        let l = laid(&g);
        // As in Beta 3: the arrows move the focus on, Select starts changing it.
        assert_eq!(
            g.decide(&l, &action("core.right")),
            Act::Focus(Target::Next)
        );
        assert_eq!(
            g.decide(&l, &action("core.left")),
            Act::Focus(Target::Other)
        );
        assert_eq!(g.decide(&l, &action("core.confirm")), Act::Edit);
        g.editing = Some(12);
        assert_eq!(g.decide(&l, &action("core.right")), Act::Step(1));
        assert_eq!(g.decide(&l, &action("core.left")), Act::Step(-1));
        assert_eq!(g.decide(&l, &action("core.confirm")), Act::EndEdit(true));
        // Back leaves the change only, not the page as well.
        assert_eq!(g.decide(&l, &action("core.back")), Act::EndEdit(false));
        assert_eq!(g.decide(&l, &action("core.next-page")), Act::Turn(2));
        assert_eq!(g.decide(&l, &tap(l.next)), Act::EndEdit(true));
        // A finger changes it with its − and + at once.
        g.editing = None;
        let (less, more) = l.stepper.unwrap();
        assert_eq!(g.decide(&l, &tap(more)), Act::Step(1));
        assert_eq!(g.decide(&l, &tap(less)), Act::Step(-1));
    }

    #[test]
    fn a_choice_of_more_than_two_opens_its_list() {
        let mut g = guide(false);
        g.setting = Some(Setting {
            key: "core.time-zone".into(),
            control: Control::Choice {
                options: ["UTC", "Europe/Berlin", "Europe/Paris"]
                    .iter()
                    .map(|v| Choice {
                        value: (*v).into(),
                        title: (*v).into(),
                    })
                    .collect(),
                at: Some(1),
            },
            ..two()
        });
        g.focus = Target::Card;
        let l = laid(&g);
        let card = l.card.unwrap();
        assert_eq!(g.decide(&l, &action("core.confirm")), Act::OpenList);
        assert_eq!(g.decide(&l, &tap(card)), Act::OpenList);
        assert_eq!(
            g.decide(&l, &action("core.right")),
            Act::Focus(Target::Next)
        );
    }

    #[test]
    fn other_inputs_are_not_the_guides() {
        let g = guide(false);
        let l = laid(&g);
        assert_eq!(g.decide(&l, &action("core.menu")), Act::Ignore);
        assert_eq!(
            g.decide(
                &l,
                &Input::Key {
                    now_ms: 0,
                    key: "x",
                    state: paperpaw_sdk::native::callbacks::KeyState::Press
                }
            ),
            Act::Ignore
        );
    }

    /// A swipe or a long press is not the guide's, as in Beta 3: the host's
    /// gestures (the edges' panels) take it.
    #[test]
    fn gestures_but_a_tap_are_not_the_guides() {
        let g = guide(true);
        let l = laid(&g);
        for kind in [InputGestureKind::LongPress, InputGestureKind::SwipeLeft] {
            let gesture = Input::Gesture {
                now_ms: 0,
                position: Point::new(10, 10),
                kind,
            };
            assert_eq!(g.decide(&l, &gesture), Act::Ignore, "{kind:?}");
        }
    }

    /// While a number is changed, a button the guide has no use for (*Power*,
    /// the menu) passes on, as Beta 3's number editor let it.
    #[test]
    fn changing_a_number_lets_other_buttons_pass() {
        let mut g = guide(false);
        g.setting = Some(Setting {
            key: "core.font-size".into(),
            control: Control::Number {
                value: 12,
                min: 8,
                max: 99,
                step: 1,
            },
            ..two()
        });
        g.focus = Target::Card;
        g.editing = Some(12);
        let l = laid(&g);
        for other in ["core.sleep", "core.menu", "core.refresh-screen"] {
            assert_eq!(g.decide(&l, &action(other)), Act::Ignore, "{other}");
        }
        assert_eq!(g.decide(&l, &action("core.right")), Act::Step(1));
    }

    #[test]
    fn the_sample_is_the_points_scaled_to_the_screen() {
        let mut g = guide(false);
        g.setting = Some(Setting {
            key: "core.font-size".into(),
            control: Control::Number {
                value: 72,
                min: 8,
                max: 99,
                step: 1,
            },
            ..two()
        });
        assert_eq!(g.sample_size(), Some(220.0));
        g.setting.as_mut().unwrap().key = "other.size".into();
        assert_eq!(g.sample_size(), None);
    }

    #[test]
    fn the_page_read_again_keeps_its_focus_and_a_page_turned_to_starts_on_next() {
        let mut g = guide(false);
        g.setting = Some(two());
        g.focus = Target::Card;
        // Back from the list of its choice, or the language changed.
        g.commit(0, 0, (Some(two()), String::new()));
        assert_eq!(g.focus, Target::Card);
        g.commit(1, 0, (None, String::new()));
        assert_eq!(g.focus, Target::Next);
    }

    #[test]
    fn the_page_read_again_without_its_step_starts_on_next() {
        let mut g = guide(false);
        g.focus = Target::Card;
        // Its setting is gone: the next step's page shows in its place.
        g.commit(0, 1, (None, String::new()));
        assert_eq!(g.focus, Target::Next);
        assert_eq!(g.steps.len(), 1);
    }

    #[test]
    fn the_steps_take_their_titles_in_the_language_chosen() {
        let mut g = guide(false);
        g.steps[0].title = "Sprache".into();
        let mut again = step("a", Page::Setting("core.language".into()));
        again.title = "Language".into();
        g.retitle(alloc::vec![again, step("c", Page::Screen("x.page".into()))]);
        assert_eq!(g.steps[0].title, "Language");
        assert_eq!(g.steps.len(), 2);
    }
}
