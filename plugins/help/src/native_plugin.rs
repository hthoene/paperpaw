//! The flat Hints plugin: the hints over a screen the first time it opens, and
//! *Show hints again*. The hints and the stack come from `core.ui`; views own
//! copies and fonts never outlive their callback.
use crate::layout::{self, ZoneHint};
use crate::seen;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Handled, Point, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        events::{Event, Incoming, kinds},
        fonts::{Font, FontRole},
        registrations,
        reply::Reply,
        settings::{self, SettingValue},
        snapshot,
        ui::{HintInput, Instance, Open, Param, Ui, UiCursor},
    },
};
use paperpaw_ui_kit::read_event;
use paperpaw_ui_kit::{
    Dialog, Messages, Paint, Screens, Style,
    mem::{SERVICE_BUFFER, buffer, push},
    paint,
    text::{self, copy},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/help_declarations.rs"));
}

const OVERLAY: u32 = 10;
const STACK: u32 = 2;
const READY: u32 = 3;

const OVERLAY_SCREEN: &str = "help.overlay";
const SHOW: &str = "help.show";
/// The two buttons of the box.
const GOT_IT: usize = 0;
const NO_MORE: usize = 1;
/// The most characters of a screen id a hint query takes.
const SCREEN_LIMIT: usize = 128;
/// The input of `core.ui` ops `stack`, `hints` and `params`: at most 256 bytes
/// (their `max_input`); replies keep a full service buffer (`max_reply`).
const QUERY_INPUT: usize = 256;

/// One row of the screen stack, as far as the hints care.
struct Row {
    screen: String,
    instance: Instance,
    setup: bool,
    waits: bool,
}

/// The hints over a screen.
#[derive(Debug)]
struct Overlay {
    screen: String,
    /// Hints with a zone (thousandths) and their translated text; the others' text.
    zones: Vec<(layout::Zone, String)>,
    lines: Vec<String>,
    focus: usize,
    /// That they were shown is kept: where it is not they would open again
    /// each time the screen shows, and closing them would never get past them.
    kept: bool,
    laid: Option<Laid>,
}

#[derive(Debug)]
struct Laid {
    zones: Vec<ZoneHint>,
    dialog: Dialog,
    style: Style,
}

#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "kept unboxed: a box would allocate infallibly in a callback; the list of views grows fallibly"
)]
enum Slot {
    Overlay(Overlay),
    Again,
}

/// The interface-7 Hints plugin.
#[derive(Debug, Default)]
pub struct NativeHelp {
    ui: Option<Ui>,
    screens: Screens<Slot>,
}

impl Plugin for NativeHelp {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // `core.ui` is opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let Some(event) = read_event!(incoming; kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenReady, kinds::ScreenClosed, kinds::StackChanged)
        else {
            return Ok(());
        };
        match event {
            Event::ScreenOpened {
                screen,
                instance,
                handover,
                ..
            } => self.opened(host, screen, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => self.shown(host, token, instance),
            Event::ScreenReady { screen, .. } if token == READY => self.explain(host, screen),
            Event::StackChanged { .. } if token == STACK => self.stacked(host),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        match self.screens.top(token).ok_or(Error::State)? {
            Slot::Overlay(overlay) => overlay.draw(host, &mut frame),
            Slot::Again => draw_again(host, &mut frame),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match self.screens.top(token) {
            Some(Slot::Overlay(overlay)) => overlay.input(host, &input),
            Some(Slot::Again) => again_input(host, &input),
            None => Ok(Handled::No),
        }
    }
}

impl NativeHelp {
    /// Opens `core.ui` in the first live callback; the handle serves later ones.
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        screen: &str,
        instance: Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        let slot = match screen {
            OVERLAY_SCREEN => {
                // The params' buffers are gone before the hints are read: one
                // pair of service buffers at a time (#248).
                let id = match handover {
                    Some(handover) => self.overlay_id(host, handover)?,
                    None => String::new(),
                };
                Slot::Overlay(Overlay::read(host, self.ui()?, id)?)
            }
            "help.again" => {
                // Done as it opens, and said.
                seen::again(host)?;
                Slot::Again
            }
            _ => return Err(Error::Invalid),
        };
        self.screens.open(instance, slot)
    }

    /// The screen id handed over to the hints overlay; empty without one.
    fn overlay_id(
        &self,
        host: &Host<'_>,
        handover: paperpaw_sdk::native::ui::HandoverId,
    ) -> Result<String, Error> {
        let mut input = buffer(QUERY_INPUT)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let mut id = String::new();
        for param in self.ui()?.params(host, handover, &mut input, &mut output)? {
            let param = param?;
            if param.key() == "id" {
                id = copy(param.value())?;
            }
        }
        Ok(id)
    }

    /// The rows of the screen stack, top first.
    fn stack(&self, host: &Host<'_>) -> Result<Vec<Row>, Error> {
        let mut input = buffer(QUERY_INPUT)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let mut rows = Vec::new();
        let mut cursor = UiCursor::start();
        loop {
            let page = self.ui()?.stack(host, cursor, &mut input, &mut output)?;
            let next = page.end().next();
            for row in page {
                let row = row?;
                push(
                    &mut rows,
                    Row {
                        screen: copy(row.screen())?,
                        instance: row.instance(),
                        setup: row.slot() == paperpaw_sdk::builtin::slots::SETUP.as_str(),
                        waits: row.waits_ready(),
                    },
                )?;
            }
            match next {
                Some(next) => cursor = next,
                None => return Ok(rows),
            }
        }
    }

    /// One of our own screens shown, by its token (`event.screen_shown` goes
    /// to a screen's own token only; another plugin's screens are met through
    /// `event.screen_ready` and `event.stack_changed`).
    fn shown(&mut self, host: &Host<'_>, token: u32, instance: Instance) -> Result<(), Error> {
        self.screens.shown(token, instance)?;
        if token == OVERLAY {
            // Seen as it shows: never again, unless asked for.
            if let Some(Slot::Overlay(overlay)) = self.screens.get(instance) {
                overlay.kept = seen::mark_seen(host, &overlay.screen)?;
            }
        }
        Ok(())
    }

    /// The stack changed: hints another screen covers close, and the screen now
    /// on top gets its hints as it comes back to the top (interface 6 opened
    /// them whenever a screen returned there), unless it waits until it is
    /// ready: `event.screen_ready` opens those.
    fn stacked(&mut self, host: &Host<'_>) -> Result<(), Error> {
        // Hints off and none open: nothing to close and nothing to open, so the
        // stack is not read and its buffers are not taken (#248).
        let open = self
            .screens
            .all()
            .any(|slot| matches!(slot, Slot::Overlay(_)));
        if !reads_stack(hints_on(host)?, open) {
            return Ok(());
        }
        let rows = self.stack(host)?;
        self.covered(host, &rows)?;
        match rows.first() {
            Some(top) if top.screen != OVERLAY_SCREEN && !top.waits => {
                self.explain_with(host, &top.screen, &rows)
            }
            _ => Ok(()),
        }
    }

    fn explain(&mut self, host: &Host<'_>, screen: &str) -> Result<(), Error> {
        // Hints off open nothing: the stack is not read (#248).
        if !hints_on(host)? {
            return Ok(());
        }
        let rows = self.stack(host)?;
        self.explain_with(host, screen, &rows)
    }

    /// Closes the hints that another screen now covers (a plugin's question, a
    /// notice opened over them; the stack changed): what they explain is hidden, so they show the
    /// next time instead.
    fn covered(&self, host: &Host<'_>, rows: &[Row]) -> Result<(), Error> {
        for (index, row) in rows.iter().enumerate() {
            // The top of the stack covers nothing: only hints below it are covered.
            if index == 0 || row.screen != OVERLAY_SCREEN {
                continue;
            }
            // The stack is listed from the top down: below is the next row.
            if let Some(below) = rows.get(index + 1) {
                seen::forget_seen(host, &below.screen)?;
            }
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).close(row.instance)?;
        }
        Ok(())
    }

    /// Opens the hints over `screen` where it has some, they are on, they were
    /// not shown yet and the first-run setup is not under way.
    fn explain_with(&self, host: &Host<'_>, screen: &str, rows: &[Row]) -> Result<(), Error> {
        if !hints_on(host)?
            // Hints are open already (or about to be): not twice.
            || rows.first().is_some_and(|row| row.screen == OVERLAY_SCREEN)
            || rows.iter().any(|row| row.setup)
            || screen.split('.').next() == Some("help")
            || !has_hints(host, self.ui()?, screen)?
            || seen::seen(host, screen)?
        {
            return Ok(());
        }
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let handover =
            self.ui()?
                .handover(host, &[Param::new("id", screen)?], &mut input, &mut output)?;
        CommandSender::new(host, &mut input).open(Open::screen(OVERLAY_SCREEN)?.handover(handover))
    }
}

/// Whether hints open by themselves (`help.show`; on unless set otherwise).
fn hints_on(host: &Host<'_>) -> Result<bool, Error> {
    let mut scratch = [0; 64];
    Ok(match settings::read_setting(host, SHOW, &mut scratch)? {
        SettingValue::Bool(on) => on,
        _ => true,
    })
}

/// Whether a changed stack is read: hints that are on may open, and hints
/// already open may be covered and must close. Otherwise it has nothing to do.
fn reads_stack(on: bool, overlay_open: bool) -> bool {
    on || overlay_open
}

/// The hints of `screen` that fit this device's input.
fn read_hints(
    host: &Host<'_>,
    ui: &Ui,
    screen: &str,
    mut each: impl FnMut(Option<layout::Zone>, &str) -> Result<(), Error>,
) -> Result<(), Error> {
    if screen.len() > SCREEN_LIMIT {
        return Err(Error::Invalid);
    }
    let touch = snapshot::capture_snapshot(host)?.has_touch();
    let mut input = buffer(QUERY_INPUT)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    let mut cursor = UiCursor::start();
    loop {
        let page = ui.hints(host, screen, cursor, &mut input, &mut output)?;
        let next = page.end().next();
        for hint in page {
            let hint = hint?;
            let fits = match hint.input() {
                HintInput::Touch => touch,
                HintInput::Buttons => !touch,
                _ => true,
            };
            if fits {
                let zone = hint
                    .zone()
                    .map(|zone| (zone.x, zone.y, zone.width, zone.height));
                each(zone, hint.text())?;
            }
        }
        match next {
            Some(next) => cursor = next,
            None => return Ok(()),
        }
    }
}

fn has_hints(host: &Host<'_>, ui: &Ui, screen: &str) -> Result<bool, Error> {
    let mut any = false;
    read_hints(host, ui, screen, |_, _| {
        any = true;
        Ok(())
    })?;
    Ok(any)
}

impl Overlay {
    fn read(host: &Host<'_>, ui: &Ui, screen: String) -> Result<Self, Error> {
        let mut messages = Messages::new()?;
        let mut zones = Vec::new();
        let mut lines = Vec::new();
        // The hint texts are keys of their owners' catalogs.
        let texts = {
            let mut keys = Vec::new();
            read_hints(host, ui, &screen, |zone, key| {
                push(&mut keys, (zone, copy(key)?))
            })?;
            keys
        };
        for (zone, key) in texts {
            let text = messages.copy(host, &key)?;
            match zone {
                Some(zone) => push(&mut zones, (zone, text))?,
                None => push(&mut lines, text)?,
            }
        }
        Ok(Self {
            screen,
            zones,
            lines,
            focus: GOT_IT,
            kept: true,
            laid: None,
        })
    }

    /// The layout, made on first use in a callback that has the font.
    fn lay_out(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.laid.is_some() {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let whole = Rect::new(0, 0, width, height);
        let mut zones = layout::zone_hints(whole, &self.zones)?;
        let small = paperpaw_ui_kit::geometry::line_px(&font, style.small)?;
        layout::lay_out_zones(
            &mut zones,
            (width, height),
            (style.gap, style.margin, small),
            &mut |text, room| {
                Ok(text::wrap_with(|l| font.measure_text(style.small, l), text, room, 64)?.len())
            },
        )?;
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "help.title")?;
        let buttons = [
            messages.copy(host, "help.got-it")?,
            messages.copy(host, "help.no-more")?,
        ];
        let mut texts: Vec<&str> = Vec::new();
        for line in &self.lines {
            texts.try_reserve(1).map_err(|_| Error::NoMemory)?;
            texts.push(line);
        }
        let labels = [buttons[0].as_str(), buttons[1].as_str()];
        let mut dialog = Dialog::new(&font, &style, (width, height), &title, &texts, &labels)?;
        let top = layout::dialog_top(&zones, dialog.panel(), height, style.margin);
        dialog.place(top)?;
        font.release()?;
        self.laid = Some(Laid {
            zones,
            dialog,
            style,
        });
        Ok(())
    }

    fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        self.lay_out(host)?;
        let laid = self.laid.as_ref().ok_or(Error::State)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = &laid.style;
        let mut ops = Vec::new();
        let line_w = u32::from(style.line).max(2) * 2;
        let small = paperpaw_ui_kit::geometry::line_px(&font, style.small)?;
        for hint in &laid.zones {
            paint::push(
                &mut ops,
                Paint::Focus(hint.frame, 0, Some(line_w), Color::BLACK),
            )?;
            let label = hint.label;
            paint::push(&mut ops, Paint::Fill(label, Color::WHITE))?;
            paint::push(
                &mut ops,
                Paint::Focus(
                    label,
                    style.corner,
                    Some(u32::from(style.line)),
                    Color::BLACK,
                ),
            )?;
            let width = f32::from(label.width.saturating_sub(style.gap.saturating_mul(2)));
            let wrapped =
                text::wrap_with(|l| font.measure_text(style.small, l), &hint.text, width, 64)?;
            let mut y = label.y.saturating_add(style.gap);
            for part in &wrapped {
                let wide = font.measure_text(style.small, part)?;
                let x = f32::from(label.x) + (f32::from(label.width) - wide).max(0.0) / 2.0;
                let row = Rect::new(label.x, y, label.width, small);
                text::label(
                    &mut ops,
                    &font,
                    part,
                    style.small,
                    x,
                    row,
                    Color::BLACK,
                    None,
                )?;
                y = y.saturating_add(small);
            }
        }
        laid.dialog.prepare(&mut ops, &font, Some(self.focus))?;
        paint::replay(frame, &font, ops.iter())?;
        font.release()
    }

    /// Closes, the screen's hints seen; `more` false switches all off, as does
    /// a storage that cannot keep that they were seen.
    fn close(&self, host: &Host<'_>, more: bool) -> Result<Handled, Error> {
        let mut scratch = [0; 128];
        let mut commands = CommandSender::new(host, &mut scratch);
        if !more || !self.kept {
            commands.set_setting(SHOW, SettingValue::Bool(false))?;
        }
        commands.back()?;
        Ok(Handled::Yes)
    }

    fn redraw(host: &Host<'_>) -> Result<Handled, Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Fast)?;
        Ok(Handled::Yes)
    }

    fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        self.lay_out(host)?;
        let laid = self.laid.as_mut().ok_or(Error::State)?;
        match *input {
            Input::Action { action, .. } => {
                if action == "core.confirm" {
                    return self.close(host, self.focus == GOT_IT);
                }
                match laid.dialog.on_action(action) {
                    paperpaw_ui_kit::dialog::Response::Moved => return Self::redraw(host),
                    paperpaw_ui_kit::dialog::Response::Held => return Ok(Handled::Yes),
                    paperpaw_ui_kit::dialog::Response::Ignored => {}
                }
                if let Some(focus) = laid.dialog.step(self.focus, action) {
                    self.focus = focus;
                    return Self::redraw(host);
                }
                // Back, and every other button: closed, the hints seen.
                self.close(host, true)
            }
            Input::Gesture { position, kind, .. } => match kind {
                InputGestureKind::Tap => {
                    let more = laid.dialog.hit(position) != Some(NO_MORE);
                    self.close(host, more)
                }
                InputGestureKind::SwipeUp | InputGestureKind::SwipeDown => {
                    let up = kind == InputGestureKind::SwipeUp;
                    match laid.dialog.on_swipe(Point::new(position.x, position.y), up) {
                        paperpaw_ui_kit::dialog::Response::Moved => Self::redraw(host),
                        _ => Ok(Handled::Yes),
                    }
                }
                _ => Ok(Handled::Yes),
            },
            _ => Ok(Handled::Yes),
        }
    }
}

/// *Show hints again*: the message that it was done.
fn draw_again(host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
    let display = snapshot::capture_snapshot(host)?;
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let style = Style::read(host, &display, &font)?;
    let area = Rect::new(0, 0, display.display_width(), display.display_height());
    let mut ops = Vec::new();
    paint::push(&mut ops, Paint::Fill(area, style.paper))?;
    let message = Messages::new()?.copy(host, "help.again-done")?;
    text::message(&mut ops, &font, &style, area, &message)?;
    paint::replay(frame, &font, ops.iter())?;
    font.release()
}

/// Any button or tap closes it.
fn again_input(host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
    let close = match *input {
        Input::Action { .. } => true,
        Input::Gesture { kind, .. } => kind == InputGestureKind::Tap,
        _ => false,
    };
    if close {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).back()?;
    }
    Ok(Handled::Yes)
}

#[cfg(test)]
mod tests {
    use super::reads_stack;

    #[test]
    fn a_changed_stack_is_read_only_where_hints_may_open_or_close() {
        assert!(reads_stack(true, false));
        assert!(reads_stack(true, true));
        // Off, but hints shown again or opened before they were switched off.
        assert!(reads_stack(false, true));
        // Off and none open: the 4 KiB buffers are never taken.
        assert!(!reads_stack(false, false));
    }
}
