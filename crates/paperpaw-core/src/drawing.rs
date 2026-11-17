//! Drawing frames: what changed, in bands for the shell, in grays,
//! described for reading aloud and tests, captured as a screenshot; the
//! busy mark; the screen made anew for another size.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;

/// What is drawn: the parts changed, the bands, large pictures shown, a
/// screenshot to save, the busy mark, how the panel refreshes.
#[derive(Debug)]
pub(crate) struct FrameState {
    /// The part of the screen to redraw at the end of the current call.
    pub(crate) pending: Option<Rect>,
    /// The areas of what was asked for in `pending`, added up.
    pub(crate) pending_weight: u32,
    /// What changed but has not been passed to [`Kernel::render`] yet.
    pub(crate) unrendered: Option<Rect>,
    /// What [`Kernel::render`] passed last, for [`Kernel::rerender`].
    pub(crate) rendered: Option<Rect>,
    /// See [`KernelBuilder::row_align`].
    pub(crate) row_align: u16,
    /// The pixels of a band, where frames are drawn in bands
    /// ([`KernelBuilder::frame_rows`]): kept when the screen is turned.
    pub(crate) band: Option<u32>,
    /// The frame shown has large pictures (see [`PICTURES_PERCENT`]).
    pub(crate) pictures_shown: bool,
    /// The opening of the screen (`ActiveScreen::serial`) that showed them.
    pub(crate) pictures_on: Option<u32>,
    /// The whole screen is drawn again as it was ([`Kernel::draw_again`]),
    /// in black and white after grays: not in grays for its pictures.
    pub(crate) again: bool,
    /// A screenshot to save with the next frame drawn: its path.
    pub(crate) screenshot: Option<String>,
    /// The mark shown while slow work waits, and the hint cell it is.
    pub(crate) mark: Option<(Rect, Option<usize>)>,
    pub(crate) refresh: RefreshPolicy,
    /// Areas a native screen asked for with the whole screen, drawn in the
    /// frame after it (`commands_redraw.rs`).
    pub(crate) followup: Option<crate::native_host::commands::Followup>,
    /// The screen whose follow-up the frame to draw is (its draws are marked,
    /// also when it is drawn again for the panel).
    pub(crate) followup_frame: Option<paperpaw_sdk::ScreenKey>,
    /// The grays a screen with pictures gets after its frame in black and
    /// white, once calm (`gray_later.rs`).
    pub(crate) gray_later: crate::gray_later::GrayLater,
}

/// How a framebuffer draws, kept while it is made anew.
#[derive(Debug, Clone, Copy)]
struct Look {
    inverted: bool,
    pictures_kept: bool,
    saturation: u16,
    mono_coverage: u8,
}

impl Look {
    fn of(framebuffer: &Framebuffer) -> Self {
        Self {
            inverted: framebuffer.is_inverted(),
            pictures_kept: framebuffer.pictures_kept(),
            saturation: framebuffer.saturation(),
            mono_coverage: framebuffer.mono_coverage(),
        }
    }

    /// `framebuffer`, drawing so.
    fn on(self, mut framebuffer: Framebuffer) -> Framebuffer {
        framebuffer.set_inverted(self.inverted);
        framebuffer.set_pictures_kept(self.pictures_kept);
        framebuffer.set_saturation(self.saturation);
        framebuffer.set_mono_coverage(self.mono_coverage);
        framebuffer
    }
}

/// A framebuffer made by `make` in bands of `rows` rows, or of fewer
/// (halved, down to `align`) where memory is short; `None` where not even
/// that fits.
fn with_fewer_rows(
    rows: u16,
    align: u16,
    mut make: impl FnMut(u16) -> Result<Framebuffer, GraphicsError>,
) -> Option<Framebuffer> {
    let align = align.max(1);
    let mut rows = rows.max(align);
    loop {
        if let Ok(framebuffer) = make(rows) {
            return Some(framebuffer);
        }
        if rows <= align {
            return None;
        }
        rows = (rows / 2 / align * align).max(align);
    }
}

impl Kernel {
    /// Draws the whole screen again in four grays (two bits a pixel), in
    /// bands as [`Kernel::render`] passes them, for a shell that shows a
    /// frame's pictures in grays ([`Effects::grays`]): bands of as many
    /// bytes as its own, which goes meanwhile. `false` where there is no
    /// memory for it (nothing passed then).
    pub fn render_gray(&mut self, out: &mut dyn FnMut(u16, FrameView<'_>)) -> bool {
        use paperpaw_platform::PixelFormat;
        let format = self.framebuffer.format();
        let bounds = self.framebuffer.bounds();
        let rows = self.framebuffer.rows();
        let look = Look::of(&self.framebuffer);
        let bits = u32::from(format.bits_per_pixel()).max(1);
        let align = self.frame.row_align;
        let gray_rows = (((u32::from(rows) * bits / 2) as u16) / align * align)
            .max(align)
            .min(bounds.height);
        // The band's memory is the gray one's (as many bytes) and then the
        // band's again: one block kept, nothing allocated between them
        // (#306).
        let Ok(none) = Framebuffer::new(1, 1, format) else {
            return false;
        };
        let room = core::mem::replace(&mut self.framebuffer, none).into_data();
        let mut room = Some(room);
        let gray = with_fewer_rows(gray_rows, align, |rows| {
            Framebuffer::banded_in(
                bounds.width,
                bounds.height,
                rows,
                PixelFormat::Gray2,
                room.take().unwrap_or_default(),
            )
        });
        let (drawn, room) = match gray {
            Some(gray) => {
                let none = core::mem::replace(&mut self.framebuffer, look.on(gray));
                self.frame.unrendered = Some(bounds);
                self.render(out);
                let gray = core::mem::replace(&mut self.framebuffer, none);
                (true, gray.into_data())
            }
            None => (false, Vec::new()),
        };
        self.restore_frame(look, bounds.width, bounds.height, rows, format, room);
        drawn
    }

    /// Makes the frame of `width` by `height` in bands of `rows` rows
    /// (fewer where memory is short now, said in a warning), drawing as
    /// `look` says; `false`, with a warning, where there is no memory for
    /// it at all, which leaves nothing drawn until there is.
    fn restore_frame(
        &mut self,
        look: Look,
        width: u16,
        height: u16,
        rows: u16,
        format: paperpaw_platform::PixelFormat,
        room: Vec<u8>,
    ) -> bool {
        let align = self.frame.row_align;
        let mut room = Some(room);
        match with_fewer_rows(rows, align, |rows| {
            Framebuffer::banded_in(width, height, rows, format, room.take().unwrap_or_default())
        }) {
            Some(framebuffer) => {
                if framebuffer.rows() < rows {
                    let fewer = framebuffer.rows();
                    self.warn_runtime(format!(
                        "memory short: the frame is drawn in bands of {fewer} rows, not {rows}"
                    ));
                }
                self.framebuffer = look.on(framebuffer);
                true
            }
            None => {
                self.warn_runtime(format!("no memory for a frame of {width}x{height}"));
                false
            }
        }
    }

    /// Whether large pictures are on the screen now, and on which opening of a
    /// screen: its `big` pictures were drawn, or they stay while less than
    /// `most` of the screen changed.
    fn note_pictures(&mut self, big: bool, most: bool) {
        if big {
            self.frame.pictures_on = self.stack.last().map(|s| s.serial);
        }
        self.frame.pictures_shown = big || (self.frame.pictures_shown && !most);
    }

    /// Draws everything that changed since the last call and passes it to
    /// `out` as full-width rows of pixels, top to bottom: `out(top, rows)`
    /// with `top` the screen row of the first one. Call it after each
    /// [`start`](Kernel::start), [`handle`](Kernel::handle) or
    /// [`tick`](Kernel::tick) whose effects contain a refresh, then refresh
    /// the panel.
    pub fn render(&mut self, out: &mut dyn FnMut(u16, FrameView<'_>)) {
        let Some(region) = self.frame.unrendered.take() else {
            return;
        };
        self.shared
            .stack_rows
            .set_followup(self.frame.followup_frame);
        // The kernel's, but for the screens' own drawing.
        let _running = self.shared.meter.enter(meter::CORE);
        let bounds = self.framebuffer.bounds();
        // Rows are passed whole: drawn in bands, the rest of a row would be
        // left from another band. And aligned as the shell asks.
        let (narrow, rows) = self.widen_band(region);
        let region = if rows < bounds.height {
            Rect::new(0, region.y, bounds.width, region.height)
        } else {
            region
        };
        let align = self.frame.row_align;
        let first = region.y - region.y % align;
        let last = (region.bottom().div_ceil(u32::from(align)) * u32::from(align))
            .min(u32::from(bounds.height));
        let region = Rect::new(
            region.x,
            first,
            region.width,
            (last - u32::from(first)) as u16,
        );
        self.frame.rendered = Some(region);
        // A screenshot is written as the frame is drawn, a band at a time.
        let shot = self.frame.screenshot.take().filter(|_| region == bounds);
        // Why it could not be written, where it could not.
        let mut failed = None;
        let mut writer = shot.as_deref().and_then(|path| {
            paperpaw_sdk::image::PngWriter::new(
                self.shared.storage.as_ref(),
                path,
                u32::from(bounds.width),
                u32::from(bounds.height),
            )
            .map_err(|e| failed = Some(e))
            .ok()
        });
        let mut gray = Vec::new();
        let mut top = region.y - region.y % rows;
        while u32::from(top) < region.bottom() {
            self.framebuffer.set_top(top);
            if let Some(part) = region.intersection(&self.framebuffer.band()) {
                self.redraw(part);
                let view = self.framebuffer.view_rows(part);
                if let (Some(writer), Some(path)) = (writer.as_mut(), shot.as_deref()) {
                    for y in 0..view.height {
                        gray.clear();
                        gray.extend((0..view.width).map(|x| {
                            let c = view.pixel(x, y);
                            ((u32::from(c.r) * 77 + u32::from(c.g) * 150 + u32::from(c.b) * 29)
                                >> 8) as u8
                        }));
                        if let Err(e) = writer.row(self.shared.storage.as_ref(), path, &gray) {
                            failed = Some(e);
                            break;
                        }
                    }
                }
                self.pass_pieces(part, narrow, out);
                // A row not written: no more of them, a picture with a
                // gap is none.
                if failed.is_some() {
                    writer = None;
                }
            }
            top = top.saturating_add(rows);
            if top == u16::MAX {
                break;
            }
        }
        self.end_frame_narrow(narrow);
        self.shared.stack_rows.set_followup(None);
        // Large pictures on the screen now: the next change of most of it
        // refreshes in full.
        let pictured = self.framebuffer.take_pictured();
        let screen = u32::from(bounds.width) * u32::from(bounds.height);
        let big = pictured.saturating_mul(100) >= screen.saturating_mul(PICTURES_PERCENT);
        let most = u32::from(region.width) * u32::from(region.height) * 2 >= screen;
        self.note_pictures(big, most);
        let all = self.framebuffer.take_all_pictured();
        self.gray_seen(all, screen);
        if let Some(path) = shot {
            let saved = writer.is_some_and(|w| w.is_complete());
            if !saved {
                // What was written of it is no picture.
                let _ = self.shared.storage.remove(&path);
                let why =
                    failed.map_or_else(|| String::from("not all of it drawn"), |e| e.to_string());
                self.warn_runtime(format!("screenshot {path}: {why}"));
            }
            let key = match failed {
                _ if saved => "core.screenshot-saved",
                Some(paperpaw_sdk::image::ImageError::Storage(StorageError::Full)) => {
                    "core.screenshot-full"
                }
                Some(paperpaw_sdk::image::ImageError::Storage(_)) => "core.screenshot-unwritten",
                _ => "core.screenshot-failed",
            };
            let text = self
                .shared
                .translator
                .format(key, &[("path", paperpaw_sdk::Arg::Str(&path))]);
            self.output.commands.push(Command::Notify(text));
        }
        // Read once for all the bands; not kept past the frame.
        self.shared.recall.clear();
    }

    /// Draws again what [`Kernel::render`] drew last, passing it the same
    /// way: for panel controllers that compare a new frame with the one
    /// shown (the X4's), whose memory for the shown one is written after
    /// a refresh, so the shell keeps no frame of its own. Call it right
    /// after the refresh, before anything else changes the screen; with
    /// changes not rendered yet it does nothing.
    pub fn rerender(&mut self, out: &mut dyn FnMut(u16, FrameView<'_>)) {
        if self.frame.unrendered.is_none() {
            self.frame.unrendered = self.frame.rendered;
            self.render(out);
        }
    }

    /// Draws the whole screen anew and passes it to `out` as
    /// [`Kernel::render`] does, while nothing on it changes: for a shell
    /// that sends a screenshot elsewhere, e.g. over USB. While changes wait
    /// to be rendered it does nothing and returns `false`.
    pub fn capture(&mut self, out: &mut dyn FnMut(u16, FrameView<'_>)) -> bool {
        if self.frame.unrendered.is_some() {
            return false;
        }
        let rendered = self.frame.rendered;
        self.frame.unrendered = Some(self.framebuffer.bounds());
        self.render(out);
        // What the panel was last sent stays what rerender sends again.
        self.frame.rendered = rendered;
        true
    }

    /// Marks `region` to be drawn again, as much of it as is on the
    /// screen (an overlay may say it reaches past it).
    pub(crate) fn invalidate(&mut self, region: Rect) {
        let Some(region) = region.intersection(&self.framebuffer.bounds()) else {
            return;
        };
        self.frame.pending = Some(self.frame.pending.map_or(region, |p| p.union(&region)));
        self.frame.pending_weight = self
            .frame
            .pending_weight
            .saturating_add(u32::from(region.width) * u32::from(region.height));
    }

    /// Redraws `region`: the topmost full screen and every overlay above
    /// it, each clipped to the region.
    pub(crate) fn redraw(&mut self, region: Rect) {
        let regions: Vec<Option<Rect>> = (0..self.stack.len()).map(|i| self.overlay(i)).collect();
        // What each lets show through it.
        let through: Vec<ShowsThrough> = self
            .stack
            .iter()
            .map(|s| s.screen.shows_through())
            .collect();
        let env = self.shared.env(self.now);
        // Overlays that wash out what lies below them.
        let dims: Vec<bool> = self
            .stack
            .iter()
            .zip(&regions)
            .map(|(s, own)| own.is_some() && s.screen.dims_below())
            .collect();
        let full = regions.iter().rposition(Option::is_none).unwrap_or(0);
        let base = self.full_base();
        // The overlays one above shows the full screen through: not
        // drawn, covering nothing (a menu left open under a picture over
        // the page).
        let hidden: Vec<bool> = (0..regions.len())
            .map(|k| k > full && through[k + 1..].contains(&ShowsThrough::FullScreen))
            .collect();
        let row = self.hint_row();
        let mut canvas = self.framebuffer.canvas();
        let mut canvas = canvas.clipped(region);
        canvas.clear(Color::WHITE);
        for (index, (active, own)) in self.stack[base..]
            .iter_mut()
            .zip(&regions[base..])
            .enumerate()
        {
            let Some(mut area) = own.unwrap_or(active.area).intersection(&region) else {
                continue;
            };
            // Less what the overlays above it cover: overlays paint all
            // of their region, so what lies below would not show (e.g.
            // the home screen under a menu whose focus moved: only the
            // rows above the menu, as frames are drawn in whole rows). One
            // that shows the full screen through covers the overlays.
            let at = base + index;
            if hidden[at] {
                continue;
            }
            let mut above = (at + 1..regions.len())
                .filter(|&k| !hidden[k] && through[k] == ShowsThrough::Nothing)
                .filter_map(|k| regions[k]);
            let Some(shown) = above.try_fold(area, uncovered) else {
                continue;
            };
            area = shown;
            // In its own part of the screen, from its corner.
            let device = env.device_within(active.area);
            let own_env = env.within(&device, active.area);
            let (x, y) = (i32::from(active.area.x), i32::from(active.area.y));
            let _running = self.shared.meter.enter(active.id.namespace());
            self.shared
                .draw_counted(active, &mut canvas.clipped(area).offset(x, y), &own_env);
            if (at + 1..dims.len()).any(|k| dims[k] && !hidden[k]) {
                canvas.wash(area);
            }
        }
        if let Some(row) = row
            && let Some(area) = row.intersection(&region)
        {
            let pressed = self.frame.mark.and_then(|(_, cell)| cell);
            paperpaw_sdk::ui::draw_button_hints(
                &mut canvas.clipped(area),
                &env,
                row,
                &self.hint.shown,
                pressed,
            );
        }
        // The busy mark in a corner, where no hint shows it.
        if let Some((mark, None)) = self.frame.mark
            && mark.intersection(&region).is_some()
        {
            let style = paperpaw_sdk::ui::UiStyle::new(&env);
            let mut canvas = canvas.clipped(mark);
            paperpaw_sdk::ui::fill_round(&mut canvas, &style, mark, Color::BLACK);
            let size = f32::from(mark.height) * 0.6;
            let sign = paperpaw_sdk::icons::PAW;
            let x = f32::from(mark.x)
                + (f32::from(mark.width) - paperpaw_sdk::ui::icon_width(&env, sign, size)) / 2.0;
            paperpaw_sdk::ui::draw_icon(&mut canvas, &env, sign, size, x, mark, Color::WHITE);
        }
        // A message over everything.
        if let Some((notice, _)) = &mut self.notice
            && let Some(area) = screens::notice_area(notice, &env).intersection(&region)
        {
            let corner = (i32::from(env.content().x), i32::from(env.content().y));
            notice.draw(&mut canvas.clipped(area).offset(corner.0, corner.1), &env);
        }
        // What was drawn for the first time tells its subscribers (87).
        self.raise_screen_ready();
    }

    /// What the screens shown are, part by part (rows, buttons, headings
    /// with their text, a notice over them), as their drawing tells (see
    /// [`paperpaw_sdk::semantics`]): for reading the screen aloud,
    /// automation and tests. Draws them once, aside.
    pub fn describe(&mut self) -> Vec<paperpaw_sdk::semantics::Part> {
        paperpaw_sdk::semantics::parts(self.described())
    }

    /// What [`describe`](Self::describe) puts together: everything the
    /// drawing helpers told as they drew, in drawing order, each with the
    /// clip it was drawn in, e.g. for checks of text cut at a cell's edge.
    pub fn described(&mut self) -> Vec<paperpaw_sdk::semantics::Described> {
        let bounds = self.framebuffer.bounds();
        // Described, not drawn: no pixels kept.
        let mut aside = Framebuffer::blind(bounds.width, bounds.height);
        let mut described = Vec::new();
        self.draw_aside(&mut aside.canvas().describing(&mut described));
        described
    }

    /// Whether the screen as it is now has pictures enough to be shown in
    /// grays ([`PICTURES_GRAY_PERCENT`] of it): drawn aside, where only
    /// pictures are counted, nothing drawn and nothing read.
    pub(crate) fn pictures_drawn(&mut self) -> bool {
        let bounds = self.framebuffer.bounds();
        let mut aside = Framebuffer::blind(bounds.width, bounds.height);
        self.draw_aside(&mut aside.canvas());
        let screen = u32::from(bounds.width) * u32::from(bounds.height);
        aside.take_all_pictured().saturating_mul(100)
            >= screen.saturating_mul(PICTURES_GRAY_PERCENT)
    }

    /// Draws everything on the screen into `canvas`, aside: the screens
    /// from the last one seen whole, the chrome, the row of hints and a
    /// notice over them.
    fn draw_aside(&mut self, canvas: &mut paperpaw_sdk::Canvas<'_>) {
        let regions: Vec<Option<Rect>> = (0..self.stack.len()).map(|i| self.overlay(i)).collect();
        let row = self.hint_row();
        let env = self.shared.env(self.now);
        let base = self.full_base();
        {
            for (active, own) in self.stack[base..].iter_mut().zip(&regions[base..]) {
                let area = own.unwrap_or(active.area);
                let device = env.device_within(active.area);
                let own_env = env.within(&device, active.area);
                let (x, y) = (i32::from(active.area.x), i32::from(active.area.y));
                // As its plugin, as when drawn: it sees its own secrets.
                let _running = self.shared.meter.enter(active.id.namespace());
                active
                    .screen
                    .draw(&mut canvas.clipped(area).offset(x, y), &own_env);
            }
            if let Some(row) = row {
                paperpaw_sdk::ui::draw_button_hints(
                    &mut canvas.clipped(row),
                    &env,
                    row,
                    &self.hint.shown,
                    None,
                );
            }
            // A message over everything, last.
            if let Some((notice, _)) = &mut self.notice {
                let area = screens::notice_area(notice, &env);
                let corner = (i32::from(env.content().x), i32::from(env.content().y));
                notice.draw(&mut canvas.clipped(area).offset(corner.0, corner.1), &env);
            }
        }
    }

    /// What the screen shown tells as it draws (see
    /// [`described`](Self::described)), alone: without the screens below
    /// an overlay and the button hints, e.g. for checks that move the
    /// focus through its parts. Draws it once, aside.
    pub fn described_active(&mut self) -> Vec<paperpaw_sdk::semantics::Described> {
        let bounds = self.framebuffer.bounds();
        // Described, not drawn: no pixels kept.
        let mut aside = Framebuffer::blind(bounds.width, bounds.height);
        let Some(top) = self.stack.len().checked_sub(1) else {
            return Vec::new();
        };
        let area = self.region_of(top);
        let env = self.shared.env(self.now);
        let active = &mut self.stack[top];
        let device = env.device_within(active.area);
        let own_env = env.within(&device, active.area);
        let (x, y) = (i32::from(active.area.x), i32::from(active.area.y));
        let mut described = Vec::new();
        let _running = self.shared.meter.enter(active.id.namespace());
        active.screen.draw(
            &mut aside
                .canvas()
                .describing(&mut described)
                .clipped(area)
                .offset(x, y),
            &own_env,
        );
        described
    }

    /// Times what the input started, and shows the busy mark first if it
    /// was slow the last time (or is a screen not opened yet; see
    /// `Timing::slow`), holding the
    /// work back until [`Kernel::proceed`].
    pub(crate) fn mark_if_slow(&mut self) -> Option<Effects> {
        let key = self
            .output
            .commands
            .iter()
            .find_map(|command| self.work_key(command))?;
        let started = self.shared.meter.micros()?;
        let slow = self.timing.slow(&key);
        self.timing.running = Some((key, started));
        if !self.busy_mark || !slow {
            return None;
        }
        let mark = self.mark_area();
        self.invalidate(mark.0);
        self.frame.mark = Some(mark);
        self.withheld = mem::take(&mut self.output.commands);
        self.withheld_trusted = mem::take(&mut self.trusted);
        self.withheld_own = mem::take(&mut self.own_trusted);
        self.output.request_refresh(RefreshMode::Fast);
        let mut effects = self.finish();
        effects.proceed = true;
        Some(effects)
    }

    /// Where the busy mark goes: the pressed button's hint, or a corner.
    pub(crate) fn mark_area(&self) -> (Rect, Option<usize>) {
        if let (Some(row), Some(cell)) = (self.hint_row(), self.hint.pressed) {
            let count = self.hint.buttons.len().max(1) as u32;
            let width = u32::from(row.width);
            let left = u32::from(row.x) + width * cell as u32 / count;
            let right = u32::from(row.x) + width * (cell as u32 + 1) / count;
            let area = Rect::new(left as u16, row.y, (right - left) as u16, row.height);
            return (area, Some(cell));
        }
        let metrics = paperpaw_sdk::Metrics::new(self.shared.device.display.dpi);
        let bounds = self.framebuffer.bounds();
        let size = (metrics.mm(9) as u16).min(bounds.width / 4);
        (Rect::new(bounds.width - size, 0, size, size), None)
    }

    /// Draws the whole screen again, refreshed fully: after the shell
    /// showed something of its own on the panel (e.g. a test picture).
    pub fn draw_again(&mut self, now: Instant) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        self.apply(Command::Redraw(RefreshMode::Full));
        self.frame.again = true;
        self.finish()
    }

    /// Tells plugins the display's new size, the screens laid out for it
    /// ([`events::DISPLAY_CHANGED`]).
    fn display_changed(&mut self) {
        let panel = self.framebuffer.bounds();
        let class = paperpaw_sdk::ui::ScreenClass::of(&self.shared.env(self.now), panel);
        let landscape = panel.width > panel.height;
        let changed = events::DisplayChanged::new(panel.width, panel.height, class, landscape);
        self.output
            .commands
            .push(events::DISPLAY_CHANGED.emit(changed));
    }

    /// The screen has another size, e.g. turned (see
    /// [`Kernel::rotation`]): every open screen is made anew for it, with
    /// what it was opened with, and the whole screen is drawn again. Turned
    /// half way round, the size stays: the row of hints may come or go,
    /// and the whole screen is drawn again.
    pub fn resize(&mut self, now: Instant, width: u16, height: u16) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        let panel = self.framebuffer.bounds();
        let format = self.shared.device.display.format;
        if (panel.width, panel.height) == (width, height) {
            let row = self.hint.height;
            self.apply_hint_height();
            if self.hint.height == row {
                self.apply(Command::Redraw(RefreshMode::Full));
            }
        } else if width > 0 && height > 0 {
            // Bands take as much memory as before: fewer rows where the
            // screen got wider.
            let rows = match self.frame.band {
                Some(pixels) => {
                    let align = self.frame.row_align;
                    let rows = (pixels / u32::from(width)).min(u32::from(height)) as u16;
                    (rows / align * align).max(align).min(height)
                }
                None => height,
            };
            let look = Look::of(&self.framebuffer);
            let before = self.framebuffer.rows();
            // The old frame goes first, the new one taking its place: beside
            // it, it would split the largest free block where memory is
            // short.
            if let Ok(none) = Framebuffer::new(1, 1, format) {
                self.framebuffer = none;
            }
            let turned = self.restore_frame(look, width, height, rows, format, Vec::new());
            if turned {
                self.shared.device.display.width = width;
                self.shared.device.display.height = height;
                self.hint.height = self.wanted_hint_height();
                self.remake_screens();
                self.renew_memory_notice();
                self.display_changed();
            } else {
                self.restore_frame(look, panel.width, panel.height, before, format, Vec::new());
            }
        }
        self.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_platform::PixelFormat;

    /// Where memory is short, the frame is made in fewer rows, halved down
    /// to the rows the panel takes at once; without memory even for that,
    /// none.
    #[test]
    fn frames_are_made_in_fewer_rows_where_memory_is_short() {
        let fits = |most: u16| {
            move |rows: u16| {
                if rows <= most {
                    Framebuffer::banded(90, 100, rows, PixelFormat::Mono1)
                } else {
                    Err(GraphicsError::OutOfMemory { bytes: 0 })
                }
            }
        };
        let made = |most, rows, align| with_fewer_rows(rows, align, fits(most)).map(|f| f.rows());
        assert_eq!(made(100, 100, 8), Some(100));
        assert_eq!(made(30, 100, 8), Some(24));
        assert_eq!(made(8, 100, 8), Some(8));
        assert_eq!(made(7, 100, 8), None);
        assert_eq!(made(60, 100, 1), Some(50));
    }
}
