//! Sliders: numbers better set by eye than by their value, such as the
//! line spacing or a light's brightness. A track filled up to the value,
//! with a knob there, between a `−` and a `+`; its title and value (with
//! its unit) on the line above, as in other rows.
//!
//! Equally easy by touch and with buttons alone:
//!
//! - Touch: the slider's band is as high as a fingertip needs
//!   ([`UiStyle::finger`]) and as wide as the screen. A finger that comes
//!   down anywhere on it and moves sideways moves the knob as far as it
//!   moves itself ([`SliderDrag`]), so it never jumps where the finger
//!   lands and the finger need not cover the knob; the value follows in
//!   the slider's steps, and lifting the finger chooses it. A tap on the
//!   track chooses the value there, a tap on `−` or `+` (or beside them)
//!   moves it one step.
//! - Buttons: on a focused slider, *confirm* starts moving it; left and
//!   right (or up and down, or the page buttons) move it one step each
//!   ([`key_step`]), *confirm* chooses the value, *back* goes back to the
//!   one before. The `−` and `+` are filled while the buttons move it.
//!
//! While a value moves it is best shown without being kept
//! ([`Command::PreviewSettings`](crate::Command::PreviewSettings) for a
//! setting), so that what it changes shows at once; it is kept when
//! chosen.
//!
//! Sliders are for touch screens, where a finger sets a value by eye;
//! with buttons alone a number is changed a step at a time all the same,
//! and a compact row with `−` and `+` serves it better ([`shows`]).
//!
//! A screen keeps a [`Slider`] for each (from a setting with
//! [`Slider::of`]), draws rows of [`row_height`] with
//! [`Slider::draw_row`] (what it shows in a [`SliderView`]), or only the
//! track with [`Slider::draw_track`] where it places the buttons itself,
//! and passes pointer samples to [`SliderDrag::on_pointer`] from
//! [`Screen::on_pointer`](crate::Screen::on_pointer). While the knob
//! moves, the slider alone is best drawn again with a quick refresh
//! ([`RefreshMode::Fast`](crate::RefreshMode::Fast)), and the screen
//! with a clean one when the finger is lifted.

use crate::builtin::actions;
use crate::id::Id;
use crate::semantics::Role;
use crate::settings::{SettingKind, SettingSpec};
use crate::ui::{
    UiStyle, button_height, draw_button, draw_focused_steps, draw_note_below, draw_value_row,
    title_area,
};
use crate::{Canvas, Color, Env, PointerEvent, PointerPhase, Rect};

/// A number from `min` to `max` in steps of `step`, set with a slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slider {
    min: i64,
    /// At least `min`.
    max: i64,
    /// At least 1.
    step: i64,
}

/// How a slider shows the focus (see [`Slider::draw_row`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SliderFocus {
    /// Not focused: black on white.
    #[default]
    None,
    /// Focused, on the focus bar ([`Ui::focus`](crate::ui::Ui::focus)):
    /// white on black.
    Focused,
    /// Focused and moved by the buttons: its `−` and `+` filled.
    Editing,
}

/// What a slider shows and where (see [`Slider::draw_row`] and
/// [`Slider::draw_band`]): made with [`SliderView::new`] and the builder
/// methods, so that it can grow without breaking plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SliderView<'a> {
    /// The row (for [`Slider::draw_row`]) or the band
    /// ([`Slider::draw_band`]).
    pub area: Rect,
    pub value: i64,
    /// The value as shown, with its unit (e.g. "135 %").
    pub shown: &'a str,
    /// The title on the row's first line; empty for none.
    pub title: &'a str,
    /// A short help text below it; empty for none.
    pub note: &'a str,
    pub focus: SliderFocus,
    /// The widest value it shows, which its title keeps clear of (as its
    /// row was laid out with: [`row_height_for`]); empty: `shown`.
    pub room: &'a str,
}

impl<'a> SliderView<'a> {
    /// `value` in `area`, shown as `shown`; no title or note, not
    /// focused.
    pub fn new(area: Rect, value: i64, shown: &'a str) -> Self {
        Self {
            area,
            value,
            shown,
            title: "",
            note: "",
            focus: SliderFocus::None,
            room: "",
        }
    }

    pub fn room(self, room: &'a str) -> Self {
        Self { room, ..self }
    }

    pub fn title(self, title: &'a str) -> Self {
        Self { title, ..self }
    }

    pub fn note(self, note: &'a str) -> Self {
        Self { note, ..self }
    }

    pub fn focus(self, focus: SliderFocus) -> Self {
        Self { focus, ..self }
    }
}

/// Where the parts of a slider are in its band (see [`parts`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SliderParts {
    pub minus: Rect,
    pub track: Rect,
    pub plus: Rect,
}

impl Slider {
    /// From `min` to `max` (no less than `min`) in steps of 1.
    pub const fn new(min: i64, max: i64) -> Self {
        Self {
            min,
            max: if max < min { min } else { max },
            step: 1,
        }
    }

    /// In steps of `step` (at least 1) from `min`.
    pub const fn step(self, step: i64) -> Self {
        Self {
            step: if step < 1 { 1 } else { step },
            ..self
        }
    }

    /// The lowest value.
    pub fn min(&self) -> i64 {
        self.min
    }

    /// The highest value, at least [`Slider::min`].
    pub fn max(&self) -> i64 {
        self.max
    }

    /// How far a step goes, at least 1.
    pub fn step_size(&self) -> i64 {
        self.step
    }

    /// The slider of a number setting: its range and step; `None` for
    /// settings of other kinds.
    pub fn of(spec: &SettingSpec) -> Option<Self> {
        match spec.kind {
            SettingKind::Int { min, max } => Some(Self::new(min, max).step(spec.extra().step)),
            _ => None,
        }
    }

    /// `value` within the range (a `max` below `min`, set through the
    /// fields, counts as `min`).
    pub fn clamp(&self, value: i64) -> i64 {
        value.clamp(self.min, self.max.max(self.min))
    }

    /// `value` moved by `steps` steps, within the range. From `max`
    /// where the range is no whole number of steps, the first step down
    /// goes to the last whole step (from 12 in fives to 10), as a drag
    /// gives; elsewhere a step is `step` (as many back return to the
    /// value).
    pub fn stepped(&self, value: i64, steps: i64) -> i64 {
        let (min, max) = (i128::from(self.min), i128::from(self.max.max(self.min)));
        let step = i128::from(self.step.max(1));
        let value = i128::from(self.clamp(value));
        let past = (max - min) % step;
        let target = if steps < 0 && value == max && past != 0 {
            max - past + (i128::from(steps) + 1) * step
        } else {
            value + i128::from(steps) * step
        };
        // Within the range, so within `i64`.
        target.clamp(min, max) as i64
    }

    /// The width of the range, without overflow at the widest.
    fn range(&self) -> f64 {
        (i128::from(self.max) - i128::from(self.min)).max(0) as f64
    }

    /// How far along the track `value` is, from 0 to 1.
    pub fn share(&self, value: i64) -> f32 {
        let range = self.range().max(1.0);
        let along = (i128::from(self.clamp(value)) - i128::from(self.min)) as f64;
        (along / range).clamp(0.0, 1.0) as f32
    }

    /// The value for a finger at `x` on `track` (where the knob's middle
    /// would be): the nearest step, the ends past the track's ends.
    pub fn value_at(&self, track: Rect, x: i32) -> i64 {
        let knob = i32::from(knob_size(track));
        let left = i32::from(track.x) + knob / 2;
        let width = (i32::from(track.width) - knob).max(1);
        let share = ((x - left) as f32 / width as f32).clamp(0.0, 1.0);
        if share >= 1.0 {
            return self.clamp(self.max);
        }
        let step = self.step.max(1);
        let steps = (self.range() * f64::from(share) / step as f64 + 0.5) as i128;
        let value = i128::from(self.min) + steps * i128::from(step);
        // At most `max`, so within `i64`.
        self.clamp(value.min(i128::from(self.max)) as i64)
    }

    /// Where the middle of the knob is on `track` at `value`, as drawn
    /// ([`Slider::draw_track`]); [`Slider::value_at`] there is `value`.
    pub fn knob_x(&self, track: Rect, value: i64) -> i32 {
        let knob = knob_size(track);
        let width = track.width.saturating_sub(knob);
        let filled = ((f32::from(width) * self.share(value)) + 0.5) as u16;
        i32::from(track.x + knob / 2 + filled)
    }

    /// Draws the track in `track`: a thick line in `ink` up to `value`, a
    /// thin one after it, and a large knob at the value (round where the
    /// interface has round corners). For screens that place a slider's
    /// buttons themselves; [`Slider::draw_row`] draws all of it.
    pub fn draw_track(
        &self,
        canvas: &mut Canvas<'_>,
        style: &UiStyle,
        track: Rect,
        value: i64,
        ink: Color,
    ) {
        let part = super::look::SliderPart {
            track,
            knob_x: self.knob_x(track, value),
            ink,
        };
        if style.look.slider(canvas, style, part) {
            return;
        }
        let knob = knob_size(track);
        // Half of the filled part's thickness: an eighth of the knob, at
        // least the interface's lines.
        let line = (knob / 8).max(style.line.max(1) as u16);
        let mid = track.y + track.height / 2;
        let left = track.x + knob / 2;
        let width = track.width.saturating_sub(knob);
        let filled = (self.knob_x(track, value) - i32::from(left)) as u16;
        // After the value a thin line: in ink, not gray, so that it stays
        // in the quick refreshes that show a knob moving, which leave out
        // grays.
        let thin = line.div_ceil(2);
        canvas.fill(
            Rect::new(
                left + filled,
                mid.saturating_sub(thin / 2),
                width - filled,
                thin,
            ),
            ink,
        );
        canvas.fill(
            Rect::new(left, mid.saturating_sub(line), filled, line * 2),
            ink,
        );
        let (x, y) = (
            i32::from(left + filled) - i32::from(knob / 2),
            i32::from(mid) - i32::from(knob / 2),
        );
        let size = u32::from(knob);
        if style.corner > 0 {
            canvas.fill_round_rect(x, y, size, size, size / 2, ink);
        } else {
            canvas.fill_rect(x, y, size, size, ink);
        }
    }

    /// Draws a slider's band, `view.area` (see [`band`]): `−`, the track
    /// at the value and `+`, as its focus says; the value as shown tells
    /// screen readers what it is set to.
    pub fn draw_band(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        style: &UiStyle,
        view: &SliderView<'_>,
    ) {
        let band = view.area;
        let focus = view.focus;
        let SliderParts { minus, track, plus } = parts(style, band);
        let ink = match focus {
            SliderFocus::None => {
                draw_button(canvas, env, style, minus, "−", false, false);
                draw_button(canvas, env, style, plus, "+", false, false);
                Color::BLACK
            }
            SliderFocus::Focused | SliderFocus::Editing => {
                if let Some(font) = style.font(env) {
                    let editing = focus == SliderFocus::Editing;
                    draw_focused_steps(canvas, style, &font, band, (minus, plus), editing);
                }
                Color::WHITE
            }
        };
        canvas
            .describe(Role::Slider, view.shown, track)
            .focused(focus != SliderFocus::None);
        self.draw_track(canvas, style, track, view.value, ink);
    }

    /// Draws a row of [`row_height`] with a slider, `view.area`: the
    /// title and the value on its first line, as other rows have them,
    /// the slider's band below, and the note below that (if the view has
    /// one; the row is then as high as `row_height(style, true)` says). A
    /// focused row is drawn white, for the focus bar below it.
    pub fn draw_row(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        style: &UiStyle,
        view: &SliderView<'_>,
    ) {
        let row = view.area;
        let focused = view.focus != SliderFocus::None;
        // A long title on more lines, the band below them: never cut.
        let room = if view.room.is_empty() {
            view.shown
        } else {
            view.room
        };
        let extra = title_extra(env, style, row.width, view.title, room);
        let title = title_area(style, row);
        let title = Rect::new(title.x, title.y, title.width, title.height + extra);
        draw_value_row(canvas, env, style, title, focused, view.title, view.shown);
        let band = band_below(style, row, extra);
        let band_view = SliderView {
            area: band,
            ..*view
        };
        self.draw_band(canvas, env, style, &band_view);
        if !view.note.is_empty() {
            let below = band.y + band.height;
            draw_note_below(canvas, env, style, row, below, focused, view.note);
        }
    }
}

/// Whether `spec` is shown as a slider here: a number marked so
/// ([`SettingExtra::slider`](crate::SettingExtra::slider)), on a touch
/// screen. With buttons alone it keeps its compact row with `−` and `+`.
pub fn shows(env: &Env<'_>, spec: &SettingSpec) -> bool {
    env.device().input.touch && marked(spec)
}

/// Whether `spec` is a number marked to be set with a slider, wherever
/// one is shown ([`shows`]).
pub fn marked(spec: &SettingSpec) -> bool {
    matches!(spec.kind, SettingKind::Int { .. }) && spec.extra().slider
}

/// The size of the knob on `track`: three quarters of its height, so
/// that it shows beside the finger that moves it.
fn knob_size(track: Rect) -> u16 {
    (track.height * 3 / 4).max(4).min(track.width.max(1))
}

/// The height of a slider's band: as the first line of a row, and on
/// touch screens as high as its `−` and `+` need to be a fingertip
/// large ([`UiStyle::touch_row`]).
pub fn band_height(style: &UiStyle) -> u16 {
    style.touch_row(style.title_band()) as u16
}

/// The height of a row with a slider (see [`Slider::draw_row`]), with
/// room for a note if `noted`.
pub fn row_height(style: &UiStyle, noted: bool) -> u16 {
    let band = u32::from(band_height(style));
    let height = if noted {
        style.noted_row() + band
    } else {
        style.title_band() + band + style.gap * 3 / 2
    };
    height as u16
}

/// How much higher than [`row_height`] a slider's row is where its
/// `title`, beside its value (as wide as `room`, its widest), takes more
/// than its line: the band goes as much lower ([`band_below`]).
pub fn title_extra(env: &Env<'_>, style: &UiStyle, width: u16, title: &str, room: &str) -> u16 {
    let Some(font) = style.font(env) else {
        return 0;
    };
    let lines = super::setting_rows::ValueLines::new(&font, style, width, title, room);
    let band = title_area(style, Rect::new(0, 0, width, row_height(style, false))).height;
    lines.height().saturating_sub(band)
}

/// [`row_height`] for a slider titled `title` beside its value (as wide
/// as `room`, its widest), in a row `width` wide: higher by
/// [`title_extra`] where the title needs more than its line.
pub fn row_height_for(
    env: &Env<'_>,
    style: &UiStyle,
    width: u16,
    title: &str,
    room: &str,
    noted: bool,
) -> u16 {
    row_height(style, noted) + title_extra(env, style, width, title, room)
}

/// The slider's band in a row whose title takes `extra` more than its
/// line ([`title_extra`]).
pub fn band_below(style: &UiStyle, row: Rect, extra: u16) -> Rect {
    let moved = Rect::new(
        row.x,
        row.y + extra,
        row.width,
        row.height.saturating_sub(extra),
    );
    band(style, moved)
}

/// The slider's band in a row of [`row_height`]: below its first line.
pub fn band(style: &UiStyle, row: Rect) -> Rect {
    let title = title_area(style, row);
    let top = title.y + title.height;
    let height = band_height(style).min(row.bottom().saturating_sub(u32::from(top)) as u16);
    Rect::new(row.x, top, row.width, height)
}

/// Where `−`, the track and `+` are in `band`: the buttons as large as
/// other rows' and flush with the margins (where titles start and values
/// end), the track between them.
pub fn parts(style: &UiStyle, band: Rect) -> SliderParts {
    let size = button_height(style, band);
    let y = band.y + (band.height - size) / 2;
    let margin = style.margin as u16;
    let gap = style.gap as u16;
    let minus = Rect::new(band.x + margin, y, size, size);
    let plus = Rect::new(
        (band.x + band.width).saturating_sub(margin + size),
        y,
        size,
        size,
    );
    let start = minus.x + size + gap;
    let track = Rect::new(start, y, plus.x.saturating_sub(start + gap), size);
    SliderParts { minus, track, plus }
}

/// How many steps a button moves a slider (or a number) being changed
/// with the buttons: one down for left, down and the buttons back, one
/// up for right, up and the buttons forward; `None` for other actions.
pub fn key_step(action: &Id) -> Option<i64> {
    let down = [
        actions::LEFT,
        actions::DOWN,
        actions::PREV_PAGE,
        actions::PREVIOUS,
    ];
    let up = [
        actions::RIGHT,
        actions::UP,
        actions::NEXT_PAGE,
        actions::NEXT,
    ];
    if down.contains(action) {
        Some(-1)
    } else if up.contains(action) {
        Some(1)
    } else {
        None
    }
}

/// A knob being dragged (see the [module documentation](self)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SliderDrag {
    drag: Option<Drag>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drag {
    pointer: u8,
    /// Where the finger came down.
    start: i32,
    /// The value then.
    from: i64,
    /// The value now.
    value: i64,
    /// It moved further than a tap would.
    moved: bool,
}

/// What a pointer sample did to a slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SliderStep {
    /// The knob was moved to this value: show it, not kept yet.
    Moved(i64),
    /// The finger came down, or moved within the same step: nothing to
    /// draw.
    Same,
    /// The finger was lifted: keep this value (it may be the one before,
    /// e.g. after a tap beside the slider).
    Chosen(i64),
    /// The finger was taken away (e.g. by the system): show the value
    /// kept again.
    Cancelled,
}

impl SliderDrag {
    /// The value the knob is dragged to, while a finger holds it.
    pub fn dragging(&self) -> Option<i64> {
        self.drag.map(|d| d.value)
    }

    /// Follows a finger on `slider`, set to `value` and drawn in `band`
    /// (its `−`, track and `+` where [`parts`] puts them): one that comes
    /// down anywhere in `band` is taken. Moved sideways, the knob moves
    /// as far from `value` as the finger from where it came down, in the
    /// slider's steps and within its range; a tap chooses the value on
    /// the track where it lands, or a step down left of the track and up
    /// right of it. `None` for samples it does not take: the screen then
    /// handles them as it would.
    pub fn on_pointer(
        &mut self,
        event: &PointerEvent,
        style: &UiStyle,
        slider: &Slider,
        band: Rect,
        value: i64,
    ) -> Option<SliderStep> {
        let track = parts(style, band).track;
        // Further than this, a finger moves the knob: as far as a tap may
        // wander (about 3 mm).
        let slop = (style.finger * 3 / 7).max(style.gap).max(1) as i32;
        // Where the finger came down, the value it had: with steps finer
        // than pixels, the knob's pixel holds several values, and the one
        // there need not be this one.
        let dragged = |drag: &Drag, x: i32| match x - drag.start {
            0 => drag.from,
            moved => slider.value_at(track, slider.knob_x(track, drag.from) + moved),
        };
        match (event.phase, self.drag) {
            // The same finger coming down again starts afresh: the end of
            // its drag got lost, which would otherwise hold the knob for
            // good. Another finger meanwhile is not taken.
            (PointerPhase::Down, held) if held.is_none_or(|d| d.pointer == event.id) => {
                if !band.contains(event.point) {
                    self.drag = None;
                    return None;
                }
                let from = slider.clamp(value);
                self.drag = Some(Drag {
                    pointer: event.id,
                    start: event.point.x,
                    from,
                    value: from,
                    moved: false,
                });
                Some(SliderStep::Same)
            }
            (PointerPhase::Move, Some(drag)) if drag.pointer == event.id => {
                if !drag.moved && (event.point.x - drag.start).abs() < slop {
                    return Some(SliderStep::Same);
                }
                let value = dragged(&drag, event.point.x);
                self.drag = Some(Drag {
                    value,
                    moved: true,
                    ..drag
                });
                Some(if value == drag.value {
                    SliderStep::Same
                } else {
                    SliderStep::Moved(value)
                })
            }
            (PointerPhase::Up, Some(drag)) if drag.pointer == event.id => {
                self.drag = None;
                let x = event.point.x;
                let value = if drag.moved {
                    dragged(&drag, x)
                } else if !band.contains(event.point) {
                    drag.from
                } else if x < i32::from(track.x) {
                    slider.stepped(drag.from, -1)
                } else if x >= track.right() as i32 {
                    slider.stepped(drag.from, 1)
                } else {
                    slider.value_at(track, x)
                };
                Some(SliderStep::Chosen(value))
            }
            (PointerPhase::Cancel, Some(drag)) if drag.pointer == event.id => {
                self.drag = None;
                Some(SliderStep::Cancelled)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point, PointerTool};

    use paperpaw_graphics::Framebuffer;
    use paperpaw_platform::PixelFormat;

    const STYLE: UiStyle = crate::ui::tests::STYLE;

    /// On a touch screen of 220 dpi: a fingertip 7 mm large.
    const TOUCH: UiStyle = UiStyle {
        finger: 61,
        ..STYLE
    };

    fn pointer(phase: PointerPhase, x: i32, y: i32) -> PointerEvent {
        PointerEvent::new(0, phase, Point::new(x, y), PointerTool::Finger, 1000)
    }

    #[test]
    fn a_tap_sets_the_value_where_it_lands() {
        // Knob 22 wide: the track runs from 111 to 189.
        let slider = Slider::new(0, 255);
        let track = Rect::new(100, 0, 100, 30);
        assert_eq!(slider.value_at(track, 50), 0);
        assert_eq!(slider.value_at(track, 111), 0);
        assert_eq!(slider.value_at(track, 150), 128);
        assert_eq!(slider.value_at(track, 189), 255);
        assert_eq!(slider.value_at(track, 400), 255);
        // Where the knob is drawn, the value is read back.
        let fives = Slider::new(0, 100).step(5);
        let wide = Rect::new(80, 0, 320, 60);
        for value in (0..=100).step_by(5) {
            assert_eq!(fives.value_at(wide, fives.knob_x(wide, value)), value);
        }
        assert_eq!(fives.knob_x(wide, 0), 80 + 22);
        assert_eq!(fives.knob_x(wide, 100), 400 - 23);
    }

    #[test]
    fn values_keep_to_the_steps_and_the_range() {
        // 100 to 250 in fives: 31 places.
        let slider = Slider::new(100, 250).step(5);
        let track = Rect::new(0, 0, 330, 30);
        for x in (0..400).step_by(7) {
            let value = slider.value_at(track, x);
            assert!((100..=250).contains(&value), "{value}");
            assert_eq!((value - 100) % 5, 0, "{value}");
        }
        // The end is reached even where the range is not whole steps.
        let odd = Slider::new(0, 12).step(5);
        assert_eq!(odd.value_at(track, 400), 12);
        assert_eq!(odd.stepped(10, 1), 12);
        assert_eq!(odd.stepped(0, -1), 0);
        assert_eq!(slider.stepped(135, 1), 140);
        assert_eq!(slider.stepped(135, -100), 100);
        // From the end that is no whole step, down to the last whole
        // step: 12 to 10, as a drag gives.
        assert_eq!(odd.stepped(12, -1), 10);
        assert_eq!(odd.stepped(12, -2), 5);
        assert_eq!(odd.stepped(5, -1), 0);
        // Elsewhere a step is a step, so as many back return.
        assert_eq!(odd.stepped(7, 1), 12);
        assert_eq!(odd.stepped(7, -1), 2);
        // Made right from wrong numbers.
        assert_eq!(Slider::new(5, 1), Slider::new(5, 5));
        // Fields set past what the constructors allow do not bring it
        // down.
        let wrong = Slider {
            min: 10,
            max: 0,
            step: 0,
        };
        assert_eq!(wrong.clamp(-3), 10);
        assert_eq!(wrong.stepped(4, 3), 10);
        assert_eq!(wrong.value_at(track, 200), 10);
        assert_eq!(wrong.share(4), 0.0);
        // The widest range neither overflows nor leaves it.
        let widest = Slider::new(i64::MIN, i64::MAX);
        assert_eq!(widest.stepped(i64::MAX, 1), i64::MAX);
        assert_eq!(widest.stepped(i64::MIN, -1), i64::MIN);
        assert_eq!(widest.value_at(track, 400), i64::MAX);
        assert_eq!(widest.value_at(track, -10), i64::MIN);
        assert!((widest.share(0) - 0.5).abs() < 0.01);
        assert_eq!(Slider::new(0, 9).step(0).step, 1);
        assert_eq!(Slider::new(3, 3).share(3), 0.0);
    }

    #[test]
    fn settings_give_their_range_and_step() {
        static STEP: crate::SettingExtra = crate::SettingExtra::new().step(5);
        static SLIDER: crate::SettingExtra = crate::SettingExtra::new().step(5).slider();
        static ON_SLIDER: crate::SettingExtra = crate::SettingExtra::new().slider();
        let spec = crate::SettingSpec::number(
            "a.spacing",
            "a.spacing",
            100,
            250,
            135,
            crate::builtin::groups::OTHER,
        )
        .with_extra(&STEP);
        assert_eq!(Slider::of(&spec), Some(Slider::new(100, 250).step(5)));
        assert!(!marked(&spec));
        assert!(marked(&spec.clone().with_extra(&SLIDER)));
        let switch =
            crate::SettingSpec::switch("a.on", "a.on", true, crate::builtin::groups::OTHER);
        assert_eq!(Slider::of(&switch), None);
        assert!(!marked(&switch.with_extra(&ON_SLIDER)));
    }

    #[test]
    fn its_parts_keep_to_the_margins() {
        let row = Rect::new(0, 100, 480, row_height(&STYLE, true));
        let band = band(&STYLE, row);
        assert_eq!(band.y, title_area(&STYLE, row).bottom() as u16);
        assert!(band.bottom() <= row.bottom());
        let SliderParts { minus, track, plus } = parts(&STYLE, band);
        // Flush with the margins, where titles start and values end.
        assert_eq!(minus.x, 12);
        assert_eq!(plus.right(), 480 - 12);
        assert_eq!(minus.height, button_height(&STYLE, band));
        assert_eq!(track.x, minus.right() as u16 + 8);
        assert_eq!(track.right() + 8, u32::from(plus.x));
        // A row with a note is higher by the note's lines.
        assert!(row_height(&STYLE, true) > row_height(&STYLE, false));
        // By touch, − and + are a fingertip large, the band higher by as
        // much: the room above and below them stays as it was.
        let row = Rect::new(0, 100, 480, row_height(&TOUCH, false));
        let band = super::band(&TOUCH, row);
        let base = TOUCH.title_band();
        assert_eq!(band.height, 75);
        assert_eq!(
            (band.height - 61) / 2,
            ((base - button_height(&STYLE, Rect::new(0, 0, 0, base as u16)) as u32) / 2) as u16
        );
        let SliderParts { minus, track, plus } = parts(&TOUCH, band);
        assert_eq!((minus.height, plus.height, track.height), (61, 61, 61));
        assert!(band.bottom() <= row.bottom());
        assert!(row_height(&TOUCH, false) > row_height(&STYLE, false));
    }

    /// A slider from 0 to 100 in fives across a touch screen 480 wide:
    /// its band at 100, 87 high; − from 12 to 72, the track from 80 to
    /// 400 (the knob's middle from 102 to 377), + from 408 to 468.
    fn across() -> (Slider, Rect) {
        (Slider::new(0, 100).step(5), Rect::new(0, 100, 480, 87))
    }

    fn sample(drag: &mut SliderDrag, phase: PointerPhase, x: i32, y: i32) -> Option<SliderStep> {
        let (slider, band) = across();
        drag.on_pointer(&pointer(phase, x, y), &TOUCH, &slider, band, 50)
    }

    #[test]
    fn a_finger_moves_the_knob_as_far_as_it_moves() {
        use PointerPhase::{Down, Move, Up};
        use SliderStep::{Chosen, Moved, Same};
        let mut drag = SliderDrag::default();
        // Not on the band: left to the screen.
        assert_eq!(sample(&mut drag, Down, 300, 90), None);
        // Down far from the knob (at 240): it stays where it is.
        assert_eq!(sample(&mut drag, Down, 300, 140), Some(Same));
        assert_eq!(drag.dragging(), Some(50));
        // Less than a tap may wander: nothing yet.
        assert_eq!(sample(&mut drag, Move, 320, 180), Some(Same));
        // 28 to the right: the knob 28 further, two steps.
        assert_eq!(sample(&mut drag, Move, 328, 180), Some(Moved(60)));
        assert_eq!(sample(&mut drag, Move, 330, 120), Some(Same));
        // Back near where it came down: the value it had, though closer
        // than a tap may wander.
        assert_eq!(sample(&mut drag, Move, 302, 120), Some(Moved(50)));
        // Far past either end: no further than the range.
        assert_eq!(sample(&mut drag, Move, 2000, 120), Some(Moved(100)));
        assert_eq!(sample(&mut drag, Move, -900, 120), Some(Moved(0)));
        assert_eq!(drag.dragging(), Some(0));
        // Lifted past the band, where the knob is now: chosen.
        assert_eq!(sample(&mut drag, Up, -900, 400), Some(Chosen(0)));
        assert_eq!(drag.dragging(), None);
        assert_eq!(sample(&mut drag, Move, 300, 140), None);
    }

    #[test]
    fn a_tap_steps_or_sets_where_it_lands() {
        use PointerPhase::{Cancel, Down, Move, Up};
        use SliderStep::{Cancelled, Chosen, Same};
        let tap = |x: i32, up: (i32, i32)| {
            let mut drag = SliderDrag::default();
            assert_eq!(sample(&mut drag, Down, x, 140), Some(Same));
            sample(&mut drag, Up, up.0, up.1)
        };
        // On −, or in the margin beside it: a step down; on + or beside
        // it, a step up.
        assert_eq!(tap(40, (42, 150)), Some(Chosen(45)));
        assert_eq!(tap(4, (4, 140)), Some(Chosen(45)));
        assert_eq!(tap(430, (430, 140)), Some(Chosen(55)));
        assert_eq!(tap(476, (476, 140)), Some(Chosen(55)));
        // On the track: the value there.
        assert_eq!(tap(377, (377, 140)), Some(Chosen(100)));
        assert_eq!(tap(102, (102, 140)), Some(Chosen(0)));
        // Lifted off the band without moving sideways: as it was.
        assert_eq!(tap(300, (300, 400)), Some(Chosen(50)));
        // Taken away: the value kept shows again; a second finger meanwhile
        // is not taken.
        let mut drag = SliderDrag::default();
        sample(&mut drag, Down, 300, 140);
        let second = pointer(Down, 200, 140).with_id(1);
        let (slider, band) = across();
        assert_eq!(drag.on_pointer(&second, &TOUCH, &slider, band, 50), None);
        assert_eq!(drag.dragging(), Some(50));
        assert_eq!(sample(&mut drag, Cancel, 0, 0), Some(Cancelled));
        assert_eq!(sample(&mut drag, Up, 0, 0), None);
        // The same finger down again while its drag is held (its end got
        // lost): taken afresh, not held for good.
        let mut drag = SliderDrag::default();
        sample(&mut drag, Down, 300, 140);
        sample(&mut drag, Move, 360, 140);
        assert_eq!(sample(&mut drag, Down, 102, 140), Some(Same));
        assert_eq!(drag.dragging(), Some(50));
        assert_eq!(sample(&mut drag, Up, 102, 140), Some(Chosen(0)));
        // Down off the band over a lost drag: left to the screen, the
        // drag gone.
        sample(&mut drag, Down, 300, 140);
        assert_eq!(sample(&mut drag, Down, 300, 90), None);
        assert_eq!(drag.dragging(), None);
        // A value outside the range starts from its end.
        let mut drag = SliderDrag::default();
        drag.on_pointer(&pointer(Down, 300, 140), &TOUCH, &slider, band, 120);
        assert_eq!(drag.dragging(), Some(100));
    }

    #[test]
    fn buttons_move_it_a_step() {
        assert_eq!(key_step(&actions::LEFT), Some(-1));
        assert_eq!(key_step(&actions::PREV_PAGE), Some(-1));
        assert_eq!(key_step(&actions::RIGHT), Some(1));
        assert_eq!(key_step(&actions::NEXT), Some(1));
        assert_eq!(key_step(&actions::CONFIRM), None);
    }

    #[test]
    fn the_track_is_filled_up_to_the_value() {
        let slider = Slider::new(0, 100);
        let track = Rect::new(10, 0, 220, 40);
        let draw = |value: i64, ink: Color, paper: Color| {
            let mut frame = Framebuffer::new(240, 40, PixelFormat::Gray8).unwrap();
            let mut canvas = frame.canvas();
            canvas.clear(paper);
            slider.draw_track(&mut canvas, &STYLE, track, value, ink);
            frame
        };
        // Knob 20 wide: the track runs from 20 to 220, the middle at 120.
        let half = draw(50, Color::BLACK, Color::WHITE);
        let view = half.view();
        assert_eq!(view.pixel(40, 20), Color::BLACK);
        assert_eq!(view.pixel(120, 11), Color::BLACK, "the knob");
        assert_eq!(view.pixel(120, 29), Color::BLACK, "the knob");
        assert_eq!(view.pixel(180, 20), Color::BLACK, "thin after it");
        assert_eq!(view.pixel(180, 18), Color::WHITE, "thin after it");
        assert_eq!(view.pixel(40, 18), Color::BLACK, "thick before it");
        assert_eq!(view.pixel(40, 5), Color::WHITE, "a thin line");
        // At the end, filled all the way; white on the focus bar.
        let full = draw(100, Color::WHITE, Color::BLACK);
        assert_eq!(full.view().pixel(200, 20), Color::WHITE);
        assert_eq!(full.view().pixel(219, 12), Color::WHITE, "the knob");
        assert_eq!(full.view().pixel(5, 20), Color::BLACK, "nothing before it");
    }
}
