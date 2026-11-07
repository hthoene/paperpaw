//! The check's page laid out as ops: header, title and help, the body of the
//! step, and the controls with their places (for taps and the focus).
use super::{LIGHT_LEVELS, LightTest, gray_levels, reads};
use crate::steps::{self, Buttons, CardTest, Outcome, Results, Step};
use crate::test_screen::Metrics;
use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error, Host,
        callbacks::DrawFrame,
        fonts::{Font, FontRole, TextOrigin},
        formatting::Arg,
        snapshot::capture_snapshot,
    },
};
use paperpaw_ui_kit::{
    Messages, Page, Paint, Style, header,
    mem::push,
    paint,
    text::{self, copy, wrap_with},
};

/// The radius of a touch target, in millimetres.
pub const TARGET_MM: u32 = 5;

/// What a control on a page does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Answer(Outcome),
    /// The step's title, where the page has no room for its help (the largest
    /// text on a small screen): the help whole, in a dialog.
    Help,
    /// The screen page: the pattern turned black for white, with a fast
    /// refresh, which shows ghosting.
    Flip,
    /// The screen page: a full refresh, which clears it.
    FullRefresh,
    /// The buttons page: begins asking for the buttons, which take every press
    /// from then on.
    StartButtons,
    /// The light page: its levels once more.
    LightAgain,
    /// The Wi-Fi page: the Wi-Fi plugin's page, to join a network.
    WifiPage,
    /// The sleep page: sleep now.
    SleepNow,
    /// The end: all steps again.
    StartOver,
    /// The end: close the check.
    Done,
}

impl Control {
    /// The translation key of its label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Control::Answer(Outcome::Pass) => "diagnostics.check.pass",
            Control::Answer(Outcome::Fail) => "diagnostics.check.fail",
            Control::Answer(Outcome::Skip) => "diagnostics.check.skip",
            Control::Help => "diagnostics.check.title",
            Control::Flip => "diagnostics.check.flip",
            Control::FullRefresh => "diagnostics.check.full-refresh",
            Control::StartButtons => "diagnostics.check.buttons.start",
            Control::LightAgain => "diagnostics.check.light-again",
            Control::WifiPage => "diagnostics.check.wifi-page",
            Control::SleepNow => "diagnostics.check.sleep-now",
            Control::StartOver => "diagnostics.check.start-over",
            Control::Done => "diagnostics.check.done",
        }
    }
}

/// What the page shows, borrowed from the check.
pub struct Body<'a> {
    pub step: Option<Step>,
    pub flipped: bool,
    pub buttons: &'a Buttons,
    pub buttons_on: bool,
    pub tapped: &'a [bool; steps::TOUCH_TARGETS.len()],
    pub light: Option<LightTest>,
    pub card: Option<&'a CardTest>,
    pub networks: Option<usize>,
    pub radio: reads::Radio,
    pub results: &'a Results,
    pub unsaved: Option<&'a str>,
    /// Whether the Wi-Fi plugin has its page to open.
    pub wifi_page: bool,
}

/// A laid out page.
#[derive(Debug, Default)]
pub struct PageView {
    ops: Vec<Paint>,
    /// The controls with their places, tools first.
    pub controls: Vec<(Control, Rect)>,
    /// The header's tap regions.
    pub header: Page,
    dpi: u16,
}

impl PageView {
    /// Millimetres to pixels on this panel.
    #[must_use]
    pub fn metrics(&self) -> Metrics {
        Metrics(self.dpi)
    }

    /// The centres of the touch targets on the screen.
    #[must_use]
    pub fn targets(&self, width: u16, height: u16) -> [Point; steps::TOUCH_TARGETS.len()] {
        targets(self.metrics(), width, height)
    }

    /// Draws the page.
    ///
    /// # Errors
    /// Font and canvas refusals.
    pub fn draw(&self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let result = paint::replay(frame, &font, self.ops.iter().chain(&self.header.paint));
        font.release()?;
        result
    }
}

/// The centres of the touch targets on the screen.
#[must_use]
pub fn targets(metrics: Metrics, width: u16, height: u16) -> [Point; steps::TOUCH_TARGETS.len()] {
    let inset = i32::try_from(metrics.mm(TARGET_MM + 2)).unwrap_or(0);
    let span =
        |size: u16, tenths: u8| inset + (i32::from(size) - 2 * inset) * i32::from(tenths) / 10;
    steps::TOUCH_TARGETS.map(|(x, y)| Point::new(span(width, x), span(height, y)))
}

/// A step's title and what to do on its page; the end's at the end.
///
/// # Errors
/// The catalog's refusals and allocation failures.
pub fn texts(host: &Host<'_>, step: Option<Step>) -> Result<(String, String), Error> {
    let key = step.map_or("end", Step::key);
    let mut messages = Messages::new()?;
    Ok((
        messages.copy(host, &format!("diagnostics.check.{key}.title"))?,
        messages.copy(host, &format!("diagnostics.check.{key}.help"))?,
    ))
}

fn fill(ops: &mut Vec<Paint>, rect: Rect, color: Color) -> Result<(), Error> {
    paint::push(ops, Paint::Fill(rect, color))
}

/// `count` columns of `row` `gap` apart, the first at the left.
fn columns(row: Rect, count: usize, gap: u16) -> Vec<Rect> {
    let n = u16::try_from(count).unwrap_or(1).max(1);
    let width = row.width.saturating_sub(gap * (n - 1)) / n;
    (0..n)
        .map(|i| Rect::new(row.x + i * (width + gap), row.y, width, row.height))
        .collect()
}

struct Ink<'a> {
    font: &'a Font<'a>,
    style: &'a Style,
}

impl Ink<'_> {
    fn lines(&self, size: f32, text: &str, width: u16) -> Result<Vec<String>, Error> {
        wrap_with(
            |s| self.font.measure_text(size, s),
            text,
            f32::from(width),
            64,
        )
    }

    fn line_px(&self, size: f32) -> Result<u16, Error> {
        paperpaw_ui_kit::geometry::line_px(self.font, size)
    }

    /// Draws `text` wrapped into `area`, as many lines as fit.
    fn paragraph(
        &self,
        ops: &mut Vec<Paint>,
        area: Rect,
        size: f32,
        text: &str,
        color: Color,
    ) -> Result<(), Error> {
        let height = self.line_px(size)?;
        let mut top = area.y;
        for line in self.lines(size, text, area.width)? {
            if top + height > area.y + area.height {
                break;
            }
            text::label(
                ops,
                self.font,
                &line,
                size,
                f32::from(area.x),
                Rect::new(area.x, top, area.width, height),
                color,
                None,
            )?;
            top += height;
        }
        Ok(())
    }

    fn button(
        &self,
        ops: &mut Vec<Paint>,
        rect: Rect,
        label: &str,
        on: bool,
        focused: bool,
    ) -> Result<(), Error> {
        let s = self.style;
        let radius = if s.pill {
            u32::from(rect.height) / 2
        } else {
            s.corner
        };
        let thickness = u32::from(s.line) * if focused { 3 } else { 1 };
        let (text_color, filled) = if on { (s.paper, true) } else { (s.ink, false) };
        if filled {
            paint::push(ops, Paint::Focus(rect, radius, None, s.ink))?;
        }
        if !filled || focused {
            paint::push(ops, Paint::Focus(rect, radius, Some(thickness), s.ink))?;
        }
        let label = text::fit(
            self.font,
            s.text,
            label,
            f32::from(rect.width.saturating_sub(s.gap)),
        )?;
        let width = self.font.measure_text(s.text, &label)?;
        let (ascent, descent, _) = paperpaw_ui_kit::geometry::line_metrics(self.font, s.text)?;
        let baseline =
            i32::from(rect.y) + ((f32::from(rect.height) + ascent - descent) / 2.0) as i32;
        paint::push(
            ops,
            Paint::Text {
                text: label,
                size: s.text,
                origin: TextOrigin::new(
                    f32::from(rect.x) + (f32::from(rect.width) - width) / 2.0,
                    baseline,
                ),
                color: text_color,
                clip: Some(rect),
            },
        )
    }

    /// A row: a name and its value at the other end.
    fn row(
        &self,
        ops: &mut Vec<Paint>,
        rect: Rect,
        name: &str,
        value: &str,
        focused: bool,
    ) -> Result<(), Error> {
        let s = self.style;
        if focused {
            paint::push(
                ops,
                Paint::Focus(rect, s.corner, Some(u32::from(s.line) * 3), s.ink),
            )?;
        }
        let pad = f32::from(s.margin);
        let value_width = if value.is_empty() {
            0.0
        } else {
            self.font.measure_text(s.text, value)?
        };
        let room = f32::from(rect.width) - 2.0 * pad - value_width - f32::from(s.gap);
        let area = Rect::new(rect.x, rect.y, rect.width, rect.height);
        let name_end = Rect::new(
            rect.x,
            rect.y,
            (f32::from(rect.width) - pad - value_width - f32::from(s.gap)).max(0.0) as u16,
            rect.height,
        );
        let _ = (room, area);
        text::label(
            ops,
            self.font,
            name,
            s.text,
            f32::from(rect.x) + pad,
            name_end,
            s.ink,
            None,
        )?;
        if !value.is_empty() {
            text::label(
                ops,
                self.font,
                value,
                s.text,
                f32::from(rect.x) + f32::from(rect.width) - pad - value_width,
                rect,
                s.muted,
                None,
            )?;
        }
        Ok(())
    }
}

/// The lines of values a page shows: a name and its value each.
#[allow(clippy::too_many_lines, reason = "one arm for each step")]
fn lines(
    host: &Host<'_>,
    body: &Body<'_>,
    messages: &mut Messages,
) -> Result<Vec<(String, String)>, Error> {
    let mut out: Vec<(String, String)> = Vec::new();
    let tr = |messages: &mut Messages, key: &str| messages.copy(host, key);
    let snapshot = capture_snapshot(host)?;
    macro_rules! add {
        ($name:expr, $value:expr) => {
            push(&mut out, ($name, $value))?
        };
    }
    match body.step {
        Some(Step::Buttons) => {
            let asked = body.buttons.asked().filter(|_| body.buttons_on);
            for name in &body.buttons.names {
                let state = if body.buttons.pressed.contains(name) {
                    "diagnostics.check.button.pressed"
                } else if body.buttons.failed.contains(name) {
                    "diagnostics.check.button.failed"
                } else if asked == Some(name.as_str()) {
                    "diagnostics.check.button.asked"
                } else {
                    "diagnostics.check.button.waiting"
                };
                let key = format!("diagnostics.button.{name}");
                let label = messages.copy(host, &key).or_else(|_| copy(name))?;
                add!(label, tr(messages, state)?);
            }
        }
        Some(Step::Light) => {
            if let Some((brightness, warmth)) = body.light.and_then(|l| LIGHT_LEVELS.get(l.level)) {
                add!(
                    tr(messages, "diagnostics.check.brightness")?,
                    messages.digits(host, &format!("{brightness} %"))?
                );
                add!(
                    tr(messages, "diagnostics.check.warmth")?,
                    messages.digits(host, &format!("{warmth} %"))?
                );
            }
        }
        Some(Step::Battery) => {
            let yes_no = |messages: &mut Messages, yes: bool| {
                messages.copy(
                    host,
                    if yes {
                        "diagnostics.check.yes"
                    } else {
                        "diagnostics.check.no"
                    },
                )
            };
            let Some(percent) = snapshot.battery_percent() else {
                add!(tr(messages, "diagnostics.check.no-battery")?, String::new());
                return Ok(out);
            };
            add!(
                tr(messages, "diagnostics.check.charge")?,
                format!("{percent} %")
            );
            if let Some(mv) = snapshot.battery_millivolts() {
                add!(
                    tr(messages, "diagnostics.check.voltage")?,
                    format!("{mv} mV")
                );
            }
            add!(
                tr(messages, "diagnostics.check.charging")?,
                yes_no(messages, snapshot.is_charging())?
            );
            // What the inputs say by themselves, where the device has two
            // (#209): the level of the charging pin, and whether USB frames come.
            if let Some(high) = reads::value(host, "usb-pin-high")? {
                let level = if high != 0 { "usb-high" } else { "usb-low" };
                add!(
                    tr(messages, "diagnostics.check.usb-pin")?,
                    tr(messages, &format!("diagnostics.check.{level}"))?
                );
            }
            if let Some(moving) = reads::value(host, "usb-frames")? {
                let state = if moving != 0 {
                    "usb-moving"
                } else {
                    "usb-still"
                };
                add!(
                    tr(messages, "diagnostics.check.usb-frames")?,
                    tr(messages, &format!("diagnostics.check.{state}"))?
                );
            }
        }
        Some(Step::Card) => match body.card {
            None => add!(tr(messages, "diagnostics.check.measuring")?, String::new()),
            Some(test) => {
                if let Some(error) = &test.error {
                    add!(tr(messages, "diagnostics.check.error")?, copy(error)?);
                } else {
                    let speed = |messages: &mut Messages, us: Option<u64>| match us {
                        None => tr(messages, "diagnostics.check.unknown"),
                        Some(us) => messages
                            .digits(host, &format!("{} KiB/s", steps::speed(test.bytes, us))),
                    };
                    add!(
                        tr(messages, "diagnostics.check.write")?,
                        speed(messages, test.write_us)?
                    );
                    add!(
                        tr(messages, "diagnostics.check.read")?,
                        speed(messages, test.read_us)?
                    );
                }
            }
        },
        Some(Step::Clock) => {
            let unknown = |messages: &mut Messages| tr(messages, "diagnostics.check.unknown");
            let (time, date) = match snapshot.unix_seconds() {
                Some(secs) => {
                    let iso = paperpaw_sdk::time::UnixTime::from_secs(secs).iso8601();
                    let (date, time) = iso.split_once('T').unwrap_or((&iso, ""));
                    let time = time.split('.').next().unwrap_or(time);
                    (
                        Some(messages.digits(host, date)?),
                        Some(messages.digits(host, &format!("{time} UTC"))?),
                    )
                }
                None => (None, None),
            };
            let time = match time {
                Some(t) => t,
                None => unknown(messages)?,
            };
            let date = match date {
                Some(d) => d,
                None => unknown(messages)?,
            };
            add!(tr(messages, "diagnostics.check.time")?, time);
            add!(tr(messages, "diagnostics.check.date")?, date);
        }
        Some(Step::Wifi) => {
            let found = match body.networks {
                Some(n) => messages.digits(host, &format!("{n}"))?,
                None => tr(messages, "diagnostics.check.looking")?,
            };
            add!(tr(messages, "diagnostics.check.networks")?, found);
            add!(
                tr(messages, "diagnostics.check.joined")?,
                tr(
                    messages,
                    if body.radio.online {
                        "diagnostics.check.yes"
                    } else {
                        "diagnostics.check.no"
                    }
                )?
            );
        }
        Some(Step::Memory) => match reads::memory(host) {
            Err(_) => add!(tr(messages, "diagnostics.check.unknown")?, String::new()),
            Ok(m) => {
                let of = |messages: &mut Messages, free: u32, size: u32| {
                    messages.format(
                        host,
                        "diagnostics.check.free-of",
                        &[
                            Arg::Int {
                                name: "free",
                                value: i64::from(free / 1024),
                            },
                            Arg::Int {
                                name: "size",
                                value: i64::from(size / 1024),
                            },
                        ],
                    )
                };
                add!(
                    tr(messages, "diagnostics.check.heap")?,
                    of(messages, m.heap_free, m.heap_size)?
                );
                if let (Some(free), Some(size)) = (m.stack_free, m.stack_size) {
                    add!(
                        tr(messages, "diagnostics.check.stack")?,
                        format!("{free} B / {size} B")
                    );
                }
            }
        },
        None => {
            for step in Step::ALL {
                let outcome = match body.results.outcome(step) {
                    Some(o) => tr(messages, Control::Answer(o).label())?,
                    None => String::new(),
                };
                add!(texts(host, Some(step))?.0, outcome);
            }
            // Where the report is, the help says; here only that it is not.
            if let Some(error) = body.unsaved {
                let unsaved = messages.format(
                    host,
                    "diagnostics.check.unsaved",
                    &[Arg::Text {
                        name: "error",
                        value: error,
                    }],
                )?;
                add!(tr(messages, "diagnostics.check.report")?, unsaved);
            }
        }
        Some(Step::Screen | Step::Touch | Step::Sleep) => {}
    }
    let _ = gray_levels;
    Ok(out)
}

/// The screen's test: a stripe for each gray level, and below it a
/// checkerboard, turned black for white with `flipped`.
fn screen_test(
    ops: &mut Vec<Paint>,
    levels: u32,
    area: Rect,
    flipped: bool,
    metrics: Metrics,
) -> Result<(), Error> {
    let (x, y, w) = (u32::from(area.x), u32::from(area.y), u32::from(area.width));
    let ramp = u32::from(area.height) / 3;
    for level in 0..levels {
        let x0 = x + w * level / levels;
        let x1 = x + w * (level + 1) / levels;
        let gray = (level * 255 / (levels - 1)) as u8;
        fill(
            ops,
            Rect::new(x0 as u16, y as u16, (x1 - x0) as u16, ramp as u16),
            Color::gray(gray),
        )?;
    }
    let t = metrics.tenth_mm(3) as u16;
    let frame = Rect::new(area.x, area.y, area.width, ramp as u16);
    for edge in [
        Rect::new(frame.x, frame.y, frame.width, t),
        Rect::new(
            frame.x,
            frame.y + frame.height - t.min(frame.height),
            frame.width,
            t,
        ),
    ] {
        fill(ops, edge, Color::BLACK)?;
    }
    // Squares of about 6 mm, as many as fill the width exactly.
    let columns = (w / metrics.mm(6).max(4)).max(1);
    let cell = w / columns;
    let top = y + ramp + metrics.mm(3);
    let bottom = u32::from(area.y) + u32::from(area.height);
    let (mut row, mut cy) = (0, top);
    while cy + cell <= bottom {
        for col in 0..columns {
            let black = (row + col) % 2 == 0;
            let color = if black != flipped {
                Color::BLACK
            } else {
                Color::WHITE
            };
            fill(
                ops,
                Rect::new((x + col * cell) as u16, cy as u16, cell as u16, cell as u16),
                color,
            )?;
        }
        cy += cell;
        row += 1;
    }
    Ok(())
}

/// Lays the page out.
///
/// # Errors
/// The host's refusals and allocation failures.
#[allow(clippy::too_many_lines, reason = "one page, top to bottom")]
pub fn build(
    host: &Host<'_>,
    body: &Body<'_>,
    suggested: Option<Outcome>,
    width: u16,
    height: u16,
    focus: Option<usize>,
) -> Result<PageView, Error> {
    let snapshot = capture_snapshot(host)?;
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let result = build_with(
        host, &font, &snapshot, body, suggested, width, height, focus,
    );
    font.release()?;
    result
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one page, top to bottom"
)]
fn build_with(
    host: &Host<'_>,
    font: &Font<'_>,
    snapshot: &paperpaw_sdk::native::snapshot::Snapshot,
    body: &Body<'_>,
    suggested: Option<Outcome>,
    width: u16,
    height: u16,
    focus: Option<usize>,
) -> Result<PageView, Error> {
    let style = Style::read(host, snapshot, font)?;
    let metrics = Metrics(snapshot.display_dpi());
    let ink = Ink {
        font,
        style: &style,
    };
    let mut messages = Messages::new()?;
    let mut ops = Vec::new();
    fill(&mut ops, Rect::new(0, 0, width, height), style.paper)?;

    let touch_page = body.step == Some(Step::Touch);
    let mut header_page = Page::default();
    let (title_text, help_text) = texts(host, body.step)?;
    let margin = style.margin;
    let gap = style.gap;
    let inner = width.saturating_sub(2 * margin);
    let mut y = if touch_page {
        (metrics.mm(TARGET_MM * 2 + 2) + u32::from(gap)) as u16
    } else {
        // Which step, as a note: the header's page arrows would turn nothing here.
        let title = messages.copy(host, "diagnostics.check.title")?;
        header::prepare(
            &mut header_page,
            host,
            font,
            &style,
            &mut messages,
            &title,
            width,
            snapshot.has_touch(),
            0,
            1,
        )?;
        if let Some(step) = body.step {
            let note =
                messages.digits(host, &format!("{} / {}", step.index() + 1, Step::ALL.len()))?;
            let size = style.small;
            let w = font.measure_text(size, &note)?;
            text::label(
                &mut header_page.paint,
                font,
                &note,
                size,
                f32::from(width) - f32::from(margin) - w,
                Rect::new(0, 0, width, style.header),
                if style.dark { style.paper } else { style.muted },
                None,
            )?;
        }
        style.header + gap * 2
    };
    let lines_of = |size: f32, text: &str| -> Result<u16, Error> {
        Ok(ink.line_px(size)?
            * u16::try_from(ink.lines(size, text, inner)?.len().max(1)).unwrap_or(1))
    };
    let title = Rect::new(margin, y, inner, lines_of(style.title, &title_text)?);
    y += title.height + gap;

    let button = style.row;
    let answers: Vec<Control> = match body.step {
        None => alloc::vec![Control::StartOver, Control::Done],
        Some(_) => Outcome::ALL.into_iter().map(Control::Answer).collect(),
    };
    let tools: Vec<Control> = match body.step {
        Some(Step::Screen) => alloc::vec![Control::Flip, Control::FullRefresh],
        Some(Step::Buttons) if !body.buttons_on => alloc::vec![Control::StartButtons],
        Some(Step::Light) if body.light.is_none() => alloc::vec![Control::LightAgain],
        Some(Step::Wifi) if body.wifi_page => alloc::vec![Control::WifiPage],
        Some(Step::Sleep) => alloc::vec![Control::SleepNow],
        _ => Vec::new(),
    };
    // The touch page keeps its edges free for the targets: its answers go
    // between the targets halfway down and those at the bottom.
    let bottom = if touch_page {
        height / 4 * 3 + button / 2
    } else {
        height.saturating_sub(margin)
    };
    let answer_row = Rect::new(margin, bottom.saturating_sub(button), inner, button);
    let tool_row = Rect::new(
        margin,
        answer_row.y.saturating_sub(gap * 2 + button),
        inner,
        button,
    );
    let above = if tools.is_empty() {
        answer_row
    } else {
        tool_row
    };
    let body_bottom = if touch_page {
        height.saturating_sub(margin)
    } else {
        above.y.saturating_sub(gap * 2)
    };
    // The help whole, with a row's room at least left for the body.
    let room = body_bottom.saturating_sub(y + gap * 2 + button);
    let small = style.small;
    let mut help_size = None;
    let mut size = small;
    while size >= small * 0.6 {
        if lines_of(size, &help_text)? <= room {
            help_size = Some(size);
            break;
        }
        size *= 0.93;
    }
    // Where it does not fit so, the title opens it.
    let help = match help_size {
        Some(size) => Rect::new(margin, y, inner, lines_of(size, &help_text)?),
        None => Rect::new(margin, y, inner, 0),
    };
    y += if help.height > 0 {
        help.height + gap * 2
    } else {
        gap
    };
    let help_size = help_size.unwrap_or(small);
    let body_rect = Rect::new(margin, y, inner, body_bottom.saturating_sub(y));

    let mut controls = Vec::new();
    if help.height == 0 {
        push(&mut controls, (Control::Help, title))?;
    }
    for (row, list) in [(tool_row, &tools), (answer_row, &answers)] {
        for (control, rect) in list.iter().zip(columns(row, list.len(), gap)) {
            push(&mut controls, (*control, rect))?;
        }
    }

    // Title and help.
    if help.height > 0 {
        ink.paragraph(&mut ops, title, style.title, &title_text, style.ink)?;
        ink.paragraph(&mut ops, help, help_size, &help_text, style.note)?;
    }
    // The body.
    match body.step {
        Some(Step::Screen) => {
            screen_test(
                &mut ops,
                gray_levels(snapshot),
                body_rect,
                body.flipped,
                metrics,
            )?;
        }
        Some(Step::Touch) => {
            let radius = metrics.mm(TARGET_MM);
            let line = metrics.tenth_mm(5);
            for (target, tapped) in targets(metrics, width, height).iter().zip(body.tapped) {
                let d = (radius * 2) as u16;
                let rect = Rect::new(
                    (target.x - radius as i32).max(0) as u16,
                    (target.y - radius as i32).max(0) as u16,
                    d,
                    d,
                );
                let thickness = if *tapped { None } else { Some(line) };
                paint::push(
                    &mut ops,
                    Paint::Focus(rect, radius, thickness, Color::BLACK),
                )?;
            }
        }
        _ => {
            let shown = lines(host, body, &mut messages)?;
            // A row each, closer where they do not fit so (the end's list).
            let line = ink.line_px(style.small)?;
            let count = u16::try_from(shown.len()).unwrap_or(u16::MAX).max(1);
            let row = style.row.min(body_rect.height / count).max(line);
            let mut top = body_rect.y;
            for (name, value) in &shown {
                if top + row > body_rect.y + body_rect.height {
                    break;
                }
                ink.row(
                    &mut ops,
                    Rect::new(
                        body_rect.x.saturating_sub(margin),
                        top,
                        body_rect.width + 2 * margin,
                        row,
                    ),
                    name,
                    value,
                    false,
                )?;
                top += row;
            }
        }
    }
    // The controls: the answer suggested stands out; while buttons are asked
    // for, they choose nothing.
    let suggested = suggested.map(Control::Answer);
    for (index, (control, rect)) in controls.iter().enumerate() {
        let focused = focus == Some(index);
        if *control == Control::Help {
            // A row, widened by its margin inside: its title lines up with the
            // text below.
            let row = Rect::new(
                rect.x.saturating_sub(margin),
                rect.y,
                rect.width + 2 * margin,
                rect.height,
            );
            ink.row(&mut ops, row, &title_text, "›", focused)?;
            continue;
        }
        let label = messages.copy(host, control.label())?;
        let on = Some(*control) == suggested || *control == Control::Done;
        ink.button(&mut ops, *rect, &label, on, focused)?;
    }
    Ok(PageView {
        ops,
        controls,
        header: header_page,
        dpi: snapshot.display_dpi(),
    })
}
