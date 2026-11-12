//! The sleep screen as the interface-7 plugin shows it: what it reads when it
//! opens (the mode, the facts that decide what shows), the widgets it makes
//! and places when it first draws, and what it draws. The decisions are
//! [`crate::native_sleep`]'s and the layout [`crate::native_stack`]'s.
//!
//! The pictures (the cat, the logo, the cover, the folder's picture and the
//! picture over the page) are [`images`]'s: asked for from the events, since a
//! draw can send no command; until one is there the screen shows the message.
use crate::{
    native_sleep::{self as model, Content, Facts},
    native_stack::{self as stack, Entry},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose},
        commands::CommandSender,
        fonts::{Font, FontRole},
        image::ImageStyle,
        settings::{self, SettingValue},
        snapshot::{self, Snapshot},
        ui::{Compose, HandoverId, MadeWidget, Ui, UiCursor, WidgetHandle},
    },
};
use paperpaw_ui_kit::{
    Messages, Paint, Style,
    mem::{SERVICE_BUFFER, buffer, push},
    paint,
    text::{copy, label, wrap_with},
};

/// The places the widgets are made at (the declared places, in full).
const WIDGETS_PLACE: &str = "sleep.widgets";
const OVERLAY_PLACE: &str = "sleep.overlay";
/// The slot of the reader: the page the picture over the page may be used on.
const READER: &str = "core.reader";
/// The parameter of `core.sleep`: the device is switched off after the screen.
const OFF: &str = "off";
/// Most pages of the screen stack read for the page below.
const MOST_STACK_PAGES: usize = 8;
/// Room for the longest text setting and its frame: the most `settings.read`
/// takes (a bigger output buffer is `ERR_INVALID`).
const VALUE_BUFFER: usize = 4096;
/// Most lines the message takes.
const MOST_LINES: usize = 4;
/// The input of `core.ui` ops `stack`, `params` and `widget_drop`: at most 256
/// bytes (their `max_input`); replies keep what each op answers (`max_reply`).
const QUERY_INPUT: usize = 256;

mod images;

/// What is read of the settings for the picture over the page.
#[derive(Debug)]
struct OverSetup {
    /// The widget chosen; none: a picture goes over the page.
    widget: String,
    /// The picture's file chosen (`sleep.overlay-picture`); none: the folder's.
    picture: String,
    position: String,
    percent: u32,
    /// How opaque the picture is drawn (0 to 255).
    opacity: u8,
}

/// A widget made for the screen and where it goes.
#[derive(Debug)]
struct Placed {
    handle: WidgetHandle,
    rect: Rect,
}

/// What the first draw made of the screen.
#[derive(Debug)]
struct Laid {
    /// Nothing could be laid out: only the message.
    failed: bool,
    widgets: Vec<Placed>,
    /// The widget over the page and its panel.
    over: Option<Placed>,
}

/// One open sleep screen.
#[derive(Debug)]
pub struct Sleeping {
    /// `sleep.show`, as the setting has it.
    setting: String,
    /// The device is switched off after this screen, not put to sleep.
    off: bool,
    /// The device woke since the screen opened.
    pub woke: bool,
    /// The widgets of `sleep.widgets`, where they show.
    list: String,
    over: Option<OverSetup>,
    laid: Option<Laid>,
    /// What is known of what can be shown (see [`facts_before`]).
    facts: Facts,
    /// The pictures asked for and held.
    pics: images::Pics,
}

impl Sleeping {
    /// What shows when nothing could be read: the message.
    ///
    /// # Errors
    /// Never: nothing is allocated.
    pub fn plain(off: bool) -> Self {
        Self {
            setting: String::new(),
            off,
            woke: false,
            list: String::new(),
            over: None,
            laid: None,
            facts: Facts {
                over_page: false,
                picture: false,
                cover: false,
                ..facts_before(off)
            },
            pics: images::Pics::default(),
        }
    }

    /// Reads the parameters of the open, the settings and, for the picture
    /// over the page, the screen below.
    ///
    /// # Errors
    /// Allocation and the host's refusals.
    pub fn open(host: &Host<'_>, ui: &Ui, handover: Option<HandoverId>) -> Result<Self, Error> {
        let off = match handover {
            Some(handover) => off_param(host, ui, handover)?,
            None => false,
        };
        let mut view = Self::plain(off);
        view.setting = read_text(host, "sleep.show")?;
        view.facts = facts_before(off);
        let effective = model::effective(&view.setting, view.facts);
        if effective == model::WIDGETS {
            view.list = read_text(host, "sleep.widgets")?;
        }
        if effective == model::OVERLAY {
            let when = read_text(host, "sleep.overlay-when")?;
            let (guarded, reading) = page_below(host, ui)?;
            if stack::used(&when, guarded, reading) {
                view.over = Some(OverSetup {
                    widget: read_text(host, "sleep.overlay-widget")?,
                    picture: read_text(host, "sleep.overlay-picture")?,
                    position: read_text(host, "sleep.overlay-position")?,
                    percent: model::percent(read_int(host, "sleep.overlay-size", 35)?),
                    opacity: model::opacity(read_int(host, "sleep.overlay-see-through", 0)?),
                });
            }
            view.facts.over_page = view.over.is_some();
        }
        Ok(view)
    }

    /// The page stays as it is: nothing is drawn and no refresh asked for.
    #[must_use]
    pub fn quiet(&self) -> bool {
        self.setting == model::PAGE && !self.off
    }

    /// Asks for a full refresh, as interface 6 did each time the screen showed.
    ///
    /// # Errors
    /// The host's refusal of the command.
    pub fn refresh(&self, commands: &mut CommandSender<'_, '_>) -> Result<(), Error> {
        if self.quiet() {
            return Ok(());
        }
        match commands.queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Full) {
            // Not the screen on top (something opened above it as the device
            // woke): it is drawn when it shows again.
            Ok(()) | Err(Error::State) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Draws the screen: the page below stays for the modes that leave it.
    ///
    /// # Errors
    /// Allocation and the host's refusals.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) || self.quiet() {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        if self.laid.is_none() {
            let laid = match self.lay(host, ui, &display, &font, &style) {
                Ok(laid) => laid,
                // Whatever went wrong, the device still says it sleeps.
                Err(_) => Laid::message(),
            };
            self.laid = Some(laid);
        }
        let content = self.content();
        let held = self.held_for(content);
        let laid = self.laid.as_ref().ok_or(Error::State)?;
        let screen = Rect::new(0, 0, display.display_width(), display.display_height());
        let mut below = Vec::new();
        match content {
            Content::Overlay => {
                if let Some(over) = &laid.over {
                    push(&mut below, Paint::Fill(over.rect, style.paper))?;
                }
            }
            Content::Page => {}
            Content::Cat if held.is_some() => push(&mut below, Paint::Fill(screen, Color::WHITE))?,
            Content::Cover | Content::Picture if held.is_some() => {
                push(&mut below, Paint::Fill(screen, Color::BLACK))?;
            }
            _ => push(&mut below, Paint::Fill(screen, style.paper))?,
        }
        // The message where no picture shows; the logo above it where there is one.
        let message = content == Content::Message
            || held.is_none()
                && matches!(
                    content,
                    Content::Cat | Content::Cover | Content::Picture | Content::Other
                );
        let mut logo = None;
        if message {
            // PaperPaw's logo, a third of the screen across, above the message.
            let side = match held {
                Some(_) if content == Content::Message => screen.width.min(screen.height) / 3,
                _ => 0,
            };
            logo = self.message(host, &font, &style, screen, side, &mut below)?;
        }
        paint::replay(frame, &font, below.iter())?;
        if let Some(held) = held {
            let room = images::room(content, screen, style.margin);
            let (rect, picture) = match (content, logo) {
                (Content::Message, Some(rect)) => (rect, ImageStyle::new().small()),
                (Content::Overlay, _) => match self.over_rect(&font, &style, screen, held)? {
                    Some((rect, opacity)) => (rect, ImageStyle::new().transparency(255 - opacity)),
                    None => (Rect::new(0, 0, 0, 0), ImageStyle::new()),
                },
                _ => (images::centred(held, room), ImageStyle::new()),
            };
            // A picture the host refuses (an id it ended, no memory) leaves
            // the rest: the message, or the colour it is on.
            if rect.width > 0 && rect.height > 0 {
                frame
                    .draw_canvas()?
                    .draw_image(held.id, paint::frame_rect(rect), picture)
                    .ok();
            }
        }
        let all = laid.widgets.iter().chain(laid.over.iter());
        for placed in all {
            let rect = match content {
                Content::Overlay => stack::inside(placed.rect, pad(&style)),
                _ => placed.rect,
            };
            // A widget the host cannot compose is left out: the rest shows.
            if Ui::compose(
                host,
                Compose::Widget(placed.handle),
                paint::frame_rect(rect),
            )
            .is_err()
            {
                continue;
            }
        }
        Ok(())
    }

    /// The message in the middle, with room above for a logo `logo` pixels
    /// across (none: 0); where the logo goes.
    fn message(
        &self,
        host: &Host<'_>,
        font: &Font<'_>,
        style: &Style,
        screen: Rect,
        logo: u16,
        paint: &mut Vec<Paint>,
    ) -> Result<Option<Rect>, Error> {
        let mut messages = Messages::new()?;
        let text = messages.copy(
            host,
            if self.off {
                "sleep.off"
            } else {
                "sleep.sleeping"
            },
        )?;
        let room = f32::from(screen.width.saturating_sub(style.margin.saturating_mul(4))).max(1.0);
        let lines = wrap_with(
            |l| font.measure_text(style.text, l),
            &text,
            room,
            MOST_LINES,
        )?;
        let line = paperpaw_ui_kit::geometry::line_px(font, style.text)?;
        let hint = if self.off {
            wrap_with(
                |l| font.measure_text(style.small, l),
                &messages.copy(host, "sleep.off-hint")?,
                room,
                MOST_LINES,
            )?
        } else {
            Vec::new()
        };
        let small = paperpaw_ui_kit::geometry::line_px(font, style.small)?;
        // The logo a little above the middle, the message below it.
        let above = if logo == 0 {
            0
        } else {
            u32::from(logo) + u32::from(style.gap) * 2
        };
        let total = above
            + u32::from(line) * lines.len() as u32
            + u32::from(small) * hint.len() as u32
            + if hint.is_empty() {
                0
            } else {
                u32::from(style.gap)
            };
        let top = u32::from(screen.height).saturating_sub(total) / 2;
        let at = Rect::new(
            screen.width.saturating_sub(logo) / 2,
            top.min(u32::from(u16::MAX)) as u16,
            logo,
            logo,
        );
        let mut y = top.saturating_add(above);
        for (rows, size, high, color) in [
            (&lines, style.text, line, style.ink),
            (&hint, style.small, small, style.muted),
        ] {
            for row in rows {
                let wide = font.measure_text(size, row)?;
                let x = f32::from(screen.x) + (f32::from(screen.width) - wide).max(0.0) / 2.0;
                let area = Rect::new(
                    screen.x,
                    y.min(u32::from(u16::MAX)) as u16,
                    screen.width,
                    high,
                );
                label(paint, font, row, size, x, area, color, None)?;
                y = y.saturating_add(u32::from(high));
            }
            if rows.is_empty() {
                continue;
            }
            y = y.saturating_add(u32::from(style.gap));
        }
        Ok((logo > 0).then_some(at))
    }

    /// Makes and places what the mode shows, and decides what shows.
    fn lay(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        display: &Snapshot,
        font: &Font<'_>,
        style: &Style,
    ) -> Result<Laid, Error> {
        let screen = Rect::new(0, 0, display.display_width(), display.display_height());
        let mut over = None;
        // A widget over the page is made here; the picture over it was asked
        // for when the screen opened (`images`), and stands as it was found.
        let widget = self
            .over
            .as_ref()
            .filter(|setup| !setup.widget.trim().is_empty());
        if model::effective(&self.setting, self.facts) == model::OVERLAY
            && let Some(setup) = widget
        {
            over = self.make_over(host, ui, display, font, style, setup)?;
        }
        if widget.is_some() || self.over.is_none() {
            self.facts.over_page = over.is_some();
        }
        let mut widgets = Vec::new();
        if model::effective(&self.setting, self.facts) == model::WIDGETS {
            widgets = self.make_widgets(host, ui, screen, style)?;
        }
        Ok(Laid {
            failed: false,
            widgets,
            over,
        })
    }

    /// The widgets of the list, measured, stacked and placed.
    fn make_widgets(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        screen: Rect,
        style: &Style,
    ) -> Result<Vec<Placed>, Error> {
        let width = u32::from(screen.width.saturating_sub(style.margin.saturating_mul(2)));
        let mut handles: Vec<Option<WidgetHandle>> = Vec::new();
        let mut entries = Vec::new();
        let made = self.make_each(host, ui, width, &mut handles, &mut entries);
        let placed = made.and_then(|()| stack::stack(screen, style.margin, style.gap, &entries));
        let placed = match placed {
            Ok(placed) => placed,
            Err(error) => {
                drop_all(host, ui, handles.iter().flatten().copied());
                return Err(error);
            }
        };
        let mut found = Vec::new();
        let kept = found
            .try_reserve_exact(placed.len())
            .map_err(|_| Error::NoMemory);
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut result = kept;
        for (at, rect) in &placed {
            let Some(Some(handle)) = handles.get(*at).copied() else {
                continue;
            };
            if result.is_ok() {
                result = ui
                    .widget_place(host, handle, paint::frame_rect(*rect), &mut input, &mut out)
                    .map(|()| {
                        found.push(Placed {
                            handle,
                            rect: *rect,
                        })
                    });
            }
        }
        // The ones that do not fit, and all of them where one failed, go.
        let keep = |handle: &WidgetHandle| {
            result.is_ok() && found.iter().any(|placed| placed.handle == *handle)
        };
        drop_all(
            host,
            ui,
            handles.iter().flatten().copied().filter(|h| !keep(h)),
        );
        result.map(|()| found)
    }

    /// Makes and measures each widget of the list.
    fn make_each(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        width: u32,
        handles: &mut Vec<Option<WidgetHandle>>,
        entries: &mut Vec<Entry>,
    ) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        for id in stack::ids(&self.list) {
            if id == stack::FILL {
                entries.try_reserve(1).map_err(|_| Error::NoMemory)?;
                handles.try_reserve(1).map_err(|_| Error::NoMemory)?;
                entries.push(Entry::Fill);
                handles.push(None);
                continue;
            }
            let handle = match ui.widget_make(host, id, WIDGETS_PLACE, &mut input, &mut out) {
                Ok(MadeWidget::Widget(handle)) => handle,
                // One the plugin draws itself is not asked for.
                Ok(_) => continue,
                // A widget that is gone, or not for this screen: left out.
                Err(Error::NotFound | Error::Denied) => continue,
                Err(error) => return Err(error),
            };
            let height = match ui.widget_measure(host, handle, width, &mut input, &mut out) {
                Ok(size) => size.height().min(u32::from(u16::MAX)) as u16,
                Err(Error::NoMemory) => {
                    drop_all(host, ui, [handle]);
                    return Err(Error::NoMemory);
                }
                // One the host cannot measure has nothing to show.
                Err(_) => 0,
            };
            let room = entries.try_reserve(1).and(handles.try_reserve(1));
            if room.is_err() {
                drop_all(host, ui, [handle]);
                return Err(Error::NoMemory);
            }
            entries.push(Entry::Widget(height));
            handles.push(Some(handle));
        }
        Ok(())
    }

    /// The widget over the page on its panel, placed; `None` where it cannot
    /// be made (no widget chosen, the widget gone).
    fn make_over(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        display: &Snapshot,
        font: &Font<'_>,
        style: &Style,
        setup: &OverSetup,
    ) -> Result<Option<Placed>, Error> {
        let id = setup.widget.trim();
        if id.is_empty() {
            return Ok(None);
        }
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let handle = match ui.widget_make(host, id, OVERLAY_PLACE, &mut input, &mut out) {
            Ok(MadeWidget::Widget(handle)) => handle,
            Ok(_) => return Ok(None),
            Err(Error::NotFound | Error::Denied) => return Ok(None),
            Err(error) => return Err(error),
        };
        let panel = (|| {
            let screen = model::Screen {
                width: display.display_width(),
                height: display.display_height(),
                margin: style.margin,
                status: paperpaw_ui_kit::geometry::line_px(font, style.small)?
                    .saturating_add(style.gap),
            };
            let side = model::box_side(screen, setup.percent);
            let pad = pad(style);
            let inner = u32::from(side.saturating_sub(pad.saturating_mul(2)));
            let height = ui
                .widget_measure(host, handle, inner, &mut input, &mut out)?
                .height()
                .min(u32::from(u16::MAX)) as u16;
            let (x, y, w, h) = model::placed(
                screen,
                side,
                height.saturating_add(pad.saturating_mul(2)),
                false,
                &setup.position,
            );
            let rect = Rect::new(x, y, w, h);
            ui.widget_place(
                host,
                handle,
                paint::frame_rect(stack::inside(rect, pad)),
                &mut input,
                &mut out,
            )?;
            Ok::<_, Error>(rect)
        })();
        match panel {
            Ok(rect) => Ok(Some(Placed { handle, rect })),
            Err(error) => {
                drop_all(host, ui, [handle]);
                Err(error)
            }
        }
    }
}

impl Laid {
    /// Nothing but the message.
    fn message() -> Self {
        Self {
            failed: true,
            widgets: Vec::new(),
            over: None,
        }
    }
}

/// The padding of a widget's panel.
fn pad(style: &Style) -> u16 {
    style.gap.saturating_mul(2)
}

/// The facts there are before anything is found: all that could show is
/// taken to be there, and `images` clears what it finds missing. No other
/// plugin's screen (the host shows it).
fn facts_before(off: bool) -> Facts {
    Facts {
        off,
        over_page: true,
        other_screen: false,
        picture: true,
        cover: true,
        widgets_empty: false,
    }
}

/// Lets widgets go; a refusal is of no matter (the host drops them when the
/// screen closes).
fn drop_all(host: &Host<'_>, ui: &Ui, widgets: impl IntoIterator<Item = WidgetHandle>) {
    if let (Ok(mut input), Ok(mut out)) = (buffer(QUERY_INPUT), buffer(64)) {
        for widget in widgets {
            // A refusal to let one go leaves it to the host.
            if ui.widget_drop(host, widget, &mut input, &mut out).is_err() {
                continue;
            }
        }
    }
}

/// Whether the open says the device is switched off (`off` = `1`).
fn off_param(host: &Host<'_>, ui: &Ui, handover: HandoverId) -> Result<bool, Error> {
    let (mut input, mut out) = (buffer(QUERY_INPUT)?, buffer(SERVICE_BUFFER)?);
    let params = ui.params(host, handover, &mut input, &mut out)?;
    for param in params {
        let param = param?;
        if param.key() == OFF {
            return Ok(param.value() == "1");
        }
    }
    Ok(false)
}

/// Whether a screen guards the device, and whether the topmost full screen
/// shows the reader.
fn page_below(host: &Host<'_>, ui: &Ui) -> Result<(bool, bool), Error> {
    let (mut input, mut out) = (buffer(QUERY_INPUT)?, buffer(SERVICE_BUFFER)?);
    let (mut guarded, mut reading, mut found) = (false, false, false);
    let mut cursor = UiCursor::start();
    for _ in 0..MOST_STACK_PAGES {
        let page = ui.stack(host, cursor, &mut input, &mut out)?;
        let next = page.end().next();
        guarded |= page.end().has_hidden();
        for row in page {
            let row = row?;
            guarded |= row.guards();
            if !found && !row.is_overlay() {
                found = true;
                reading = row.slot() == READER;
            }
        }
        match next {
            Some(next) => cursor = next,
            None => break,
        }
    }
    Ok((guarded, reading))
}

/// A text setting.
///
/// # Errors
/// Allocation and the host's refusals.
pub fn read_text(host: &Host<'_>, key: &str) -> Result<String, Error> {
    let mut bytes = buffer(VALUE_BUFFER)?;
    match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Text(text) => copy(text),
        _ => Ok(String::new()),
    }
}

/// A number setting; `default` where the setting has none.
///
/// # Errors
/// Allocation and the host's refusals.
pub fn read_int(host: &Host<'_>, key: &str, default: i64) -> Result<i64, Error> {
    let mut bytes = [0; 64];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Int(value) => value,
        _ => default,
    })
}
