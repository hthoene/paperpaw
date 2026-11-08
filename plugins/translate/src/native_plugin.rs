//! The interface-7 translation plugin: the panel on `core.ui` (`native_panel`),
//! asking DeepL over `core.net` and the key kept with `core.secrets`
//! (`native_ask`). What the plugin decides without a host is in `native_model`,
//! tested without one (`PORTING-7.md`).
//!
//! The panel is opened over a page with the selected text as the hand-over's
//! text. When it is first shown it reads the key and sends the text off (or
//! says what is missing); the end of the request (`http_done`) shows the
//! translation or why there is none. The key is asked for on the keyboard
//! (a masked text question), kept with the secrets, and used at once.
use crate::{
    declarations::{HTTP_TOKEN, PANEL_TOKEN},
    native_ask::{Asker, KEY_LABEL, Kept},
    native_model::{self, Outcome, Secret},
    native_panel::{Act, Note, Panel},
};
use alloc::string::String;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        events::{Event, Incoming, kinds},
        i18n, registrations,
        reply::Reply,
        settings::{self, SettingValue},
        ui::{Instance, Notify, Question, RequestId, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, wipe},
    read_event, uitext,
};

#[expect(
    dead_code,
    reason = "the language setting is in the image, submitted once a choice may have 33 values (draft 29)"
)]
mod image {
    include!(concat!(env!("OUT_DIR"), "/translate_declarations.rs"));
}

/// The longest selected text read (`ui.md` 1.3: the host text's cap).
const MAX_TEXT: usize = 24 * 1024;
/// The most a key may be (the secrets' value cap).
const MAX_KEY: usize = 1024;
/// How long *Go online* keeps the network for the panel, in milliseconds.
const ONLINE_MS: u32 = 600_000;
/// The setting of the language translated into.
const TO: &str = "translate.to";

/// The interface-7 translation plugin.
#[derive(Debug, Default)]
pub struct NativeTranslate {
    asker: Asker,
    ui: Option<Ui>,
    screens: Screens<Panel>,
    /// The key being asked for, on the keyboard.
    asking_key: Option<RequestId>,
}

impl Plugin for NativeTranslate {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        // `image::SETTINGS` is submitted when a choice may have 33 values
        // (draft 29); `PORTING-7.md` says what that costs until then. The
        // offer comes after the screen it names.
        for batch in image::SCREEN
            .iter()
            .chain(image::OFFER)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::HttpDone,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::Answered
        ) else {
            return Ok(());
        };
        match event {
            Event::HttpDone(done) if token == HTTP_TOKEN => {
                let outcome = self.asker.done(host, &done)?;
                match outcome {
                    Some(outcome) => self.finished(host, outcome),
                    None => Ok(()),
                }
            }
            Event::ScreenOpened {
                instance, handover, ..
            } if token == PANEL_TOKEN => self.opened(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == PANEL_TOKEN => {
                self.screens.shown(token, instance)?;
                self.begin(host)
            }
            Event::ScreenClosed { instance, .. } if token == PANEL_TOKEN => {
                self.asker.cancel(host);
                self.asking_key = None;
                self.screens.close(instance);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token != PANEL_TOKEN {
            return Err(Error::Invalid);
        }
        self.screens
            .top(token)
            .ok_or(Error::State)?
            .draw(host, &mut frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token != PANEL_TOKEN {
            return Ok(Handled::No);
        }
        let Some(panel) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let act = match input {
            Input::Action { action, .. } => panel.action(action),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => panel.tap(position),
            _ => Act::Ignored,
        };
        if act == Act::Ignored {
            return Ok(Handled::No);
        }
        self.act(host, act)?;
        Ok(Handled::Yes)
    }
}

impl NativeTranslate {
    fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    fn panel(&mut self) -> Option<&mut Panel> {
        self.screens.top(PANEL_TOKEN)
    }

    /// The panel of a new instance, with the selected text.
    fn opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        let original = match handover {
            Some(handover) => {
                let ui = self.ui(host)?;
                match uitext::read_all(ui, host, handover.text(), MAX_TEXT) {
                    Ok(text) => text,
                    // No text (or none whole): nothing to translate, said so.
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(_) => String::new(),
                }
            }
            None => String::new(),
        };
        self.screens.open(instance, Panel::new(original)?)
    }

    /// Sends the text off when the panel is first shown.
    fn begin(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(panel) = self.panel() else {
            return Ok(());
        };
        if !panel.first_time() {
            return Ok(());
        }
        self.ask(host, None)
    }

    /// Reads the key (or takes `typed`), and sends the text off, or says what
    /// is missing: no network, no key.
    fn ask(&mut self, host: &Host<'_>, typed: Option<Secret>) -> Result<(), Error> {
        let key = match typed {
            Some(key) => Some(key),
            None => match self.asker.key(host) {
                Ok(key) => key,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // No secrets at all (or the list cannot be read): no key, asked for.
                Err(_) => None,
            },
        };
        let note = match &key {
            None => Some(Note::NeedsKey),
            Some(key) => match self.send(host, key) {
                Ok(()) => None,
                // No network at all, or this plugin's network off.
                Err(Error::Unsupported | Error::Denied) => Some(Note::NoNetwork),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => Some(Note::Failed),
            },
        };
        let panel = self.panel().ok_or(Error::State)?;
        panel.say(note.unwrap_or(Note::Loading));
        redraw(host)
    }

    fn send(&mut self, host: &Host<'_>, key: &Secret) -> Result<(), Error> {
        let Some(panel) = self.screens.top(PANEL_TOKEN) else {
            return Err(Error::State);
        };
        let original = panel.original();
        let mut scratch = [0; 64];
        let mut ui_language = [0; 32];
        let to = match settings::read_setting(host, TO, &mut scratch) {
            Ok(SettingValue::Text(to)) => to,
            _ => crate::native_model::AUTO,
        };
        let language = i18n::language(host, &mut ui_language).unwrap_or("en");
        let target = native_model::target(to, language)?;
        self.asker.ask(host, key.as_str(), original, &target)
    }

    /// What the request came to.
    fn finished(&mut self, host: &Host<'_>, outcome: Outcome) -> Result<(), Error> {
        let Some(panel) = self.panel() else {
            return Ok(());
        };
        match outcome {
            Outcome::Translated(text) => panel.show(text),
            Outcome::BadKey => panel.say(Note::BadKey),
            Outcome::Quota => panel.say(Note::Quota),
            Outcome::Unreachable => panel.say(Note::Unreachable),
            Outcome::Offline => {
                // The offer, once; then it is said to need a network.
                if !panel.offer() {
                    panel.say(Note::NoNetwork);
                }
            }
            _ => panel.say(Note::Failed),
        }
        redraw(host)
    }

    /// What a key or tap asked of the panel.
    fn act(&mut self, host: &Host<'_>, act: Act) -> Result<(), Error> {
        match act {
            Act::Moved | Act::Focus => redraw(host),
            Act::Close => {
                // Best effort: a panel that cannot be closed now stays, and the
                // host's own back key still closes it.
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).close_screen().ok();
                Ok(())
            }
            Act::AskKey => self.ask_key(host),
            Act::GoOnline => {
                // Refused (a device that cannot): the request is sent all the same,
                // and says again that the device is offline.
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch)
                    .go_online(ONLINE_MS)
                    .ok();
                if let Some(panel) = self.panel() {
                    panel.say(Note::Loading);
                }
                // The request again: the network is there for the panel.
                self.ask(host, None)
            }
            Act::NotNow => {
                if let Some(panel) = self.panel() {
                    panel.say(Note::NoNetwork);
                }
                redraw(host)
            }
            _ => Ok(()),
        }
    }

    /// Asks for the key on the keyboard, typed masked.
    fn ask_key(&mut self, host: &Host<'_>) -> Result<(), Error> {
        // The catalog's words; the key itself where the host has none.
        let title = match Messages::new()?.copy(host, KEY_LABEL) {
            Ok(title) => title,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => paperpaw_ui_kit::text::copy(KEY_LABEL)?,
        };
        let question = Question::text(&title, "")?.secret()?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let asked = self.ui(host)?.ask(host, question, &mut input, &mut out);
        wipe(&mut input);
        wipe(&mut out);
        self.asking_key = Some(asked?);
        Ok(())
    }

    /// The key was typed: kept with the secrets and used at once.
    fn answered(
        &mut self,
        host: &Host<'_>,
        answer: paperpaw_sdk::native::events::Answered,
    ) -> Result<(), Error> {
        use paperpaw_sdk::native::events::AnswerOutcome;
        if self.asking_key != Some(answer.request()) || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        self.asking_key = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let Some(typed) = self.read_key(host, answer.request())? else {
            // More than a key can be: it was not accepted.
            if let Some(panel) = self.panel() {
                panel.say(Note::BadKey);
            }
            return redraw(host);
        };
        let kept = self.asker.keep_key(host, typed.as_str());
        match kept {
            Ok(Kept::Empty) => return Ok(()),
            Ok(Kept::Unsupported) => {
                // Used this time only; the reader is told (owner decision 141).
                native_model::best_effort(notice(host, "core.secrets-unsupported"));
            }
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // Not kept (the card, the list is full): used this time all the same.
            Ok(Kept::Saved) | Err(_) => {}
        }
        if let Some(panel) = self.panel() {
            panel.say(Note::Loading);
        }
        self.ask(host, Some(typed))
    }

    /// The key typed, trimmed, read with buffers that are wiped; `None` for one
    /// longer than a key can be.
    fn read_key(&mut self, host: &Host<'_>, request: RequestId) -> Result<Option<Secret>, Error> {
        let ui = self.ui(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let result = ui
            .text_read(host, request.text(), 0, &mut input, &mut out)
            .and_then(|part| {
                let total = usize::try_from(part.total()).map_err(|_| Error::NoSpace)?;
                if total > MAX_KEY + 64 {
                    return Ok(None);
                }
                if part.data().len() != total {
                    return Err(Error::Invalid);
                }
                let key = part.data().trim();
                if key.len() > MAX_KEY {
                    return Ok(None);
                }
                Secret::new(key).map(Some)
            });
        wipe(&mut input);
        wipe(&mut out);
        result
    }
}

/// The shared notice `key`, shown to the reader.
fn notice(host: &Host<'_>, key: &str) -> Result<(), Error> {
    let text = Messages::new()?.copy(host, key)?;
    let mut scratch = [0; 512];
    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
}

/// Asks for the panel to be drawn again. A refusal (the panel is not on top any
/// more: another screen opened, or it closed) is not an error of what changed it.
#[allow(clippy::unnecessary_wraps, reason = "reads as the other steps do")]
fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(RedrawMode::Partial)
        .ok();
    Ok(())
}
