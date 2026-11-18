//! The Pomodoro screen: a large countdown, the progress of the phase, the dots
//! of the set, the day's count, and the buttons Start/Pause, Skip and Reset
//! (the kit's chips). The interface-6 code was `screen.rs`.
//!
//! The view holds no host handle: the plugin (`native_plugin.rs`) does what a
//! key or a tap means ([`Act`]). Moving the focus draws only the two buttons it
//! touches, a minute's tick draws only the clock block ([`ScreenView::block`]),
//! both with partial refreshes; everything else draws the whole screen.

use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        fonts::{Font, FontRole},
        formatting::Arg,
        snapshot::{self, Snapshot},
    },
};
use paperpaw_ui_kit::{
    Chips, Messages, Page, Paint, Style, header, paint,
    widget::{Align, text::line as one_line},
};

use crate::native_shown::{Shown, count_line, week_line};
use crate::native_widgets::{bar, main_key, phase_key};

/// The screen's title (a catalog key).
pub const TITLE: &str = "pomodoro.title";

/// The buttons, in their order: the main one, Skip, Reset.
const SKIP: &str = "pomodoro.skip";
const RESET: &str = "pomodoro.reset";

/// What a button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Start, pause or go on.
    Toggle,
    /// Go on to the next phase.
    Skip,
    /// Back to the first round of a set.
    Reset,
}

/// What the plugin has to do after an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Not for the view.
    Ignored,
    /// The focus moved from one button to another: draw those two again.
    Moved(usize, usize),
    /// A button was chosen.
    Press(Press),
    /// The back arrow.
    Back,
}

/// The screen's state: which button has the focus, and where the last drawing
/// put its parts.
#[derive(Debug, Default)]
pub struct ScreenView {
    focus: usize,
    header: Page,
    chips: Option<Chips>,
    /// The phase's name, the clock and the bar, as the last drawing put them.
    block: Option<Rect>,
}

/// `value` seconds as "MM:SS".
fn clock_text(secs: u64) -> String {
    let (minutes, seconds) = (secs / 60, secs % 60);
    let mut out = String::new();
    for part in [minutes, seconds] {
        if part < 10 {
            out.push('0');
        }
        push_number(&mut out, part);
        out.push(':');
    }
    out.pop();
    out
}

fn push_number(out: &mut String, mut value: u64) {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + u8::try_from(value % 10).unwrap_or(0);
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for digit in &digits[at..] {
        out.push(char::from(*digit));
    }
}

impl ScreenView {
    /// The part of the screen a minute's tick draws again: the phase's name, the
    /// clock and the bar. `None` before the first drawing.
    #[must_use]
    pub fn block(&self) -> Option<Rect> {
        self.block
    }

    /// What the buttons drawn for `from` and `to` cover, with the frame a focus
    /// draws round them.
    #[must_use]
    pub fn reach(&self, from: usize, to: usize) -> Option<Rect> {
        let chips = self.chips.as_ref()?;
        Some(chips.reach(from)?.union(&chips.reach(to)?))
    }

    /// A key or a tap.
    pub fn input(&mut self, input: &Input<'_>) -> Act {
        match *input {
            Input::Action { action, .. } => {
                if action == "core.confirm" {
                    return Act::Press(self.press(self.focus));
                }
                let action = match action {
                    "core.up" => "core.previous",
                    "core.down" => "core.next",
                    other => other,
                };
                let to = self
                    .chips
                    .as_ref()
                    .and_then(|chips| chips.step(self.focus, action));
                match to {
                    Some(to) => {
                        let from = core::mem::replace(&mut self.focus, to);
                        if from == to {
                            Act::Ignored
                        } else {
                            Act::Moved(from, to)
                        }
                    }
                    None => Act::Ignored,
                }
            }
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => {
                if self.header.action(position) == Some("core.back") {
                    return Act::Back;
                }
                match self.chips.as_ref().and_then(|chips| chips.hit(position)) {
                    Some(at) => {
                        self.focus = at;
                        Act::Press(self.press(at))
                    }
                    None => Act::Ignored,
                }
            }
            _ => Act::Ignored,
        }
    }

    fn press(&self, at: usize) -> Press {
        match at {
            1 => Press::Skip,
            2 => Press::Reset,
            _ => Press::Toggle,
        }
    }

    /// Draws the header and the timer.
    ///
    /// # Errors
    /// `NoMemory` or what the host says.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        shown: &Shown,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let result = self.paint(host, frame, &display, &font, shown);
        font.release()?;
        result
    }

    fn paint(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        display: &Snapshot,
        font: &Font<'_>,
        shown: &Shown,
    ) -> Result<(), Error> {
        let mut style = Style::read(host, display, font)?;
        style.describing = paint::asking(frame);
        let (width, height) = (display.display_width(), display.display_height());
        let mut messages = Messages::new()?;
        let mut page = Page::default();
        let title = messages.copy(host, TITLE)?;
        header::prepare(
            &mut page,
            host,
            font,
            &style,
            &mut messages,
            &title,
            width,
            display.has_touch(),
            0,
            1,
        )?;
        let top = style.header.saturating_add(style.gap);
        let margin = style.margin.max(style.gap);
        let body = Rect::new(
            margin,
            top,
            width.saturating_sub(margin.saturating_mul(2)),
            height.saturating_sub(top).saturating_sub(margin),
        );
        let mut ops: Vec<Paint> = Vec::new();
        paint::push(
            &mut ops,
            Paint::Fill(
                Rect::new(0, top, width, height.saturating_sub(top)),
                style.paper,
            ),
        )?;
        let chips = buttons(host, (font, &style, display), &mut messages, body, shown)?;
        self.focus = self.focus.min(BUTTONS - 1);
        chips.prepare(&mut ops, font, 0, Some(self.focus))?;
        let above = chips
            .cell(0)
            .map_or(body.height, |cell| cell.y.saturating_sub(top));
        let room = above.saturating_sub(style.gap);
        let block = timer_block(
            &mut ops,
            (host, font, &style),
            &mut messages,
            (shown, Rect::new(body.x, top, body.width, room)),
        )?;
        paint::replay(frame, font, page.paint.iter().chain(ops.iter()))?;
        page.paint = Vec::new();
        self.header = page;
        self.chips = Some(chips);
        self.block = Some(block);
        Ok(())
    }
}

/// The number of buttons.
const BUTTONS: usize = 3;

/// The buttons at the bottom of `body`, in a row in the middle.
fn buttons(
    host: &Host<'_>,
    (font, style, display): (&Font<'_>, &Style, &Snapshot),
    messages: &mut Messages,
    body: Rect,
    shown: &Shown,
) -> Result<Chips, Error> {
    let main = messages.copy(host, main_key(shown))?;
    let skip = messages.copy(host, SKIP)?;
    let reset = messages.copy(host, RESET)?;
    let labels = [main.as_str(), skip.as_str(), reset.as_str()];
    let finger = Chips::finger(display);
    let height = Chips::height_for(font, style, finger, body.width, &labels)?;
    let at = Rect::new(
        body.x,
        body.y.saturating_add(body.height).saturating_sub(height),
        body.width,
        height,
    );
    let chips = Chips::new(font, style, finger, at, &labels)?;
    if let (Some(first), Some(last)) = (chips.cell(0), chips.cell(BUTTONS - 1)) {
        let used = last.x.saturating_add(last.width).saturating_sub(first.x);
        if used < at.width && last.y == first.y {
            let shifted = Rect::new(at.x + (at.width - used) / 2, at.y, used, at.height);
            let mut centred = Chips::new(font, style, finger, shifted, &labels)?;
            centred.set_describing(style.describing);
            return Ok(centred);
        }
    }
    let mut chips = chips;
    chips.set_describing(style.describing);
    Ok(chips)
}

/// The timer above the buttons in `room`: what fits, most wanted first (the
/// phase's name, the clock, the bar; then the dots, the day's count, the week).
/// Returns the block a minute's tick draws again.
fn timer_block(
    ops: &mut Vec<Paint>,
    (host, font, style): (&Host<'_>, &Font<'_>, &Style),
    messages: &mut Messages,
    (shown, room): (&Shown, Rect),
) -> Result<Rect, Error> {
    let (row, gap) = (style.row, style.gap);
    let count = count_line(shown, |key, args: &[Arg<'_>]| {
        messages.format(host, key, args)
    })?;
    let week = week_line(shown, |key, args: &[Arg<'_>]| {
        messages.format(host, key, args)
    })?;
    let bar_h = (row / 2).max(8);
    let mut optional = [true, true, week.is_some()];
    let fixed = row + bar_h + gap * 2;
    let extra = |optional: &[bool; 3]| -> u16 {
        let n: u16 = optional.iter().map(|on| u16::from(*on)).sum();
        n.saturating_mul(row + gap)
    };
    // The dots, the count and the week go, the last first, while the clock
    // would have less than two rows.
    for slot in [2, 1, 0] {
        if room.height < fixed + extra(&optional) + row.saturating_mul(2) {
            optional[slot] = false;
        }
    }
    let clock_h = room
        .height
        .saturating_sub(fixed + extra(&optional))
        .max(row);
    let mut y = room.y;
    let mut band = |h: u16| {
        let rect = Rect::new(room.x, y, room.width, h);
        y = y.saturating_add(h).saturating_add(gap);
        rect
    };
    let phase_name = messages.copy(host, phase_key(shown.phase, false))?;
    let phase_band = band(row);
    let (text, small) = (style.text, style.small);
    let centre = Align::Center;
    one_line(
        ops,
        font,
        phase_band,
        centre,
        (text, small),
        &phase_name,
        style.ink,
    )?;
    let clock_band = band(clock_h);
    one_line(
        ops,
        font,
        clock_band,
        centre,
        (f32::from(clock_h) * 1.1, style.title),
        &clock_text(shown.secs),
        style.ink,
    )?;
    let bar_band = band(bar_h);
    bar(ops, bar_band, shown.gone, style.ink)?;
    if optional[0] {
        dots_row(ops, band(row), shown, style.ink)?;
    }
    if optional[1] {
        one_line(
            ops,
            font,
            band(row),
            centre,
            (small, small),
            &count,
            style.muted,
        )?;
    }
    if let (true, Some(week)) = (optional[2], week.as_deref()) {
        one_line(
            ops,
            font,
            band(row),
            centre,
            (small, small),
            week,
            style.muted,
        )?;
    }
    Ok(phase_band.union(&clock_band).union(&bar_band).grow(4))
}

/// The dots of the set, `done` of `rounds` filled, in the middle of `area`.
fn dots_row(
    out: &mut Vec<Paint>,
    area: Rect,
    shown: &Shown,
    ink: paperpaw_sdk::Color,
) -> Result<(), Error> {
    crate::native_widgets::dots(out, area, (shown.rounds, shown.done), ink)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_has_two_digits_each() {
        assert_eq!(clock_text(25 * 60), "25:00");
        assert_eq!(clock_text(9 * 60 + 5), "09:05");
        assert_eq!(clock_text(0), "00:00");
        assert_eq!(clock_text(90 * 60), "90:00");
    }
}
