//! The test screen: every gray level of the panel, four boxes to focus and
//! check (buttons and touch), and pages turned with a progress bar. It is
//! shown as ops laid out first, so what it draws can be tested apart from a
//! host (`layout`, `shapes`).
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Handled, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole, TextOrigin},
        formatting::Arg,
        settings::{self, SettingValue},
        snapshot::capture_snapshot,
    },
};
use paperpaw_ui_kit::{Messages, Paint, paint};

/// The setting that says how many pages the test turns.
pub const PAGES: &str = "diagnostics.pages";
const DEFAULT_PAGES: i64 = 20;

/// Pixels for lengths in millimetres at `dpi`, rounded and at least 1 for a
/// non-zero length.
#[derive(Clone, Copy, Debug)]
pub struct Metrics(pub u16);

impl Metrics {
    /// Pixels for `tenths` tenths of a millimetre.
    #[must_use]
    pub const fn tenth_mm(self, tenths: u32) -> u32 {
        let px = (tenths * self.0 as u32 + 127) / 254;
        if px == 0 && tenths > 0 { 1 } else { px }
    }

    /// Pixels for `mm` millimetres.
    #[must_use]
    pub const fn mm(self, mm: u32) -> u32 {
        self.tenth_mm(mm * 10)
    }
}

/// Screen regions, computed once per screen size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub frame: Rect,
    pub header: Rect,
    pub ramp: Rect,
    pub boxes: [Rect; 4],
    /// Page counter text above the progress bar.
    pub label: Rect,
    pub progress: Rect,
}

impl Layout {
    #[must_use]
    pub fn new(width: u16, height: u16, metrics: Metrics) -> Self {
        let px = |mm| metrics.mm(mm).min(u32::from(u16::MAX)) as u16;
        let margin = px(3);
        let gap = px(4);
        let inner_w = width.saturating_sub(2 * margin);
        let frame = Rect::new(margin, margin, inner_w, height.saturating_sub(2 * margin));

        let content_x = margin + gap;
        let content_w = inner_w.saturating_sub(2 * gap);
        let header = Rect::new(content_x, margin + gap, content_w, px(8));
        let ramp = Rect::new(content_x, header.y + header.height + gap, content_w, px(10));

        let progress_h = px(6);
        let progress_y = (frame.y + frame.height).saturating_sub(gap + progress_h);
        let progress = Rect::new(content_x, progress_y, content_w, progress_h);
        let label_h = px(7);
        let label_y = progress_y.saturating_sub(label_h);
        let label = Rect::new(content_x, label_y, content_w, label_h);

        let grid_y = ramp.y + ramp.height + gap;
        let grid_h = label_y.saturating_sub(grid_y + gap / 2);
        let box_w = content_w.saturating_sub(gap) / 2;
        let box_h = grid_h.saturating_sub(gap) / 2;
        let cell = |col: u16, row: u16| {
            Rect::new(
                content_x + col * (box_w + gap),
                grid_y + row * (box_h + gap),
                box_w,
                box_h,
            )
        };
        Self {
            frame,
            header,
            ramp,
            boxes: [cell(0, 0), cell(1, 0), cell(0, 1), cell(1, 1)],
            label,
            progress,
        }
    }
}

/// What the panel shows, for the ramp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    /// Gray levels, 16 where the format has none (at most 16 stripes).
    pub levels: u32,
    /// Whether the panel shows colour.
    pub colour: bool,
}

/// What the screen holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub page: u32,
    pub focus: usize,
    pub checked: [bool; 4],
}

impl State {
    const fn start() -> Self {
        Self {
            page: 0,
            focus: 0,
            checked: [false; 4],
        }
    }

    /// The page shown: within the pages, also when they became fewer.
    #[must_use]
    pub fn shown(&self, pages: u32) -> u32 {
        self.page.min(pages.saturating_sub(1))
    }
}

fn stroke(ops: &mut Vec<Paint>, r: Rect, thickness: u32, color: Color) -> Result<(), Error> {
    let t = thickness
        .min(u32::from(r.width) / 2)
        .min(u32::from(r.height) / 2) as u16;
    if t == 0 {
        return Ok(());
    }
    for edge in [
        Rect::new(r.x, r.y, r.width, t),
        Rect::new(r.x, r.y + r.height - t, r.width, t),
        Rect::new(r.x, r.y + t, t, r.height - 2 * t),
        Rect::new(r.x + r.width - t, r.y + t, t, r.height - 2 * t),
    ] {
        paint::push(ops, Paint::Fill(edge, color))?;
    }
    Ok(())
}

fn fill(ops: &mut Vec<Paint>, r: Rect, color: Color) -> Result<(), Error> {
    paint::push(ops, Paint::Fill(r, color))
}

/// The rectangles of the screen: frame, header bar, the ramp (one stripe for
/// each gray level, the primary and secondary colours under it on a colour
/// panel), the boxes with the focus and checks, and the progress bar.
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn shapes(
    layout: &Layout,
    metrics: Metrics,
    panel: Panel,
    state: &State,
    pages: u32,
) -> Result<Vec<Paint>, Error> {
    let mut ops = Vec::new();
    stroke(&mut ops, layout.frame, metrics.tenth_mm(5), Color::BLACK)?;
    fill(&mut ops, layout.header, Color::BLACK)?;

    let levels = panel.levels.clamp(2, 16);
    let r = layout.ramp;
    let (x, y, w, mut h) = (
        u32::from(r.x),
        u32::from(r.y),
        u32::from(r.width),
        u32::from(r.height),
    );
    if panel.colour {
        // Lower half: primary and secondary colours for colour panels.
        h /= 2;
        let hues = [
            Color::rgb(255, 0, 0),
            Color::rgb(255, 255, 0),
            Color::rgb(0, 255, 0),
            Color::rgb(0, 255, 255),
            Color::rgb(0, 0, 255),
            Color::rgb(255, 0, 255),
        ];
        for (i, hue) in (0u32..).zip(hues) {
            let x0 = x + w * i / 6;
            let x1 = x + w * (i + 1) / 6;
            fill(
                &mut ops,
                Rect::new(x0 as u16, (y + h) as u16, (x1 - x0) as u16, h as u16),
                hue,
            )?;
        }
    }
    for level in 0..levels {
        let x0 = x + w * level / levels;
        let x1 = x + w * (level + 1) / levels;
        let gray = (level * 255 / (levels - 1)) as u8;
        fill(
            &mut ops,
            Rect::new(x0 as u16, y as u16, (x1 - x0) as u16, h as u16),
            Color::gray(gray),
        )?;
    }
    stroke(&mut ops, r, metrics.tenth_mm(3), Color::BLACK)?;

    for (index, b) in layout.boxes.iter().enumerate() {
        fill(&mut ops, *b, Color::WHITE)?;
        if state.checked[index] {
            let inset = metrics
                .mm(3)
                .min(u32::from(b.width) / 2)
                .min(u32::from(b.height) / 2) as u16;
            fill(
                &mut ops,
                Rect::new(
                    b.x + inset,
                    b.y + inset,
                    b.width - 2 * inset,
                    b.height - 2 * inset,
                ),
                Color::gray(64),
            )?;
        }
        let border = if index == state.focus {
            metrics.tenth_mm(12)
        } else {
            metrics.tenth_mm(3)
        };
        stroke(&mut ops, *b, border, Color::BLACK)?;
    }

    let p = layout.progress;
    fill(&mut ops, p, Color::WHITE)?;
    let border = metrics.tenth_mm(3);
    stroke(&mut ops, p, border, Color::BLACK)?;
    let inner = u32::from(p.width).saturating_sub(2 * border);
    let filled = if pages <= 1 {
        inner
    } else {
        (u64::from(inner) * u64::from(state.shown(pages)) / u64::from(pages - 1)) as u32
    };
    let bar_h = u32::from(p.height).saturating_sub(2 * border);
    if filled > 0 && bar_h > 0 {
        fill(
            &mut ops,
            Rect::new(
                p.x + border as u16,
                p.y + border as u16,
                filled as u16,
                bar_h as u16,
            ),
            Color::BLACK,
        )?;
    }
    Ok(ops)
}

/// The box the focus goes to: left and right step through all boxes (enough
/// for devices with only two navigation buttons), up and down switch rows.
#[must_use]
pub fn move_focus(focus: usize, action: &str) -> Option<usize> {
    match action {
        "core.left" => Some((focus + 3) % 4),
        "core.right" => Some((focus + 1) % 4),
        "core.up" | "core.down" => Some((focus + 2) % 4),
        _ => None,
    }
}

/// The test screen of one instance.
#[derive(Debug)]
pub struct TestScreen {
    layout: Layout,
    metrics: Metrics,
    panel: Panel,
    state: State,
}

impl TestScreen {
    /// A screen of `width` by `height` pixels on the device's panel.
    ///
    /// # Errors
    /// The snapshot's refusal.
    pub fn new(host: &Host<'_>, width: u16, height: u16) -> Result<Self, Error> {
        let snapshot = capture_snapshot(host)?;
        let metrics = Metrics(snapshot.display_dpi());
        Ok(Self {
            layout: Layout::new(width, height, metrics),
            metrics,
            panel: Panel {
                levels: snapshot.display_gray_levels().map_or(16, u32::from),
                colour: snapshot.display_is_colour(),
            },
            state: State::start(),
        })
    }

    /// The screen was resized (a pane changed size).
    pub fn resize(&mut self, width: u16, height: u16) {
        self.layout = Layout::new(width, height, self.metrics);
    }

    fn pages(host: &Host<'_>) -> u32 {
        let mut scratch = [0; 32];
        let value = match settings::read_setting(host, PAGES, &mut scratch) {
            Ok(SettingValue::Int(value)) => value,
            _ => DEFAULT_PAGES,
        };
        value.clamp(1, i64::from(u32::MAX)) as u32
    }

    fn redraw(host: &Host<'_>, mode: RedrawMode) -> Result<Handled, Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(mode)?;
        Ok(Handled::Yes)
    }

    /// Back to the start: the first page, nothing chosen.
    ///
    /// # Errors
    /// The host's refusal of the redraw.
    pub fn reset(&mut self, host: &Host<'_>) -> Result<Handled, Error> {
        self.state = State::start();
        Self::redraw(host, RedrawMode::Full)
    }

    fn set_focus(&mut self, host: &Host<'_>, focus: usize) -> Result<(), Error> {
        self.state.focus = focus;
        // Focus feedback should be quick; ghosting is cleaned up later.
        Self::redraw(host, RedrawMode::Fast).map(|_| ())
    }

    fn toggle(&mut self, host: &Host<'_>, index: usize) -> Result<Handled, Error> {
        self.state.checked[index] = !self.state.checked[index];
        Self::redraw(host, RedrawMode::Partial)
    }

    /// An action the host resolved for this screen.
    ///
    /// # Errors
    /// The host's refusal of the redraw.
    pub fn action(&mut self, host: &Host<'_>, action: &str) -> Result<Handled, Error> {
        let pages = Self::pages(host);
        match action {
            "core.next-page" | "core.prev-page" => {
                let shown = self.state.shown(pages);
                let page = if action == "core.next-page" {
                    (shown + 1).min(pages - 1)
                } else {
                    shown.saturating_sub(1)
                };
                if page != self.state.page {
                    self.state.page = page;
                    Self::redraw(host, RedrawMode::Partial)?;
                }
                Ok(Handled::Yes)
            }
            "core.confirm" => self.toggle(host, self.state.focus),
            other => match move_focus(self.state.focus, other) {
                Some(focus) => {
                    if focus != self.state.focus {
                        self.set_focus(host, focus)?;
                    }
                    Ok(Handled::Yes)
                }
                None => Ok(Handled::No),
            },
        }
    }

    /// A tap checks the box under it; a held finger starts over, as a held
    /// *Select* does.
    ///
    /// # Errors
    /// The host's refusal of the redraw.
    pub fn gesture(
        &mut self,
        host: &Host<'_>,
        position: Point,
        kind: InputGestureKind,
    ) -> Result<Handled, Error> {
        match kind {
            InputGestureKind::LongPress => self.reset(host),
            InputGestureKind::Tap => {
                let Some(index) = self.layout.boxes.iter().position(|b| b.contains(position))
                else {
                    return Ok(Handled::No);
                };
                if index != self.state.focus {
                    self.set_focus(host, index)?;
                }
                self.toggle(host, index)
            }
            _ => Ok(Handled::No),
        }
    }

    /// Draws the screen.
    ///
    /// # Errors
    /// Font, canvas and allocation refusals.
    pub fn draw(&self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let pages = Self::pages(host);
        let mut ops = shapes(&self.layout, self.metrics, self.panel, &self.state, pages)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "diagnostics.title")?;
        let digits = |value: u32| -> i64 { i64::from(value) };
        let label = messages.format(
            host,
            "diagnostics.page",
            &[
                Arg::Int {
                    name: "page",
                    value: digits(self.state.shown(pages)) + 1,
                },
                Arg::Int {
                    name: "pages",
                    value: digits(pages),
                },
            ],
        )?;
        self.texts(&mut ops, &font, &title, &label)?;
        let result = paint::replay(frame, &font, &ops);
        font.release()?;
        result
    }

    /// The header's title (white on the bar) and the page label (centred).
    fn texts(
        &self,
        ops: &mut Vec<Paint>,
        font: &Font<'_>,
        title: &str,
        label: &str,
    ) -> Result<(), Error> {
        let line = |ops: &mut Vec<Paint>,
                    area: Rect,
                    pad: f32,
                    text: &str,
                    color: Color,
                    centred: bool| {
            let size = f32::from(area.height) * 0.5;
            let (ascent, descent, _) = paperpaw_ui_kit::geometry::line_metrics(font, size)?;
            let baseline =
                i32::from(area.y) + ((f32::from(area.height) + ascent - descent) / 2.0) as i32;
            let x = if centred {
                f32::from(area.x) + (f32::from(area.width) - font.measure_text(size, text)?) / 2.0
            } else {
                f32::from(area.x) + pad
            };
            paint::push(
                ops,
                Paint::Text {
                    text: paperpaw_ui_kit::text::copy(text)?,
                    size,
                    origin: TextOrigin::new(x, baseline),
                    color,
                    clip: Some(area),
                },
            )
        };
        line(
            ops,
            self.layout.header,
            self.metrics.mm(2) as f32,
            title,
            Color::WHITE,
            false,
        )?;
        let area = self.layout.label;
        // The label is cleared first: it changes with the page.
        fill(ops, area, Color::WHITE)?;
        line(ops, area, 0.0, label, Color::BLACK, true)
    }
}

#[cfg(test)]
mod tests;
