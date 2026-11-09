//! The interface-7 wallabag plugin (slice 3: the set-up). The set-up screen
//! with the server, the user name, the password, the API client's ID and its
//! secret; the password and the secret are kept among the plugin's secrets, the
//! rest as settings. Fetching articles follows (`PORTING-7.md`).
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    events::{AnswerOutcome, Answered, Event, Incoming, kinds},
    registrations,
    reply::Reply,
    secrets::Secrets,
    settings::{self, SettingValue},
    ui::{Notify, Open, Question, RequestId as AskId, TextMode, TextWrite, Ui},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push, wipe},
    read_event,
    text::copy,
};

use crate::declarations::{OPEN_TOKEN, SCREEN};
use crate::native_keep::{self as keep, CLIENT_SECRET, PASSWORD, Secret};
use crate::native_wallabag::Row;

mod image {
    include!(concat!(env!("OUT_DIR"), "/wallabag_declarations.rs"));
}

const SERVER: &str = "wallabag.server";
const USER: &str = "wallabag.user";
const CLIENT: &str = "wallabag.client-id";
/// The rows the set-up shows: *Fetch articles* comes with the fetch.
const SHOWN: [Row; 5] = [
    Row::Server,
    Row::User,
    Row::Password,
    Row::Client,
    Row::Secret,
];
/// What a kept secret is shown as.
const DOTS: &str = "••••••";

/// The set-up screen: its rows and the note below them.
#[derive(Debug, Default)]
struct View {
    list: ItemList,
    note: String,
}

/// The interface-7 wallabag plugin.
#[derive(Debug, Default)]
pub struct NativeWallabag {
    ui: Option<Ui>,
    secrets: Option<Secrets>,
    opened: bool,
    screens: Screens<View>,
    /// The row being typed, and its question.
    typing: Option<(AskId, Row)>,
}

impl Plugin for NativeWallabag {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
        {
            registrations::submit(host, batch)?;
        }
        // The services are opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Answered, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("wallabag.setup")?)
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::ScreenOpened { instance, .. } if token == SCREEN => {
                self.ensure(host)?;
                let mut view = View::default();
                self.build(host, &mut view)?;
                self.screens.open(instance, view)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => self.screens.shown(token, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let view = self.screens.top(token).ok_or(Error::State)?;
        let title = say.copy(host, "wallabag.title")?;
        let footer = if view.note.is_empty() {
            Footer::None
        } else {
            Footer::Note(&view.note)
        };
        view.list.draw_with(host, &mut frame, &title, true, footer)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step
            && let Some(row) = SHOWN.get(index).copied()
        {
            self.ask(host, row)?;
        }
        Ok(ItemList::handled(step))
    }
}

impl NativeWallabag {
    /// Opens the services once. A device without secrets keeps none (said when
    /// one is typed).
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        if !self.opened {
            self.opened = true;
            self.secrets = match Secrets::open(host) {
                Ok(secrets) => Some(secrets),
                Err(Error::NoMemory) => {
                    self.opened = false;
                    return Err(Error::NoMemory);
                }
                Err(_) => None,
            };
        }
        Ok(())
    }

    fn text_setting(host: &Host<'_>, key: &str) -> Result<String, Error> {
        let mut scratch = [0; 1100];
        match settings::read_setting(host, key, &mut scratch)? {
            SettingValue::Text(text) => copy(text),
            _ => Ok(String::new()),
        }
    }

    /// Whether the secret `name` is kept (a device without secrets keeps none).
    fn kept(&self, host: &Host<'_>, name: &str) -> Result<bool, Error> {
        match &self.secrets {
            Some(secrets) => keep::present(secrets, host, name),
            None => Ok(false),
        }
    }

    /// The rows and the note below them, as the account is now.
    fn build(&self, host: &Host<'_>, view: &mut View) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let mut items = Vec::new();
        for row in SHOWN {
            let value = match row {
                Row::Server => {
                    let url = paperpaw_sdk::web::try_base_url(&Self::text_setting(host, SERVER)?)
                        .map_err(|_| Error::NoMemory)?;
                    copy(url.strip_prefix("https://").unwrap_or(&url))?
                }
                Row::User => Self::text_setting(host, USER)?,
                Row::Client => Self::text_setting(host, CLIENT)?,
                Row::Password => dots(self.kept(host, PASSWORD)?)?,
                Row::Secret => dots(self.kept(host, CLIENT_SECRET)?)?,
                Row::Fetch => continue,
            };
            let title = say.copy(host, row.title())?;
            push(&mut items, Item::row(&title, &value)?)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus);
        if view.note.is_empty() {
            view.note = say.copy(host, "wallabag.help")?;
        }
        Ok(())
    }

    /// Rebuilds the rows of the open screen and asks for a redraw.
    fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut view = View::default();
        let Some(top) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        view.list.set_focus(top.list.focus());
        view.note = copy(&top.note)?;
        self.build(host, &mut view)?;
        if let Some(top) = self.screens.top(SCREEN) {
            *top = view;
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    /// Asks for the text of `row` (a password masked, the server as an address).
    fn ask(&mut self, host: &Host<'_>, row: Row) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let title = say.copy(host, row.title())?;
        let secret = matches!(row, Row::Password | Row::Secret);
        let initial = match row {
            Row::Server => Some(Self::text_setting(host, SERVER)?),
            Row::User => Some(Self::text_setting(host, USER)?),
            Row::Client => Some(Self::text_setting(host, CLIENT)?),
            _ => None,
        };
        let question = if secret {
            Question::text(&title, "")?.secret()?
        } else if row == Row::Server {
            Question::text(&title, "")?.mode(TextMode::Url)?
        } else {
            Question::text(&title, "")?
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let asked = ui.ask(host, question, &mut input, &mut out);
        // The question stays open when the filling fails (the field is then
        // empty); it is tracked either way so its answer is still taken.
        if let (Ok(request), Some(initial)) = (
            &asked,
            initial.as_deref().filter(|i| !i.is_empty()),
        ) {
            match ui.text_write(
                host,
                request.text(),
                0,
                initial,
                TextWrite::Last,
                &mut input,
                &mut out,
            ) {
                Ok(()) | Err(_) => {}
            }
        }
        wipe(&mut input);
        wipe(&mut out);
        self.typing = Some((asked?, row));
        Ok(())
    }

    /// An answer to a question the plugin asked.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, row)) = self.typing else {
            return Ok(());
        };
        if request != answer.request() {
            return Ok(());
        }
        self.typing = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let read = ui.text_read(host, answer.request().text(), 0, &mut input, &mut out);
        let text = match read {
            Ok(part) if part.data().len() as u64 == u64::from(part.total()) => {
                Secret::copy(part.data())
            }
            Ok(_) => Err(Error::NoSpace),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut out);
        let text = text?;
        self.apply(host, row, text.as_str())
    }

    /// Keeps what was typed for `row`: a setting, or a secret. An empty answer
    /// for a secret keeps what was kept: a confirm by mistake on the empty field
    /// (a password is never shown in it) loses nothing.
    fn apply(&mut self, host: &Host<'_>, row: Row, typed: &str) -> Result<(), Error> {
        let text = typed.trim();
        let setting = |key: &str| {
            let mut scratch = [0; 1200];
            CommandSender::new(host, &mut scratch).set_setting(key, SettingValue::Text(text))
        };
        match row {
            Row::Server => setting(SERVER)?,
            Row::User => setting(USER)?,
            Row::Client => setting(CLIENT)?,
            Row::Password | Row::Secret => {
                if text.is_empty() {
                    return Ok(());
                }
                let (name, label) = if row == Row::Password {
                    (PASSWORD, "wallabag.secret.password")
                } else {
                    (CLIENT_SECRET, "wallabag.secret.client")
                };
                self.keep_secret(host, name, label, text)?;
            }
            Row::Fetch => return Ok(()),
        }
        // On to the next row.
        let at = SHOWN.iter().position(|r| *r == row).unwrap_or(0);
        if let Some(top) = self.screens.top(SCREEN) {
            top.list.set_focus((at + 1).min(SHOWN.len() - 1));
        }
        self.refresh(host)
    }

    /// Keeps a secret; said with the shared text where the device keeps none.
    fn keep_secret(
        &self,
        host: &Host<'_>,
        name: &str,
        label: &str,
        value: &str,
    ) -> Result<(), Error> {
        let mut say = Messages::new()?;
        // The shared text only where the device keeps no secrets; any other
        // refusal (a value too long, a full store) says it was not kept.
        let notice = match &self.secrets {
            None => Some("core.secrets-unsupported"),
            Some(secrets) => {
                let label = say.copy(host, label)?;
                match keep::keep(secrets, host, name, &label, value) {
                    Ok(()) => None,
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(Error::Unsupported) => Some("core.secrets-unsupported"),
                    Err(_) => Some("wallabag.not-kept"),
                }
            }
        };
        if let Some(key) = notice {
            let text = say.copy(host, key)?;
            let mut scratch = [0; 512];
            match CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?) {
                Ok(()) | Err(_) => {}
            }
        }
        Ok(())
    }
}

/// What a secret is shown as: dots where it is kept.
fn dots(kept: bool) -> Result<String, Error> {
    copy(if kept { DOTS } else { "" })
}
