//! The interface-7 Anki plugin. The home screen (study, sync, the deck, the
//! computer's address and port, the API key: the key is kept among the
//! plugin's secrets, the rest as settings), the deck choice, the study, and
//! the sync (`native_sync`) carried out over the network service: started by
//! the *Sync* row, the action, or opening the screen where the setting says,
//! ended by the `http_done` events of its requests.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    events::{AnswerOutcome, Answered, Event, Incoming, kinds},
    formatting::Arg,
    net::{BODY_PART, Header, HttpDone, Net, REPLY, REQUEST_INPUT, Request, RequestId},
    registrations,
    reply::Reply,
    secrets::Secrets,
    settings::{self, SettingValue},
    storage::{self, Root},
    ui::{Notify, Open, Question, RequestId as AskId, TextMode, TextWrite, Ui},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    files::Volume,
    mem::{SERVICE_BUFFER, buffer, push, wipe},
    read_event,
    text::{concat, copy},
};

use crate::declarations::{DECKS, HOME, HTTP_TOKEN, OPEN_TOKEN, STOP_TOKEN, STUDY, SYNC_TOKEN};
use crate::native_anki::{Body, MAX_KEY};
use crate::native_keep::{self as keep, API_KEY, Secret};
use crate::native_study::{self, Study};
use crate::native_study_screen::StudyScreen;
use crate::native_sync::{Account, Done, Finish, Link, Mode, REQUEST_TOKEN, Step, Sync};

mod image {
    include!(concat!(env!("OUT_DIR"), "/anki_declarations.rs"));
}

const HOST: &str = "anki.host";
const PORT: &str = "anki.port";
const DECK: &str = "anki.deck";
const NEW_CARDS: &str = "anki.new-cards";
const SYNC_ON_OPEN: &str = "anki.sync-on-open";
/// What a kept secret is shown as.
const DOTS: &str = "••••••";
/// How long a request may take: AnkiConnect answers at once, but Anki may be
/// busy for a moment.
const TIMEOUT_MS: u32 = 20_000;

/// The storage calls on the plugin's data folder.
struct Data<'a, 'h>(&'a Host<'h>);

impl Volume for Data<'_, '_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        storage::size(self.0, Root::Data, name)
    }
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        storage::read_at(self.0, Root::Data, name, at, out)
    }
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::write(self.0, Root::Data, name, data)
    }
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        storage::append(self.0, Root::Data, name, data)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        storage::rename(self.0, Root::Data, from, to)
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        storage::remove(self.0, Root::Data, name)
    }
    fn sync(&self) -> Result<(), Error> {
        storage::sync(self.0)
    }
}

/// The sync's world on the device: the data folder and the network service
/// (opened by the first request).
struct HostLink<'a, 'h> {
    host: &'a Host<'h>,
    net: &'a mut Option<Net>,
    data: Data<'a, 'h>,
    url: String,
}

impl HostLink<'_, '_> {
    fn net(&mut self) -> Result<&Net, Error> {
        if self.net.is_none() {
            *self.net = Some(Net::open(self.host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }
}

impl Link for HostLink<'_, '_> {
    fn volume(&self) -> &dyn Volume {
        &self.data
    }

    fn send(&mut self, body: &Body) -> Result<RequestId, Error> {
        let headers = [Header::new("Content-Type", "application/json")?];
        let url = copy(&self.url)?;
        let request = Request::new(REQUEST_TOKEN, "POST", &url)?
            .headers(&headers)?
            .body(body.as_bytes())?
            .timeout_ms(TIMEOUT_MS)?;
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let host = self.host;
        let sent = self.net()?.request(host, &request, &mut input, &mut out);
        // The input held the key in the body.
        wipe(&mut input);
        wipe(&mut out);
        sent
    }

    fn read(
        &mut self,
        done: &Done,
        feed: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let length = usize::try_from(done.body_len).map_err(|_| Error::NoSpace)?;
        let (mut input, mut reply) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let host = self.host;
        let net = self.net()?;
        let mut offset = 0usize;
        let result = loop {
            if offset >= length {
                break Ok(());
            }
            let at = u32::try_from(offset).map_err(|_| Error::NoSpace)?;
            match net.body(host, done.id, at, &mut input, &mut reply) {
                Ok([]) => break Ok(()),
                Ok(part) => {
                    let take = part.len().min(BODY_PART).min(length - offset);
                    let fed = part.get(..take).ok_or(Error::Invalid).and_then(&mut *feed);
                    if let Err(error) = fed {
                        break Err(error);
                    }
                    offset += take;
                }
                Err(error) => break Err(error),
            }
        };
        wipe(&mut input);
        wipe(&mut reply);
        result
    }

    fn cancel(&mut self) {
        let host = self.host;
        if let (Ok(mut input), Ok(mut out)) = (buffer(REQUEST_INPUT), buffer(REPLY))
            && let Ok(net) = self.net()
        {
            net.cancel(host, REQUEST_TOKEN, &mut input, &mut out).ok();
        }
    }
}

/// The rows of the home screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Study,
    Sync,
    Deck,
    Host,
    Port,
    Key,
}

const SHOWN: [Row; 6] = [
    Row::Study,
    Row::Sync,
    Row::Deck,
    Row::Host,
    Row::Port,
    Row::Key,
];

impl Row {
    const fn title(self) -> &'static str {
        match self {
            Self::Study => "anki.study",
            Self::Sync => "anki.sync",
            Self::Deck => "anki.deck",
            Self::Host => "anki.host",
            Self::Port => "anki.port",
            Self::Key => "anki.key",
        }
    }
}

/// The home screen: its rows and the note below them.
#[derive(Debug, Default)]
struct Home {
    list: ItemList,
    note: String,
}

/// The deck choice: *All decks* and the names Anki gave.
#[derive(Debug, Default)]
struct Decks {
    list: ItemList,
    names: Vec<String>,
    note: String,
}

/// The interface-7 Anki plugin.
#[derive(Debug, Default)]
pub struct NativeAnki {
    ui: Option<Ui>,
    secrets: Option<Secrets>,
    opened: bool,
    net: Option<Net>,
    sync: Sync,
    mode: Option<Mode>,
    home: Screens<Home>,
    decks: Screens<Decks>,
    study: Screens<StudyScreen>,
    /// The row being typed, and its question.
    typing: Option<(AskId, Row)>,
    /// What the last sync said, kept for the home screen.
    said: String,
}

impl Plugin for NativeAnki {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
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
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Answered, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::HttpDone, kinds::Stopping)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("anki.home")?)
            }
            Event::ActionRun { .. } if token == SYNC_TOKEN => self.sync_now(host),
            Event::HttpDone(done) if token == HTTP_TOKEN => self.request_done(host, &done),
            Event::Stopping { .. } if token == STOP_TOKEN => {
                self.cancel(host);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::ScreenOpened { instance, .. } if token == HOME => {
                self.ensure(host)?;
                let mut view = Home::default();
                self.build_home(host, &mut view)?;
                self.home.open(instance, view)?;
                if self.bool_setting(host, SYNC_ON_OPEN)? && self.configured(host)? {
                    self.sync_now(host)?;
                }
                Ok(())
            }
            Event::ScreenOpened { instance, .. } if token == DECKS => {
                self.ensure(host)?;
                let mut view = Decks::default();
                self.build_decks(host, &mut view)?;
                self.decks.open(instance, view)?;
                self.fetch_decks(host)
            }
            Event::ScreenOpened { instance, .. } if token == STUDY => {
                let study = Study::open(&Data(host))?;
                self.study.open(instance, StudyScreen::of(study))
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => match token {
                HOME => {
                    self.home.shown(token, instance)?;
                    // The values may have changed on the screens above it.
                    self.refresh_home(host)
                }
                DECKS => self.decks.shown(token, instance),
                STUDY => {
                    self.study.shown(token, instance)?;
                    if let Some(view) = self.study.top(STUDY) {
                        view.sync_hint(host);
                    }
                    Ok(())
                }
                _ => Ok(()),
            },
            Event::ScreenClosed { instance, .. } => {
                self.home.close(instance);
                self.decks.close(instance);
                self.study.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut say = Messages::new()?;
        match token {
            HOME => {
                let view = self.home.top(token).ok_or(Error::State)?;
                let title = say.copy(host, "anki.title")?;
                let footer = note(&view.note);
                view.list.draw_with(host, &mut frame, &title, true, footer)
            }
            DECKS => {
                let view = self.decks.top(token).ok_or(Error::State)?;
                let title = say.copy(host, "anki.deck")?;
                let footer = note(&view.note);
                view.list.draw_with(host, &mut frame, &title, true, footer)
            }
            STUDY => self
                .study
                .top(token)
                .ok_or(Error::State)?
                .draw(host, &mut frame),
            _ => Ok(()),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        match token {
            HOME => {
                let Some(view) = self.home.top(token) else {
                    return Ok(Handled::No);
                };
                let step = view.list.input(host, &input)?;
                if let ItemStep::Chosen(index) = step
                    && let Some(row) = SHOWN.get(index).copied()
                {
                    self.chosen(host, row)?;
                }
                Ok(ItemList::handled(step))
            }
            DECKS => {
                let Some(view) = self.decks.top(token) else {
                    return Ok(Handled::No);
                };
                let step = view.list.input(host, &input)?;
                if let ItemStep::Chosen(index) = step {
                    self.deck_chosen(host, index)?;
                }
                Ok(ItemList::handled(step))
            }
            STUDY => match self.study.top(token) {
                Some(view) => view.input(host, &Data(host), &input),
                None => Ok(Handled::No),
            },
            _ => Ok(Handled::No),
        }
    }
}

/// The footer for a note: none where it is empty.
fn note(text: &str) -> Footer<'_> {
    if text.is_empty() {
        Footer::None
    } else {
        Footer::Note(text)
    }
}

impl NativeAnki {
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

    fn int_setting(host: &Host<'_>, key: &str, default: i64) -> Result<i64, Error> {
        let mut scratch = [0; 64];
        match settings::read_setting(host, key, &mut scratch)? {
            SettingValue::Int(value) => Ok(value),
            _ => Ok(default),
        }
    }

    fn bool_setting(&self, host: &Host<'_>, key: &str) -> Result<bool, Error> {
        let mut scratch = [0; 64];
        match settings::read_setting(host, key, &mut scratch)? {
            SettingValue::Bool(value) => Ok(value),
            _ => Ok(false),
        }
    }

    /// The address of AnkiConnect, `http://<computer>:<port>/`; empty where no
    /// computer is set.
    fn address(host: &Host<'_>) -> Result<String, Error> {
        let name = Self::text_setting(host, HOST)?;
        let name = name.trim();
        let name = name
            .strip_prefix("http://")
            .or_else(|| name.strip_prefix("https://"))
            .unwrap_or(name)
            .trim_end_matches('/');
        if name.is_empty() {
            return Ok(String::new());
        }
        let port = Self::int_setting(host, PORT, 8765)?.clamp(1, 65535);
        let mut digits = [0u8; 20];
        let port = paperpaw_ui_kit::text::decimal_u64(port as u64, &mut digits);
        concat(&["http://", name, ":", port, "/"])
    }

    fn configured(&self, host: &Host<'_>) -> Result<bool, Error> {
        Ok(!Self::address(host)?.is_empty())
    }

    /// Whether the key is kept (a device without secrets keeps none).
    fn key_kept(&self, host: &Host<'_>) -> Result<bool, Error> {
        match &self.secrets {
            Some(secrets) => keep::present(secrets, host, API_KEY),
            None => Ok(false),
        }
    }

    /// The rows and the note below them, as things are now.
    fn build_home(&self, host: &Host<'_>, view: &mut Home) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let volume = Data(host);
        let mut items = Vec::new();
        for row in SHOWN {
            let value = match row {
                Row::Study => {
                    let ready = native_study::ready(&volume)?;
                    if ready == 0 {
                        copy("–")?
                    } else {
                        say.format(host, "anki.ready", &[Arg::Count(ready as i64)])?
                    }
                }
                Row::Sync => {
                    let waiting = crate::native_cards::waiting(&volume)?;
                    if waiting == 0 {
                        copy("›")?
                    } else {
                        say.format(host, "anki.waiting", &[Arg::Count(waiting as i64)])?
                    }
                }
                Row::Deck => {
                    let deck = Self::text_setting(host, DECK)?;
                    if deck.is_empty() {
                        say.copy(host, "anki.all-decks")?
                    } else {
                        deck
                    }
                }
                Row::Host => Self::text_setting(host, HOST)?,
                Row::Port => {
                    let mut digits = [0u8; 20];
                    let port = Self::int_setting(host, PORT, 8765)?.clamp(1, 65535);
                    copy(paperpaw_ui_kit::text::decimal_u64(port as u64, &mut digits))?
                }
                Row::Key => copy(if self.key_kept(host)? { DOTS } else { "" })?,
            };
            let title = say.copy(host, row.title())?;
            push(&mut items, Item::row(&title, &value)?)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus);
        if view.note.is_empty() {
            view.note = if self.said.is_empty() {
                say.copy(host, "anki.help")?
            } else {
                copy(&self.said)?
            };
        }
        Ok(())
    }

    /// Rebuilds the rows of the open home screen and asks for a redraw.
    fn refresh_home(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut view = Home::default();
        let Some(top) = self.home.top(HOME) else {
            return Ok(());
        };
        view.list.set_focus(top.list.focus());
        view.note = copy(&top.note)?;
        self.build_home(host, &mut view)?;
        if let Some(top) = self.home.top(HOME) {
            *top = view;
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    /// The deck choice's rows: *All decks* and the names.
    fn build_decks(&self, host: &Host<'_>, view: &mut Decks) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let mut items = Vec::new();
        let all = say.copy(host, "anki.all-decks")?;
        push(&mut items, Item::row(&all, "")?)?;
        for name in &view.names {
            push(&mut items, Item::row(name, "")?)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus);
        Ok(())
    }

    fn chosen(&mut self, host: &Host<'_>, row: Row) -> Result<(), Error> {
        let mut scratch = [0; 128];
        match row {
            Row::Study => CommandSender::new(host, &mut scratch).open(Open::screen("anki.study")?),
            Row::Deck => CommandSender::new(host, &mut scratch).open(Open::screen("anki.decks")?),
            Row::Sync => self.sync_now(host),
            Row::Host | Row::Port | Row::Key => self.ask(host, row),
        }
    }

    fn deck_chosen(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let name = match index {
            0 => String::new(),
            n => match self.decks.top(DECKS).and_then(|v| v.names.get(n - 1)) {
                Some(name) => copy(name)?,
                None => return Ok(()),
            },
        };
        let mut scratch = [0; 1200];
        CommandSender::new(host, &mut scratch).set_setting(DECK, SettingValue::Text(&name))?;
        CommandSender::new(host, &mut [0; 64]).back()
    }

    /// Asks for the text of `row` (the key masked).
    fn ask(&mut self, host: &Host<'_>, row: Row) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let title = say.copy(host, row.title())?;
        let initial = match row {
            Row::Host => Some(Self::text_setting(host, HOST)?),
            _ => None,
        };
        let question = match row {
            Row::Key => Question::text(&title, "")?.secret()?,
            Row::Port => Question::text(&title, "")?.mode(TextMode::Number)?,
            _ => Question::text(&title, "")?.mode(TextMode::Url)?,
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let asked = ui.ask(host, question, &mut input, &mut out);
        // The question stays open when the filling fails (the field is then
        // empty); it is tracked either way so its answer is still taken.
        if let (Ok(request), Some(initial)) = (&asked, initial.as_deref().filter(|i| !i.is_empty()))
        {
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

    /// Keeps what was typed for `row`. An empty key answer keeps what was kept
    /// (the field never shows it): a confirm by mistake loses nothing; a key is
    /// taken away with a single space.
    fn apply(&mut self, host: &Host<'_>, row: Row, typed: &str) -> Result<(), Error> {
        let text = typed.trim();
        match row {
            Row::Host => {
                let mut scratch = [0; 1200];
                CommandSender::new(host, &mut scratch)
                    .set_setting(HOST, SettingValue::Text(text))?;
            }
            Row::Port => {
                if let Ok(port) = text.parse::<i64>()
                    && (1..=65535).contains(&port)
                {
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch)
                        .set_setting(PORT, SettingValue::Int(port))?;
                }
            }
            Row::Key => {
                if typed.is_empty() {
                    return Ok(());
                }
                self.keep_key(host, text)?;
            }
            Row::Study | Row::Sync | Row::Deck => return Ok(()),
        }
        // On to the next row.
        let at = SHOWN.iter().position(|r| *r == row).unwrap_or(0);
        if let Some(top) = self.home.top(HOME) {
            top.list.set_focus((at + 1).min(SHOWN.len() - 1));
        }
        self.refresh_home(host)
    }

    /// Keeps the key (an empty text takes it away); said where it cannot be.
    fn keep_key(&self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let notice = match &self.secrets {
            None => Some("core.secrets-unsupported"),
            Some(_) if key.len() > MAX_KEY => Some("anki.not-kept"),
            Some(secrets) => {
                let label = say.copy(host, "anki.secret.key")?;
                match keep::keep(secrets, host, API_KEY, &label, key) {
                    Ok(()) => None,
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(Error::Unsupported) => Some("core.secrets-unsupported"),
                    Err(_) => Some("anki.not-kept"),
                }
            }
        };
        if let Some(key) = notice {
            let text = say.copy(host, key)?;
            self.notice(host, &text)?;
        }
        Ok(())
    }

    /// The account the sync is made for: the key and the deck settings.
    fn account(&self, host: &Host<'_>) -> Result<Account, Error> {
        let key = match &self.secrets {
            Some(secrets) => keep::read(secrets, host, API_KEY)?,
            None => None,
        };
        Ok(Account {
            key,
            deck: Self::text_setting(host, DECK)?,
            new: Self::int_setting(host, NEW_CARDS, 10)?.clamp(0, 50) as usize,
        })
    }

    /// Starts a sync: answers out, cards in.
    fn sync_now(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.sync.running() {
            return Ok(());
        }
        self.ensure(host)?;
        self.begin(host, Mode::Sync)
    }

    /// The deck names are asked for.
    fn fetch_decks(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let key = if self.sync.running() {
            "anki.busy"
        } else {
            return self.begin(host, Mode::Decks);
        };
        let text = Messages::new()?.copy(host, key)?;
        if let Some(view) = self.decks.top(DECKS) {
            view.note = text;
        }
        Ok(())
    }

    fn begin(&mut self, host: &Host<'_>, mode: Mode) -> Result<(), Error> {
        let url = Self::address(host)?;
        if url.is_empty() {
            return self.finished(host, Finish::said_key("anki.setup-needed"), mode);
        }
        let mut say = Messages::new()?;
        let text = say.copy(host, "anki.syncing")?;
        if mode == Mode::Sync {
            self.notice(host, &text)?;
        } else if let Some(view) = self.decks.top(DECKS) {
            view.note = text;
        }
        let account = self.account(host)?;
        self.mode = Some(mode);
        let mut link = HostLink {
            host,
            net: &mut self.net,
            data: Data(host),
            url,
        };
        let step = self.sync.start(&mut link, account, mode)?;
        self.stepped(host, step)
    }

    /// The end of a request of the sync.
    fn request_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        // The next request of the sync (the answers, then the cards) goes to
        // the same address: it is read again here (an empty one is no request).
        let url = Self::address(host)?;
        let done = Done {
            id: done.id(),
            token: done.token(),
            reason: done.reason(),
            status: done.status(),
            truncated: done.truncated(),
            body_len: done.body_len(),
        };
        let mut link = HostLink {
            host,
            net: &mut self.net,
            data: Data(host),
            url,
        };
        let step = self.sync.done(&mut link, &done)?;
        self.stepped(host, step)
    }

    fn stepped(&mut self, host: &Host<'_>, step: Step) -> Result<(), Error> {
        match step {
            Step::Quiet => Ok(()),
            Step::Ended(finish) => {
                let mode = self.mode.take().unwrap_or(Mode::Sync);
                self.finished(host, finish, mode)
            }
        }
    }

    /// The plugin stops: a sync under way ends.
    fn cancel(&mut self, host: &Host<'_>) {
        let mut link = HostLink {
            host,
            net: &mut self.net,
            data: Data(host),
            url: String::new(),
        };
        self.sync.cancel(&mut link);
    }

    /// Says how the sync ended: a notice and the note of the screen, and the
    /// deck names where they were asked for.
    fn finished(&mut self, host: &Host<'_>, finish: Finish, mode: Mode) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let text = match (finish.key, &finish.said) {
            ("anki.said", Some(said)) => say.format(
                host,
                finish.key,
                &[Arg::Text {
                    name: "message",
                    value: said,
                }],
            )?,
            ("anki.synced" | "anki.nothing-due", _) => say.format(
                host,
                finish.key,
                &[
                    Arg::Int {
                        name: "sent",
                        value: finish.sent as i64,
                    },
                    Arg::Int {
                        name: "fetched",
                        value: finish.fetched as i64,
                    },
                ],
            )?,
            _ => say.copy(host, finish.key)?,
        };
        if mode == Mode::Decks {
            if let Some(view) = self.decks.top(DECKS) {
                match finish.names {
                    Some(names) => {
                        view.names = names;
                        view.note = String::new();
                        let mut next = Decks {
                            names: core::mem::take(&mut view.names),
                            note: String::new(),
                            list: ItemList::default(),
                        };
                        next.list.set_focus(view.list.focus());
                        self.build_decks(host, &mut next)?;
                        if let Some(view) = self.decks.top(DECKS) {
                            *view = next;
                        }
                    }
                    None => view.note = text,
                }
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
            }
            return Ok(());
        }
        self.notice(host, &text)?;
        self.said = text;
        if let Some(top) = self.home.top(HOME) {
            top.note = copy(&self.said)?;
        }
        self.refresh_home(host)
    }

    /// A notice, where it can be shown.
    fn notice(&self, host: &Host<'_>, text: &str) -> Result<(), Error> {
        let mut scratch = [0; 512];
        match CommandSender::new(host, &mut scratch).notify(Notify::new(text)?) {
            Ok(()) | Err(_) => {}
        }
        Ok(())
    }
}
