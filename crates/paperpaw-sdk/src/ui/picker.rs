//! Pickers for a date ([`DatePicker`]) and a time of day ([`TimePicker`]).

use core::fmt::{self, Write};

use crate::builtin::actions;
use crate::fonts::FontHandle;
use crate::gesture::{Gesture, GestureKind};
use crate::id::Id;
use crate::time::{Date, Time, days_in_month};
use crate::widgets::largest_size;
use crate::{Canvas, Env, Point, Rect};

use super::{
    Response, Stack, TextRole, UiStyle, button_height, columns, draw_button, draw_focus,
    draw_label_in,
};

/// The answer to an action the wheels `taken`, the picker `changed` or not.
fn answer(taken: bool, changed: bool) -> Response {
    if taken {
        Response::changed_if(changed)
    } else {
        Response::Ignored
    }
}

/// What a point of a picker is on ([`DatePicker::hit`],
/// [`TimePicker::hit`]): the `+` or the `−` of a field, or its value;
/// fields counted from the left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PickerPart {
    Plus(usize),
    Minus(usize),
    Value(usize),
}

/// The order of a date's fields from the left ([`DatePicker::order`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum DateOrder {
    /// 24 Dec 2026.
    #[default]
    DayMonthYear,
    /// Dec 24 2026.
    MonthDayYear,
    /// 2026 Dec 24.
    YearMonthDay,
}

/// The columns of a picker and which has the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wheels {
    cells: [Rect; 3],
    count: u8,
    /// The height of the `+` and `−`.
    button: u16,
    /// Between a button and the value.
    gap: u16,
    focus: u8,
    /// Whether the focus shows (from the start with buttons alone).
    shown: bool,
}

impl Wheels {
    fn new(style: &UiStyle, area: Rect, count: usize) -> Self {
        let height = picker_height(style).min(area.height);
        let band = Rect::new(area.x, area.y, area.width, height);
        let mut cells = [Rect::new(area.x, area.y, 0, 0); 3];
        let gap = (style.gap * 2) as u16;
        for (cell, column) in cells.iter_mut().zip(columns(band, count, gap)) {
            *cell = column;
        }
        Self {
            cells,
            count: count.min(3) as u8,
            button: button_size(style),
            gap: style.gap as u16,
            focus: 0,
            shown: style.finger == 0,
        }
    }

    fn count(&self) -> usize {
        usize::from(self.count)
    }

    fn focus(&self) -> usize {
        usize::from(self.focus)
    }

    /// The `+`, the value and the `−` of field `i`.
    fn parts(&self, i: usize) -> (Rect, Rect, Rect) {
        let mut stack = Stack::new(self.cells[i], self.gap);
        let plus = stack.take(self.button);
        let minus = stack.take_bottom(self.button);
        (plus, stack.rest(), minus)
    }

    fn hit(&self, point: Point) -> Option<PickerPart> {
        (0..self.count()).find_map(|i| {
            let (plus, _, minus) = self.parts(i);
            if plus.contains(point) {
                Some(PickerPart::Plus(i))
            } else if minus.contains(point) {
                Some(PickerPart::Minus(i))
            } else {
                self.cells[i]
                    .contains(point)
                    .then_some(PickerPart::Value(i))
            }
        })
    }

    /// Moves the focus with `action`, or calls `step` with the field and
    /// by how much to change it; false for actions it does not take.
    fn on_action(&mut self, action: &Id, mut step: impl FnMut(usize, i32)) -> bool {
        let count = self.count;
        let change = if [actions::LEFT, actions::RIGHT].contains(action) {
            None
        } else if [actions::UP, actions::NEXT_PAGE, actions::NEXT].contains(action) {
            Some(1)
        } else if [actions::DOWN, actions::PREV_PAGE, actions::PREVIOUS].contains(action) {
            Some(-1)
        } else {
            return false;
        };
        if !self.shown {
            self.shown = true;
            return true;
        }
        match change {
            Some(by) => step(self.focus(), by),
            None if count == 0 => {}
            None if *action == actions::LEFT => self.focus = (self.focus + count - 1) % count,
            None => self.focus = (self.focus + 1) % count,
        }
        true
    }

    /// Acts on a tap at `point` (see [`Wheels::on_action`]).
    /// Whether `gesture` is a tap on one of the buttons.
    fn takes(&self, gesture: &Gesture) -> bool {
        gesture.kind == GestureKind::Tap && self.hit(gesture.start).is_some()
    }

    fn on_tap(&mut self, point: Point, mut step: impl FnMut(usize, i32)) {
        let (field, by) = match self.hit(point) {
            Some(PickerPart::Plus(i)) => (i, 1),
            Some(PickerPart::Minus(i)) => (i, -1),
            Some(PickerPart::Value(i)) => (i, 0),
            None => return,
        };
        // The tapped field focused, the focus shown.
        self.focus = field as u8;
        self.shown = true;
        if by != 0 {
            step(field, by);
        }
    }

    /// The next field, false from the last.
    fn next_field(&mut self) -> bool {
        if self.focus() + 1 >= self.count() {
            return false;
        }
        self.focus += 1;
        self.shown = true;
        true
    }

    fn set_focus(&mut self, field: usize) {
        self.focus = field.min(self.count().saturating_sub(1)) as u8;
    }

    /// Draws field `i`: its `+` and `−`, and `text` large between them,
    /// with `mark` (e.g. "PM") small after it.
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    fn draw_field(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        style: &UiStyle,
        font: Option<&FontHandle<'_>>,
        i: usize,
        text: &str,
        mark: &str,
    ) {
        let (plus, value, minus) = self.parts(i);
        draw_button(canvas, env, style, plus, "+", false, false);
        draw_button(canvas, env, style, minus, "−", false, false);
        let focused = self.shown && self.focus() == i;
        if focused {
            draw_focus(canvas, style, value);
        }
        let Some(font) = font else {
            return;
        };
        let ink = style.focus_ink(focused);
        let small = style.size(TextRole::SMALL);
        let mark_width = if mark.is_empty() {
            0.0
        } else {
            font.measure(small, mark) + style.gap as f32 / 2.0
        };
        let room = f32::from(value.width) - 2.0 * style.gap as f32;
        let large = style.size(TextRole::HEADLINE);
        let size = largest_size(large, style.text, |size| {
            font.measure(size, text) + mark_width <= room
        })
        .unwrap_or(style.text);
        let text_width = font.measure(size, text);
        let x = f32::from(value.x) + (f32::from(value.width) - text_width - mark_width) / 2.0;
        draw_label_in(
            canvas,
            font,
            style,
            TextRole::BODY,
            size,
            x,
            value,
            text,
            ink,
        );
        if !mark.is_empty() {
            let at = x + text_width + style.gap as f32 / 2.0;
            draw_label_in(
                canvas,
                font,
                style,
                TextRole::SMALL,
                small,
                at,
                value,
                mark,
                ink,
            );
        }
    }
}

/// The height of a picker's `+` and `−`: a button's in a row that can be
/// touched.
fn button_size(style: &UiStyle) -> u16 {
    let row = style.touch_row(style.control_row) as u16;
    button_height(style, Rect::new(0, 0, 0, row))
}

/// The height of a picker: its `+`, its value and its `−`.
fn picker_height(style: &UiStyle) -> u16 {
    let value = ((style.size(TextRole::HEADLINE) * 1.5) as u16).max(style.row as u16);
    2 * button_size(style) + 2 * style.gap as u16 + value
}

/// `value` within `low..=high`, going round past either end.
fn wrap(value: i32, by: i32, low: i32, high: i32) -> i32 {
    low + (value - low + by).rem_euclid(high - low + 1)
}

/// A number written without allocating.
struct Digits {
    buf: [u8; 12],
    len: usize,
}

impl Digits {
    /// `value` with at least `width` digits (zeros before it).
    fn new(value: i32, width: usize) -> Self {
        let mut digits = Digits {
            buf: [0; 12],
            len: 0,
        };
        let _ = write!(digits, "{value:0width$}");
        digits
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl Write for Digits {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        let room = self.buf.get_mut(self.len..end).ok_or(fmt::Error)?;
        room.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// The short names of the months, by number from 1.
const MONTHS: [&str; 12] = [
    "core.month.short.1",
    "core.month.short.2",
    "core.month.short.3",
    "core.month.short.4",
    "core.month.short.5",
    "core.month.short.6",
    "core.month.short.7",
    "core.month.short.8",
    "core.month.short.9",
    "core.month.short.10",
    "core.month.short.11",
    "core.month.short.12",
];

/// A date's fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateField {
    Day,
    Month,
    Year,
}

/// A picker for a date, working as [`TimePicker`] does too: its fields
/// side by side (day, month and year; hour and minute), each a column
/// with a `+` above its value and a `−` below it.
///
/// Equally easy by touch and with buttons alone:
///
/// - Touch: a tap on a field's `+` or `−` changes it by one and gives it
///   the focus, a tap on its value gives it the focus ([`DatePicker::hit`]
///   tells what a point is on).
/// - Buttons: left and right choose the field (round from the last to the
///   first), up and down change it by one, as do the page buttons (the
///   only ones up and down on devices such as the Xteink X4) and *next*
///   and *previous*. The focused field's value stands on the focus
///   ([`Ui::focus`](crate::ui::Ui::focus)). On touch screens the focus shows
///   from the first button pressed, which does nothing else.
///
/// Minutes and hours go round (59 to 0, 23 to 0) without carrying over,
/// as do days and months; a day past the end of a month it comes to (31
/// April, 29 February outside a leap year) becomes its last. Years stay
/// within the picker's range ([`DatePicker::years`]). The calendar is the
/// SDK's ([`time`](crate::time)).
///
/// *Confirm* and *back* are the screen's: e.g. *confirm* takes the value,
/// or, where only two buttons may be bound, moves on to the next field
/// ([`DatePicker::next_field`]) until the last.
///
/// ```
/// use paperpaw_sdk::builtin::actions;
/// use paperpaw_sdk::time::Time;
/// use paperpaw_sdk::ui::{TimePicker, UiStyle};
/// use paperpaw_platform::Rect;
///
/// let style = UiStyle::PLAIN;
/// let area = Rect::new(0, 0, 300, TimePicker::height(&style));
/// let mut picker = TimePicker::new(&style, area, Time::new(23, 59));
/// assert!(picker.on_action(&actions::UP).changed());
/// assert_eq!(picker.value(), Time::new(0, 59));
/// ```
///
/// Small and copied freely; laid out once when made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatePicker {
    wheels: Wheels,
    /// The date it was made with, kept until its years are known
    /// ([`DatePicker::years`]).
    given: (i32, u8, u8),
    year: i32,
    month: u8,
    day: u8,
    years: (i32, i32),
    order: DateOrder,
}

impl DatePicker {
    /// A picker for `date` at the top of `area`, [`height`](Self::height)
    /// high: day, month, year from the left, years 1900 to 2199. A date
    /// that does not exist becomes the nearest that does.
    pub fn new(style: &UiStyle, area: Rect, date: Date) -> Self {
        let (year, month, day) = (date.year, date.month, date.day);
        let mut picker = Self {
            wheels: Wheels::new(style, area, 3),
            given: (year, month, day),
            year,
            month,
            day,
            years: (1900, 2199),
            order: DateOrder::DayMonthYear,
        };
        picker.clamp();
        picker
    }

    /// The years to pick from, `min` to `max` (no less than `min`).
    pub fn years(mut self, min: i32, max: i32) -> Self {
        self.years = (min, max.max(min));
        // The date given, not the one made to fit the years before.
        (self.year, self.month, self.day) = self.given;
        self.clamp();
        self
    }

    /// The fields in `order` from the left.
    pub fn order(self, order: DateOrder) -> Self {
        Self { order, ..self }
    }

    /// The height a date picker takes.
    pub fn height(style: &UiStyle) -> u16 {
        picker_height(style)
    }

    /// The date picked.
    pub fn value(&self) -> Date {
        Date::new(self.year, self.month, self.day)
    }

    /// The focused field, from the left.
    pub fn focus(&self) -> usize {
        self.wheels.focus()
    }

    /// Gives `field` (from the left) the focus.
    pub fn set_focus(&mut self, field: usize) {
        self.wheels.set_focus(field);
    }

    /// Moves the focus to the next field: false on the last.
    pub fn next_field(&mut self) -> bool {
        self.wheels.next_field()
    }

    /// What `point` is on.
    pub fn hit(&self, point: Point) -> Option<PickerPart> {
        self.wheels.hit(point)
    }

    /// Acts on a button's `action`: [`Response::Changed`] where the date
    /// or the focus changed. Actions it does not take, such as *confirm*,
    /// are the caller's.
    pub fn on_action(&mut self, action: &Id) -> Response {
        let before = *self;
        let fields = self.fields();
        let Self {
            wheels,
            year,
            month,
            day,
            years,
            ..
        } = self;
        let taken = wheels.on_action(action, |field, by| {
            step_date(
                (&mut *year, &mut *month, &mut *day),
                *years,
                fields[field],
                by,
            );
        });
        answer(taken, *self != before)
    }

    /// Acts on a tap on a button: [`Response::Changed`] where the date
    /// changed. Other gestures, and taps beside its buttons, are the
    /// caller's.
    pub fn on_gesture(&mut self, gesture: &Gesture) -> Response {
        if !self.wheels.takes(gesture) {
            return Response::Ignored;
        }
        let point = gesture.start;
        let before = *self;
        let fields = self.fields();
        let Self {
            wheels,
            year,
            month,
            day,
            years,
            ..
        } = self;
        wheels.on_tap(point, |field, by| {
            step_date(
                (&mut *year, &mut *month, &mut *day),
                *years,
                fields[field],
                by,
            );
        });
        Response::changed_if(*self != before)
    }

    /// Draws the picker.
    pub fn draw(&self, ui: &mut super::Ui<'_>) {
        let (env, style) = (ui.env(), ui.style());
        let canvas = ui.canvas();
        let font = style.font(env);
        for (i, field) in self.fields().into_iter().enumerate() {
            let number = match field {
                DateField::Day => Digits::new(i32::from(self.day), 1),
                DateField::Year => Digits::new(self.year, 4),
                DateField::Month => {
                    let name = env.i18n().tr(MONTHS[usize::from(self.month - 1)]);
                    self.wheels
                        .draw_field(canvas, env, style, font.as_ref(), i, name, "");
                    continue;
                }
            };
            let text = number.as_str();
            self.wheels
                .draw_field(canvas, env, style, font.as_ref(), i, text, "");
        }
    }

    fn fields(&self) -> [DateField; 3] {
        use DateField::{Day, Month, Year};
        match self.order {
            DateOrder::DayMonthYear => [Day, Month, Year],
            DateOrder::MonthDayYear => [Month, Day, Year],
            DateOrder::YearMonthDay => [Year, Month, Day],
        }
    }

    /// Brings the date into the range of years and the calendar.
    fn clamp(&mut self) {
        self.year = self.year.clamp(self.years.0, self.years.1);
        self.month = self.month.clamp(1, 12);
        self.day = self.day.clamp(1, days_in_month(self.year, self.month));
    }
}

/// Changes `field` of a date by `by`: days and months go round, years
/// stay within `years`, the day is kept within its month.
fn step_date(
    (year, month, day): (&mut i32, &mut u8, &mut u8),
    years: (i32, i32),
    field: DateField,
    by: i32,
) {
    match field {
        DateField::Day => {
            let last = i32::from(days_in_month(*year, *month));
            *day = wrap(i32::from(*day), by, 1, last) as u8;
        }
        DateField::Month => *month = wrap(i32::from(*month), by, 1, 12) as u8,
        DateField::Year => *year = year.saturating_add(by).clamp(years.0, years.1),
    }
    *day = (*day).min(days_in_month(*year, *month));
}

/// A picker for a time of day: hour and minute side by side, used as a
/// [`DatePicker`] is. Small and copied freely; laid out once when made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimePicker {
    wheels: Wheels,
    hour: u8,
    minute: u8,
    twelve: bool,
}

impl TimePicker {
    /// A picker for the hour and minute of `time` (larger ones than 23
    /// and 59 are the largest) at the top of `area`,
    /// [`height`](Self::height) high, the hour shown 0 to 23.
    pub fn new(style: &UiStyle, area: Rect, time: Time) -> Self {
        Self {
            wheels: Wheels::new(style, area, 2),
            hour: time.hour.min(23),
            minute: time.minute.min(59),
            twelve: false,
        }
    }

    /// The hour shown 1 to 12 with AM or PM, as the user may want it
    /// ([`time::twelve_hours`](crate::time::twelve_hours)); the value
    /// stays 0 to 23.
    pub fn twelve_hours(self, twelve: bool) -> Self {
        Self { twelve, ..self }
    }

    /// The height a time picker takes.
    pub fn height(style: &UiStyle) -> u16 {
        picker_height(style)
    }

    /// The time picked (hour 0 to 23).
    pub fn value(&self) -> Time {
        Time::new(self.hour, self.minute)
    }

    /// The focused field: 0 the hour, 1 the minute.
    pub fn focus(&self) -> usize {
        self.wheels.focus()
    }

    /// Gives `field` (0 the hour, 1 the minute) the focus.
    pub fn set_focus(&mut self, field: usize) {
        self.wheels.set_focus(field);
    }

    /// Moves the focus to the minute: false there.
    pub fn next_field(&mut self) -> bool {
        self.wheels.next_field()
    }

    /// What `point` is on.
    pub fn hit(&self, point: Point) -> Option<PickerPart> {
        self.wheels.hit(point)
    }

    /// Acts on a button's `action`: [`Response::Changed`] where the time
    /// or the focus changed. Actions it does not take, such as *confirm*,
    /// are the caller's.
    pub fn on_action(&mut self, action: &Id) -> Response {
        let before = *self;
        let Self {
            wheels,
            hour,
            minute,
            ..
        } = self;
        let taken = wheels.on_action(action, |field, by| step_time(hour, minute, field, by));
        answer(taken, *self != before)
    }

    /// Acts on a tap on a button: [`Response::Changed`] where the time
    /// changed. Other gestures, and taps beside its buttons, are the
    /// caller's.
    pub fn on_gesture(&mut self, gesture: &Gesture) -> Response {
        if !self.wheels.takes(gesture) {
            return Response::Ignored;
        }
        let point = gesture.start;
        let before = *self;
        let Self {
            wheels,
            hour,
            minute,
            ..
        } = self;
        wheels.on_tap(point, |field, by| step_time(hour, minute, field, by));
        Response::changed_if(*self != before)
    }

    /// Draws the picker, with a `:` between hour and minute.
    pub fn draw(&self, ui: &mut super::Ui<'_>) {
        let (env, style) = (ui.env(), ui.style());
        let canvas = ui.canvas();
        let font = style.font(env);
        let (hour, mark) = if self.twelve {
            let shown = match self.hour % 12 {
                0 => 12,
                h => h,
            };
            let key = if self.hour < 12 {
                "core.clock.am"
            } else {
                "core.clock.pm"
            };
            (Digits::new(i32::from(shown), 1), meridiem(env, key))
        } else {
            (Digits::new(i32::from(self.hour), 2), "")
        };
        let font_ref = font.as_ref();
        let wheels = &self.wheels;
        wheels.draw_field(canvas, env, style, font_ref, 0, hour.as_str(), mark);
        let minute = Digits::new(i32::from(self.minute), 2);
        wheels.draw_field(canvas, env, style, font_ref, 1, minute.as_str(), "");
        let Some(font) = font_ref else {
            return;
        };
        // The `:` in the middle of the room between the values.
        let (_, left, _) = wheels.parts(0);
        let (_, right, _) = wheels.parts(1);
        let size = style.size(TextRole::HEADLINE);
        let room = f32::from(right.x) - left.right() as f32;
        let x = left.right() as f32 + (room - font.measure(size, ":")) / 2.0;
        let band = Rect::new(
            left.right() as u16,
            left.y,
            right.x - left.right() as u16,
            left.height,
        );
        draw_label_in(
            canvas,
            font,
            style,
            TextRole::BODY,
            size,
            x,
            band,
            ":",
            style.focus_ink(false),
        );
    }
}

/// "AM" or "PM" from the translation `key` ("{time} PM"): what it adds
/// to the time.
fn meridiem<'a>(env: &'a Env<'_>, key: &'static str) -> &'a str {
    env.i18n()
        .tr(key)
        .split("{time}")
        .map(str::trim)
        .find(|part| !part.is_empty())
        .unwrap_or("")
}

/// Changes `field` (0 the hour, 1 the minute) by `by`, going round.
fn step_time(hour: &mut u8, minute: &mut u8, field: usize, by: i32) {
    if field == 0 {
        *hour = wrap(i32::from(*hour), by, 0, 23) as u8;
    } else {
        *minute = wrap(i32::from(*minute), by, 0, 59) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap(point: Point) -> Gesture {
        Gesture::new(GestureKind::Tap, point, point, crate::PointerTool::Finger)
    }
    use crate::ui::tests::STYLE;

    fn area() -> Rect {
        Rect::new(0, 0, 300, DatePicker::height(&STYLE))
    }

    /// The middle of `rect`.
    fn middle(rect: Rect) -> Point {
        Point::new(
            i32::from(rect.x + rect.width / 2),
            i32::from(rect.y + rect.height / 2),
        )
    }

    #[test]
    fn buttons_choose_the_field_and_change_it() {
        let mut date = DatePicker::new(&STYLE, area(), Date::new(2026, 10, 22));
        assert!(date.on_action(&actions::UP).changed());
        assert_eq!(date.value(), Date::new(2026, 10, 23));
        assert!(date.on_action(&actions::RIGHT).changed());
        assert_eq!(date.focus(), 1);
        assert!(date.on_action(&actions::NEXT_PAGE).changed());
        assert_eq!(date.value(), Date::new(2026, 11, 23));
        assert!(date.on_action(&actions::RIGHT).changed());
        assert!(date.on_action(&actions::DOWN).changed());
        assert_eq!(date.value(), Date::new(2025, 11, 23));
        // Round from the last field to the first, and back.
        assert!(date.on_action(&actions::RIGHT).changed());
        assert_eq!(date.focus(), 0);
        assert!(date.on_action(&actions::LEFT).changed());
        assert_eq!(date.focus(), 2);
        assert!(!date.on_action(&actions::CONFIRM).taken());
        // Years stay within the range.
        let mut date = DatePicker::new(&STYLE, area(), Date::new(2030, 1, 1)).years(2000, 2030);
        date.set_focus(2);
        assert!(!date.on_action(&actions::UP).changed());
        assert_eq!(date.value(), Date::new(2030, 1, 1));
    }

    #[test]
    fn days_keep_within_their_month_and_leap_years() {
        let mut date = DatePicker::new(&STYLE, area(), Date::new(2024, 1, 31));
        date.set_focus(1);
        assert!(date.on_action(&actions::UP).changed());
        assert_eq!(date.value(), Date::new(2024, 2, 29));
        date.set_focus(2);
        assert!(date.on_action(&actions::UP).changed());
        assert_eq!(date.value(), Date::new(2025, 2, 28));
        // Days go round within the month.
        date.set_focus(0);
        assert!(date.on_action(&actions::UP).changed());
        assert_eq!(date.value(), Date::new(2025, 2, 1));
        assert!(date.on_action(&actions::DOWN).changed());
        assert_eq!(date.value(), Date::new(2025, 2, 28));
        // Months go round without changing the year; 1900 is no leap year,
        // 2000 is.
        let mut date = DatePicker::new(&STYLE, area(), Date::new(2000, 2, 29));
        assert_eq!(date.value(), Date::new(2000, 2, 29));
        date.set_focus(1);
        let _ = date.on_action(&actions::DOWN);
        let _ = date.on_action(&actions::DOWN);
        assert_eq!(date.value(), Date::new(2000, 12, 29));
        assert_eq!(
            DatePicker::new(&STYLE, area(), Date::new(1900, 2, 29)).value(),
            Date::new(1900, 2, 28)
        );
        assert_eq!(
            DatePicker::new(&STYLE, area(), Date::new(2026, 4, 31)).value(),
            Date::new(2026, 4, 30)
        );
        assert_eq!(
            DatePicker::new(&STYLE, area(), Date::new(2026, 0, 0)).value(),
            Date::new(2026, 1, 1)
        );
    }

    #[test]
    fn the_order_says_which_field_is_where() {
        let mut date =
            DatePicker::new(&STYLE, area(), Date::new(2026, 3, 9)).order(DateOrder::YearMonthDay);
        assert!(date.on_action(&actions::UP).changed());
        assert_eq!(date.value(), Date::new(2027, 3, 9));
        date.set_focus(2);
        assert!(date.on_action(&actions::UP).changed());
        assert_eq!(date.value(), Date::new(2027, 3, 10));
    }

    #[test]
    fn hours_and_minutes_go_round() {
        let mut time = TimePicker::new(&STYLE, area(), Time::new(23, 59));
        assert!(time.on_action(&actions::UP).changed());
        assert_eq!(time.value(), Time::new(0, 59));
        assert!(time.on_action(&actions::PREVIOUS).changed());
        assert_eq!(time.value(), Time::new(23, 59));
        assert!(time.next_field());
        assert!(!time.next_field());
        assert!(time.on_action(&actions::NEXT).changed());
        assert_eq!(time.value(), Time::new(23, 0));
        assert!(time.on_action(&actions::PREV_PAGE).changed());
        assert_eq!(time.value(), Time::new(23, 59));
        assert_eq!(
            TimePicker::new(&STYLE, area(), Time::new(30, 75)).value(),
            Time::new(23, 59)
        );
    }

    #[test]
    fn taps_on_plus_and_minus_change_their_field() {
        let mut time = TimePicker::new(&STYLE, area(), Time::new(7, 30));
        let (plus, value, minus) = time.wheels.parts(1);
        // Each part lies where it is drawn, the value between.
        assert!(plus.bottom() < u32::from(value.y));
        assert!(value.bottom() < u32::from(minus.y));
        assert_eq!(minus.bottom(), u32::from(area().height));
        assert_eq!(time.hit(middle(plus)), Some(PickerPart::Plus(1)));
        assert!(time.on_gesture(&tap(middle(plus))).changed());
        assert_eq!((time.value(), time.focus()), (Time::new(7, 31), 1));
        assert!(time.on_gesture(&tap(middle(minus))).changed());
        assert!(time.on_gesture(&tap(middle(minus))).changed());
        assert_eq!(time.value(), Time::new(7, 29));
        // A tap on a value focuses it; one beside the picker does nothing.
        let (_, hour, _) = time.wheels.parts(0);
        assert_eq!(time.hit(middle(hour)), Some(PickerPart::Value(0)));
        assert!(time.on_gesture(&tap(middle(hour))).changed());
        assert!(!time.on_gesture(&tap(middle(hour))).changed());
        assert_eq!((time.value(), time.focus()), (Time::new(7, 29), 0));
        assert!(!time.on_gesture(&tap(Point::new(10, 400))).taken());
        // The minus of a date's year.
        let mut date = DatePicker::new(&STYLE, area(), Date::new(2026, 10, 22));
        let (_, _, minus) = date.wheels.parts(2);
        assert_eq!(date.hit(middle(minus)), Some(PickerPart::Minus(2)));
        assert!(date.on_gesture(&tap(middle(minus))).changed());
        assert_eq!(date.value(), Date::new(2025, 10, 22));
    }

    #[test]
    fn on_touch_screens_the_first_button_shows_the_focus() {
        let touch = UiStyle {
            finger: 50,
            ..STYLE
        };
        let mut time = TimePicker::new(&touch, area(), Time::new(7, 30));
        assert!(time.on_action(&actions::UP).changed());
        assert_eq!(time.value(), Time::new(7, 30));
        assert!(time.on_action(&actions::UP).changed());
        assert_eq!(time.value(), Time::new(8, 30));
    }

    #[test]
    fn a_tap_on_a_touch_screen_shows_the_focus_of_its_field() {
        let touch = UiStyle {
            finger: 50,
            ..STYLE
        };
        let mut date = DatePicker::new(&touch, area(), Date::new(2026, 10, 22));
        assert!(!date.wheels.shown);
        let (_, month, _) = date.wheels.parts(1);
        assert!(date.on_gesture(&tap(middle(month))).changed());
        assert!(date.wheels.shown);
        assert_eq!(date.focus(), 1);
    }

    #[test]
    fn years_keep_a_date_outside_the_default_range() {
        let date = DatePicker::new(&STYLE, area(), Date::new(1850, 6, 15)).years(1800, 1900);
        assert_eq!(date.value(), Date::new(1850, 6, 15));
        // Clamped once the range is known.
        let date = DatePicker::new(&STYLE, area(), Date::new(1850, 6, 15)).years(1870, 1900);
        assert_eq!(date.value(), Date::new(1870, 6, 15));
        let leap = DatePicker::new(&STYLE, area(), Date::new(1896, 2, 29)).years(1800, 1900);
        assert_eq!(leap.value(), Date::new(1896, 2, 29));
    }

    #[test]
    fn numbers_are_written_without_allocating() {
        assert_eq!(Digits::new(5, 2).as_str(), "05");
        assert_eq!(Digits::new(2026, 4).as_str(), "2026");
        assert_eq!(Digits::new(-44, 1).as_str(), "-44");
        assert_eq!(wrap(1, -1, 1, 12), 12);
        assert_eq!(wrap(59, 1, 0, 59), 0);
    }
}
