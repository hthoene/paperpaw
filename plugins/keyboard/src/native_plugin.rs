//! The keyboard as an interface-7 plugin: the dialog of text questions on the
//! slot `core.keyboard` (it reads the question and the starting text, types on
//! the keys, and answers through `core.ui`), and the screen that chooses the
//! layouts. Drawing goes through owned [`Paint`](paperpaw_ui_kit::paint::Paint) operations.
use crate::keys::Key;
use crate::layouts::{AUTO, Added, LAYOUTS, all_drawn, chosen_drawn};
use crate::native_layouts;
use crate::native_model::{
    Asked, Board, FIELD_ROW, FieldButton, Look, Move, Own, Pressed, known_shape, known_size,
};
use crate::native_paint::{self, Words};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        device::{Device, DeviceCapability},
        events::{AnswerOutcome, Answered, Event, Incoming, kinds},
        fonts::{Font, FontRole},
        formatting::Arg,
        registrations,
        reply::Reply,
        settings::SettingValue,
        snapshot,
        ui::{
            Answer, Instance, Notify, Question, QuestionKind, RequestId, ScreenPolicy, TextMode, Ui,
        },
    },
};
use paperpaw_ui_kit::read_event;
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens, Style, geometry,
    mem::{SERVICE_BUFFER, buffer, push, wipe_text},
    paint,
    style::setting_text,
    text::copy,
    uitext,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/keyboard_declarations.rs"));
}

/// Screen tokens, as declared.
pub const SCREEN: u32 = 10;
/// The screen that chooses the layouts.
pub const CHOOSE: u32 = 11;
/// The action that finishes typing.
pub const DONE: u32 = 1;

const LAYOUTS_KEY: &str = "keyboard.layouts";
const UMLAUT_KEY: &str = "keyboard.umlaut-keys";
const SHAPE_KEY: &str = "keyboard.key-shape";
const SIZE_KEY: &str = "keyboard.size";
const CAPITALS_KEY: &str = "keyboard.auto-capitals";
const LANGUAGE_KEY: &str = "core.language";
const SCREEN_ID: &str = "keyboard.screen";
const REMOTE: &str = "core.remote-keyboard";
/// A host text, in bytes, at most.
const TEXT_LIMIT: usize = 24_576;
/// What passes the keyboard while a password is typed.
const PASSING: [&str; 3] = [
    "core.sleep",
    "core.refresh-screen",
    "core.toggle-night-mode",
];

/// A text question being answered.
#[derive(Debug)]
struct Dialog {
    request: RequestId,
    board: Board,
    style: Style,
    shape: String,
    /// The phone question asked from here, if one is open.
    nested: Option<RequestId>,
    /// Typing updates are still taken by the host.
    live: bool,
    /// Where the last frame's region began on the screen.
    origin: (i32, i32),
}

impl Dialog {
    /// Zeroes a secret text before it is freed.
    fn wipe(&mut self) {
        let text = core::mem::take(&mut self.board.text);
        if self.board.asked.secret {
            wipe_text(text);
        }
    }
}

/// Choosing the layouts.
#[derive(Debug, Default)]
struct ChooseView {
    /// The rows' ids: *Automatic*, then the built-in layouts.
    rows: Vec<String>,
    /// The chosen ids, in the order of switching.
    chosen: Vec<String>,
    list: ItemList,
}

#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "boxing would allocate infallibly; at most a few instances are open"
)]
enum View {
    Dialog(Dialog),
    Choose(ChooseView),
}

/// The interface-7 keyboard plugin.
#[derive(Debug, Default)]
pub struct NativeKeyboard {
    ui: Option<Ui>,
    screens: Screens<View>,
    /// The layout in use; the next keyboard opens with it.
    current: usize,
}

impl Plugin for NativeKeyboard {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::STAGE_SPECS)
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
        let Some(event) = read_event!(incoming; kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::ActionRun, kinds::Answered, kinds::TextTyped)
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
            } => self.screens.shown(token, instance),
            Event::ScreenClosed { instance, .. } => {
                if let Some(View::Dialog(dialog)) = self.screens.get(instance) {
                    dialog.wipe();
                }
                self.screens.close(instance);
                Ok(())
            }
            Event::ActionRun { .. } if token == DONE => self.finish_top(host),
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::TextTyped { text, done, .. } => self.text_typed(host, text.as_str(), done),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let view = self.screens.top(token).ok_or(Error::State)?;
        match view {
            View::Dialog(d) => {
                d.origin = (frame.rect.x, frame.rect.y);
                // Nothing about the parts is queued where nobody asks.
                d.style.describing = paint::asking(&mut frame);
                let font = Font::acquire_role(host, FontRole::Ui)?;
                let words = words(host, &d.board)?;
                let mut ops = Vec::new();
                native_paint::paint(&d.board, &d.style, &d.shape, &font, &words, &mut ops)?;
                paint::replay(&mut frame, &font, &ops)
            }
            View::Choose(v) => {
                let mut messages = Messages::new()?;
                let title = messages.copy(host, "keyboard.setting.layouts")?;
                v.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)
            }
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        match token {
            SCREEN => self.dialog_input(host, &input),
            CHOOSE => self.choose_input(host, &input),
            _ => Ok(Handled::No),
        }
    }
}

impl NativeKeyboard {
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    fn redraw(host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        screen: &str,
        instance: Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        let view = if screen == SCREEN_ID {
            View::Dialog(self.open_dialog(host, handover)?)
        } else {
            View::Choose(build_choose(host)?)
        };
        // A secret question: edges wait and nothing is told about the focus.
        if let View::Dialog(d) = &view
            && d.board.asked.secret
        {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch)
                .screen_policy(Some(instance), ScreenPolicy::new().secret().holds_edges())?;
        }
        self.screens.open(instance, view)
    }

    /// Reads the question the host opened this dialog for, and its text.
    fn open_dialog(
        &self,
        host: &Host<'_>,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<Dialog, Error> {
        let ui = self.ui()?;
        let handover = handover.ok_or(Error::Invalid)?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let mut request = None;
        for param in ui.params(host, handover, &mut input, &mut output)? {
            if let Some(id) = param?.request()? {
                request = Some(id);
            }
        }
        let request = request.ok_or(Error::Invalid)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let question = ui.question(host, request, &mut input, &mut output)?;
        if question.kind() != QuestionKind::Text {
            return Err(Error::Invalid);
        }
        let mode = question.mode();
        let asked = Asked {
            secret: question.is_secret(),
            multiline: question.is_multiline(),
            number: mode == TextMode::Number,
            plain: !question.is_secret() && mode == TextMode::Plain,
            live: question.is_live(),
        };
        let title = copy(question.title())?;
        let limit = (question.max_bytes() as usize).min(TEXT_LIMIT);
        // The text to start from; a question that never wrote one has none.
        let text = match uitext::read_all(ui, host, request.text(), limit) {
            Ok(text) => text,
            Err(Error::State) => String::new(),
            Err(error) => return Err(error),
        };
        let (mut dialog, ()) = (self.new_dialog(host, request, asked)?, ());
        dialog.board.title = title;
        dialog.board.text = text;
        dialog.board.shift = dialog.board.auto_shift();
        Ok(dialog)
    }

    fn new_dialog(
        &self,
        host: &Host<'_>,
        request: RequestId,
        asked: Asked,
    ) -> Result<Dialog, Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let look = Look {
            width: display.display_width(),
            height: display.display_height(),
            dpi: display.display_dpi(),
            touch: display.has_touch(),
            text: style.text,
            small: style.small,
            row: style.row,
            margin: style.margin,
            gap: style.gap,
            line: style.line,
            m_width: font.measure_text(100.0, "M")? / 100.0,
            text_line: geometry::line_px(&font, style.text)?,
            status: geometry::line_px(&font, style.small)?.saturating_add(style.gap / 2),
        };
        let mut messages = Messages::new()?;
        let shape = setting_text(host, SHAPE_KEY)?;
        let size = setting_text(host, SIZE_KEY)?;
        let own = Own {
            shape: if known_shape(&shape) {
                &shape
            } else {
                "keyboard.key-shape.round"
            },
            size: if known_size(&size) {
                &size
            } else {
                "keyboard.size.medium"
            },
            auto_capitals: read_bool(host, CAPITALS_KEY)?,
        };
        let language = setting_text(host, LANGUAGE_KEY)?;
        let rows = messages.copy(host, "keyboard.letters")?;
        let layouts = chosen_drawn(
            &language,
            &rows,
            &setting_text(host, LAYOUTS_KEY)?,
            read_bool(host, UMLAUT_KEY)?,
            &added_layouts(host, &language)?,
            &|text| covered(Some(&font), text),
        )?;
        // *Phone* where there is a network and a screen to type on another
        // device, *Paste* when the clipboard has text.
        let mut kinds: Vec<(FieldButton, f32)> = Vec::new();
        if !asked.secret && self.has_network(host)? && self.remote_keyboard(host)? {
            let label = messages.copy(host, FieldButton::Phone.label())?;
            push(
                &mut kinds,
                (FieldButton::Phone, font.measure_text(style.small, &label)?),
            )?;
        }
        if self.clipboard_has_text(host)? {
            let label = messages.copy(host, FieldButton::Paste.label())?;
            push(
                &mut kinds,
                (FieldButton::Paste, font.measure_text(style.small, &label)?),
            )?;
        }
        let board = Board::new(look, own, asked, layouts, self.current, &kinds)?;
        Ok(Dialog {
            request,
            board,
            style,
            shape: copy(own.shape)?,
            nested: None,
            live: asked.live,
            origin: (0, 0),
        })
    }

    /// Whether the device has a network.
    fn has_network(&self, host: &Host<'_>) -> Result<bool, Error> {
        let device = Device::open(host)?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let found = device
            .device(host, &mut input, &mut output)
            .map(|identity| identity.supports(DeviceCapability::Network));
        device.release(host)?;
        found
    }

    /// Whether a screen fills the slot `core.remote-keyboard`.
    fn remote_keyboard(&self, host: &Host<'_>) -> Result<bool, Error> {
        let registry = paperpaw_sdk::native::registry::Registry::open(host)?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let mut found = false;
        let cursor = paperpaw_sdk::native::registry::PageCursor::start();
        let page = registry.slot_offers(host, REMOTE, cursor, &mut input, &mut output);
        if let Ok(page) = &page {
            found = page.rows().next().is_some();
        }
        registry.release(host)?;
        page.map(|_| found)
    }

    fn clipboard_has_text(&self, host: &Host<'_>) -> Result<bool, Error> {
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        match self.ui()?.clipboard_read(host, 0, &mut input, &mut output) {
            Ok(part) => Ok(part.total() > 0),
            // No card, or the card was pulled: nothing to paste.
            Err(Error::Io) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn clipboard_text(&self, host: &Host<'_>) -> Result<String, Error> {
        let ui = self.ui()?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let mut text = String::new();
        let mut offset = 0usize;
        loop {
            let at = u32::try_from(offset).map_err(|_| Error::NoSpace)?;
            let part = ui.clipboard_read(host, at, &mut input, &mut output)?;
            let total = part.total() as usize;
            if total > 4096 {
                return Err(Error::NoSpace);
            }
            let data = part.data();
            text.try_reserve(data.len()).map_err(|_| Error::NoMemory)?;
            text.push_str(data);
            offset += data.len();
            if offset >= total {
                return Ok(text);
            }
            if data.is_empty() {
                return Err(Error::Invalid);
            }
        }
    }

    fn dialog_mut(&mut self) -> Option<&mut Dialog> {
        match self.screens.top(SCREEN)? {
            View::Dialog(d) => Some(d),
            View::Choose(_) => None,
        }
    }

    /// What the dialog's input means.
    fn dialog_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(secret) = self.dialog_mut().map(|d| d.board.asked.secret) else {
            return Ok(Handled::No);
        };
        match *input {
            Input::Action { now_ms, action } => self.dialog_action(host, now_ms, action, secret),
            Input::Gesture {
                now_ms,
                position,
                kind,
            } => self.dialog_gesture(host, now_ms, position, kind),
            _ => Ok(Handled::No),
        }
    }

    fn dialog_action(
        &mut self,
        host: &Host<'_>,
        now: u64,
        action: &str,
        secret: bool,
    ) -> Result<Handled, Error> {
        let Some(d) = self.dialog_mut() else {
            return Ok(Handled::No);
        };
        let held = action == "core.long-confirm" || action == "core.menu";
        if d.board.popup.is_some() {
            return self.variants_action(host, now, action, held);
        }
        if action == "core.confirm" {
            return match d.board.focus {
                Some((FIELD_ROW, index)) => {
                    let kind = d.board.buttons.get(index).map(|b| b.0);
                    if let Some(kind) = kind {
                        self.field_button(host, kind)?;
                    }
                    Ok(Handled::Yes)
                }
                Some((row, index)) => {
                    let key = d.board.key(row, index);
                    if let Some(key) = key {
                        self.press(host, key, now)?;
                    }
                    Ok(Handled::Yes)
                }
                None => {
                    d.board.move_focus(Move::Down);
                    d.board.focus = Some((0, 0));
                    Self::redraw(host)?;
                    Ok(Handled::Yes)
                }
            };
        }
        if held {
            // A long press on confirm: a letter's capital and variants, else done.
            let focus = d.board.focus;
            let opened = match focus {
                Some((row, index)) if row != FIELD_ROW => d.board.open_variants(row, index)?,
                _ => false,
            };
            if opened {
                Self::redraw(host)?;
            } else {
                self.finish_top(host)?;
            }
            return Ok(Handled::Yes);
        }
        if action == "core.back" {
            return self.cancel(host).map(|()| Handled::Yes);
        }
        if let Some(go) = Move::of(action) {
            // The key the focus leaves and the one it reaches, as on
            // interface 6, not the screen.
            match d.board.move_focus(go) {
                Some((from, to)) => {
                    let areas = [Some(d.board.focus_area(from)), Some(d.board.focus_area(to))];
                    paperpaw_ui_kit::list::redraw_areas(host, d.origin, areas)?;
                }
                None => Self::redraw(host)?,
            }
            return Ok(Handled::Yes);
        }
        // Typing a password, nothing else runs (see the module's top).
        if secret && !PASSING.contains(&action) {
            return Ok(Handled::Yes);
        }
        Ok(Handled::No)
    }

    /// Buttons while the variants are shown.
    fn variants_action(
        &mut self,
        host: &Host<'_>,
        now: u64,
        action: &str,
        held: bool,
    ) -> Result<Handled, Error> {
        let Some(d) = self.dialog_mut() else {
            return Ok(Handled::No);
        };
        if action == "core.confirm" {
            let chosen = d
                .board
                .popup
                .as_ref()
                .and_then(|p| p.chars.get(p.focus.unwrap_or(0)).copied());
            d.board.popup = None;
            if let Some(c) = chosen {
                let mut buffer = [0u8; 4];
                let pressed = d.board.insert(c.encode_utf8(&mut buffer), true)?;
                return self.typed(host, pressed).map(|()| Handled::Yes);
            }
        } else if action == "core.back" || held {
            d.board.popup = None;
        } else if let Some(go) = Move::of(action) {
            d.board.variants_move(go);
        }
        let _ = now;
        Self::redraw(host)?;
        Ok(Handled::Yes)
    }

    fn dialog_gesture(
        &mut self,
        host: &Host<'_>,
        now: u64,
        position: Point,
        kind: InputGestureKind,
    ) -> Result<Handled, Error> {
        let Some(d) = self.dialog_mut() else {
            return Ok(Handled::No);
        };
        if !d.board.region.contains(position) {
            return Ok(Handled::No);
        }
        if let Some(popup) = &d.board.popup {
            // A variant, or anywhere else to close them.
            let chosen = (0..popup.chars.len())
                .find(|&i| popup.key(i).contains(position))
                .map(|i| popup.chars[i]);
            d.board.popup = None;
            if let Some(c) = chosen.filter(|_| kind == InputGestureKind::Tap) {
                let mut buffer = [0u8; 4];
                let pressed = d.board.insert(c.encode_utf8(&mut buffer), true)?;
                self.typed(host, pressed)?;
            } else {
                Self::redraw(host)?;
            }
            return Ok(Handled::Yes);
        }
        let key = d.board.key_at(position);
        d.board.focus = None;
        let in_field = d.board.field.contains(position);
        let has_paste = d.board.buttons.iter().any(|b| b.0 == FieldButton::Paste);
        match (kind, key) {
            (InputGestureKind::Tap, Some((FIELD_ROW, index))) => {
                if let Some(&(button, _)) = d.board.buttons.get(index) {
                    self.field_button(host, button)?;
                }
            }
            // A long press in the text field pastes too, as elsewhere.
            (InputGestureKind::LongPress, _) if in_field && has_paste => {
                self.field_button(host, FieldButton::Paste)?;
            }
            (InputGestureKind::Tap, Some((row, index))) => {
                if let Some(key) = d.board.key(row, index) {
                    self.press(host, key, now)?;
                }
            }
            // A long press on *Delete* clears the text.
            (InputGestureKind::LongPress, Some((row, index)))
                if row != FIELD_ROW && d.board.key(row, index) == Some(Key::Delete) =>
            {
                d.board.clear();
                self.typed(host, Pressed::Changed)?;
            }
            // A long press on a letter shows its variants.
            (InputGestureKind::LongPress, Some((row, index))) if row != FIELD_ROW => {
                d.board.open_variants(row, index)?;
                Self::redraw(host)?;
            }
            _ => Self::redraw(host)?,
        }
        Ok(Handled::Yes)
    }

    fn field_button(&mut self, host: &Host<'_>, kind: FieldButton) -> Result<(), Error> {
        match kind {
            FieldButton::Paste => {
                let text = self.clipboard_text(host)?;
                let Some(d) = self.dialog_mut() else {
                    return Ok(());
                };
                let pressed = d.board.pasted(&text)?;
                self.typed(host, pressed)
            }
            FieldButton::Phone => self.ask_phone(host),
        }
    }

    /// Asks for the text on another device: the text so far goes along, but
    /// never a password.
    fn ask_phone(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(d) = self.dialog_mut() else {
            return Ok(());
        };
        if d.board.asked.secret || d.nested.is_some() {
            return Ok(());
        }
        let title = copy(&d.board.title)?;
        let text = copy(&d.board.text)?;
        let multiline = d.board.asked.multiline;
        let mut question = Question::text(&title, "")?.remote()?;
        if multiline {
            question = question.multiline()?;
        }
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = self.ui()?.ask(host, question, &mut input, &mut output)?;
        // A refusal here ends this callback in failure, which cancels the
        // request before any dialog opens (`docs/abi/ui.md`, *A failed start
        // cancels*): it is never recorded as open, and its `not_started`
        // answer is ignored in `answered`.
        uitext::write_all(self.ui()?, host, request.text(), &text)?;
        if let Some(d) = self.dialog_mut() {
            d.nested = Some(request);
        }
        Ok(())
    }

    /// Presses `key`.
    fn press(&mut self, host: &Host<'_>, key: Key, now: u64) -> Result<(), Error> {
        let Some(d) = self.dialog_mut() else {
            return Ok(());
        };
        let pressed = d.board.press(key, now)?;
        // The layout in use is kept for the next keyboard.
        let current = d.board.current;
        self.current = current;
        if pressed == Pressed::Done {
            return self.finish_top(host);
        }
        self.typed(host, pressed)
    }

    /// Text typed from outside (a hardware keyboard, a remote control): added
    /// as if typed on the keys, without what is no text; `done` confirms the
    /// dialog. Sent only while a text question is on top; another screen
    /// takes none.
    fn text_typed(&mut self, host: &Host<'_>, text: &str, done: bool) -> Result<(), Error> {
        let Some(d) = self.dialog_mut() else {
            return Ok(());
        };
        let pressed = d.board.typed(text)?;
        self.typed(host, pressed)?;
        if done {
            return self.finish_top(host);
        }
        Ok(())
    }

    /// The text or the keys changed: shows it, and tells the asker where
    /// it listens.
    fn typed(&mut self, host: &Host<'_>, pressed: Pressed) -> Result<(), Error> {
        if pressed == Pressed::Full {
            let max = self.dialog_mut().map_or(0, |d| d.board.max_chars());
            let mut messages = Messages::new()?;
            let text = messages.format(
                host,
                "keyboard.full",
                &[Arg::Int {
                    name: "count",
                    value: max as i64,
                }],
            )?;
            let mut scratch = buffer(1024)?;
            CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)?;
        }
        Self::redraw(host)?;
        if matches!(pressed, Pressed::Changed | Pressed::Full) {
            self.live(host)?;
        }
        Ok(())
    }

    /// The text so far, to an asker that listens.
    fn live(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let ui = self.ui.take().ok_or(Error::State)?;
        let result = (|| {
            let Some(View::Dialog(d)) = self.screens.top(SCREEN) else {
                return Ok(());
            };
            if !d.live {
                return Ok(());
            }
            match answer_text(&ui, host, d.request, &d.board.text, Answer::Typing) {
                // The live text stopped there (X4): the final answer is whole.
                Err(Error::NoSpace | Error::State) => {
                    d.live = false;
                    Ok(())
                }
                other => other,
            }
        })();
        self.ui = Some(ui);
        result
    }

    /// Closes the keyboard and sends the text.
    fn finish_top(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let ui = self.ui.take().ok_or(Error::State)?;
        let result = (|| {
            let Some(View::Dialog(d)) = self.screens.top(SCREEN) else {
                return Ok(());
            };
            d.board.popup = None;
            let sent = answer_text(&ui, host, d.request, &d.board.text, Answer::Accept);
            // Closed whatever the answer: a request the host ended is not ours.
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).back()?;
            d.wipe();
            match sent {
                Err(Error::State) => Ok(()),
                other => other,
            }
        })();
        self.ui = Some(ui);
        result
    }

    /// Closes the keyboard; nothing is answered with a text.
    fn cancel(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let ui = self.ui.take().ok_or(Error::State)?;
        let result = (|| {
            let Some(View::Dialog(d)) = self.screens.top(SCREEN) else {
                return Ok(());
            };
            let mut input = buffer(SERVICE_BUFFER)?;
            let mut output = buffer(SERVICE_BUFFER)?;
            let sent = ui.answer(host, d.request, Answer::Cancel, &mut input, &mut output);
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).back()?;
            d.wipe();
            match sent {
                Err(Error::State) => Ok(()),
                other => other,
            }
        })();
        self.ui = Some(ui);
        result
    }

    /// The text typed on the phone replaces the text so far.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some(d) = self.dialog_mut() else {
            return Ok(());
        };
        if d.nested != Some(answer.request()) || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        d.nested = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let text = uitext::read_all(self.ui()?, host, answer.request().text(), TEXT_LIMIT)?;
        let Some(d) = self.dialog_mut() else {
            return Ok(());
        };
        // The text so far stays until the new one is in.
        let pressed = d.board.replace_pasted(&text)?;
        self.typed(host, pressed)
    }

    fn choose_input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let Some(View::Choose(v)) = self.screens.top(CHOOSE) else {
            return Ok(Handled::No);
        };
        let step = v.list.input(host, input)?;
        if let ItemStep::Chosen(index) = step {
            toggle(host, v, index)?;
        }
        Ok(ItemList::handled(step))
    }
}

/// The words the keyboard shows.
fn words(host: &Host<'_>, board: &Board) -> Result<Words, Error> {
    let mut messages = Messages::new()?;
    let hold_for = match board.held_letters() {
        Some(letters) => Some(messages.format(
            host,
            "keyboard.hold-for",
            &[Arg::Text {
                name: "letters",
                value: &letters,
            }],
        )?),
        None => None,
    };
    Ok(Words {
        letters_key: messages.copy(host, "keyboard.letters-key")?,
        symbols_key: messages.copy(host, "keyboard.symbols-key")?,
        done: messages.copy(host, "keyboard.done")?,
        phone: messages.copy(host, "keyboard.phone")?,
        paste: messages.copy(host, "keyboard.paste")?,
        hold_back: messages.copy(host, "keyboard.hold-back")?,
        hold_for,
    })
}

fn read_bool(host: &Host<'_>, key: &str) -> Result<bool, Error> {
    let mut bytes = [0; 32];
    Ok(matches!(
        paperpaw_sdk::native::settings::read_setting(host, key, &mut bytes)?,
        SettingValue::Bool(true)
    ))
}

/// Writes the answer text and commits it with `how`.
fn answer_text(
    ui: &Ui,
    host: &Host<'_>,
    request: RequestId,
    text: &str,
    how: Answer,
) -> Result<(), Error> {
    uitext::write_all(ui, host, request.text(), text)?;
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    ui.answer(host, request, how, &mut input, &mut output)
}

/// The layouts other plugins add, by the stage `keyboard.layouts`. Where the
/// host does not run the stage (or nobody subscribed) there are none: the
/// built-in layouts work without it.
fn added_layouts(host: &Host<'_>, language: &str) -> Result<Vec<Added>, Error> {
    match native_layouts::collect(host, language) {
        Ok(added) => Ok(added),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(Vec::new()),
    }
}

/// Whether the font draws all of `text` (`text.covers`). Where the host cannot
/// say (a host without the slot, no font), it is taken as drawn: every layout
/// stays offered, as before the query existed.
fn covered(font: Option<&Font<'_>>, text: &str) -> bool {
    match font.map(|font| font.covers(text)) {
        Some(Ok(length)) => length == text.len(),
        _ => true,
    }
}

/// The rows of the layout chooser and its items, from the setting.
fn build_choose(host: &Host<'_>) -> Result<ChooseView, Error> {
    let font = Font::acquire_role(host, FontRole::Ui).ok();
    let drawn = |text: &str| covered(font.as_ref(), text);
    let mut rows: Vec<String> = Vec::new();
    push(&mut rows, copy(AUTO)?)?;
    // Only layouts the fonts can draw are offered.
    for layout in LAYOUTS.iter().filter(|l| all_drawn(&drawn, l.letters)) {
        push(&mut rows, copy(layout.id)?)?;
    }
    let language = setting_text(host, LANGUAGE_KEY)?;
    for layout in added_layouts(host, &language)? {
        if all_drawn(&drawn, &layout.letters) {
            push(&mut rows, layout.id)?;
        }
    }
    let mut chosen: Vec<String> = Vec::new();
    for id in setting_text(host, LAYOUTS_KEY)?
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        push(&mut chosen, copy(id)?)?;
    }
    let mut view = ChooseView {
        rows,
        chosen,
        list: ItemList::default(),
    };
    view.list.replace(items(host, &view.rows, &view.chosen)?);
    Ok(view)
}

/// The list of every layout with *On* (and its place) or *Off*, built aside.
fn items(host: &Host<'_>, rows: &[String], chosen: &[String]) -> Result<Vec<Item>, Error> {
    let mut messages = Messages::new()?;
    let on = messages.copy(host, "keyboard.on")?;
    let off = messages.copy(host, "keyboard.off")?;
    let mut list = Vec::new();
    for id in rows {
        // The id is the translation key of the name; an added layout may have
        // none, and then shows as its id.
        let name = match messages.copy(host, id) {
            Ok(name) => name,
            Err(Error::NotFound | Error::Invalid | Error::Utf8 | Error::NoSpace) => copy(id)?,
            Err(error) => return Err(error),
        };
        let at = chosen.iter().position(|c| c == id);
        let mut digits = [0; 20];
        let value = match at {
            // Chosen layouts carry their place in switching: "On (2)".
            Some(at) if chosen.len() > 1 => paperpaw_ui_kit::text::concat(&[
                &on,
                " (",
                paperpaw_ui_kit::text::decimal(at + 1, &mut digits),
                ")",
            ])?,
            Some(_) => copy(&on)?,
            None => copy(&off)?,
        };
        push(&mut list, Item::row(&name, &value)?)?;
    }
    Ok(list)
}

/// Switches layout `index` on or off; *Automatic* stands for itself.
fn toggle(host: &Host<'_>, view: &mut ChooseView, index: usize) -> Result<(), Error> {
    let Some(id) = view.rows.get(index) else {
        return Ok(());
    };
    let mut chosen: Vec<String> = Vec::new();
    if id == AUTO {
        push(&mut chosen, copy(AUTO)?)?;
    } else {
        for c in view.chosen.iter().filter(|c| *c != AUTO) {
            push(&mut chosen, copy(c)?)?;
        }
        match chosen.iter().position(|c| c == id) {
            Some(at) => {
                chosen.remove(at);
            }
            None => push(&mut chosen, copy(id)?)?,
        }
        if chosen.is_empty() {
            push(&mut chosen, copy(AUTO)?)?;
        }
    }
    let value = paperpaw_ui_kit::mem::join(&chosen, ", ")?;
    // The list is built aside and the setting written first: the view
    // changes only when both succeeded.
    let list = items(host, &view.rows, &chosen)?;
    let mut scratch = buffer(1024)?;
    CommandSender::new(host, &mut scratch).set_setting(LAYOUTS_KEY, SettingValue::Text(&value))?;
    view.chosen = chosen;
    let focus = view.list.focus();
    view.list.replace(list);
    view.list.set_focus(focus);
    Ok(())
}
