//! The screens of File sync: the home (*Sync now*, the three pairs, and below
//! them what is going on or what the last sync did) and a pair (its card
//! folder, server, user, password and mode). The password is kept among the
//! plugin's secrets, the rest as settings.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, FrameRect, Input},
    commands::{CommandSender, RedrawMode},
    events::{AnswerOutcome, Answered},
    formatting::Arg,
    settings::SettingValue,
    ui::{Open, Question, TextMode, TextWrite},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    dav::{
        secret::{self as keep, Secret},
        url::folder_url,
    },
    mem::{SERVICE_BUFFER, buffer, push, wipe},
    text::copy,
};

use crate::declarations::{HOME, MODE, PAIR, PAIRS};
use crate::native_pair::{self as pairs, folder_of};
use crate::native_plugin::NativeFileSync;

/// What a kept secret is shown as.
const DOTS: &str = "••••••";
/// The longest password kept (the header must fit a request).
const MAX_PASSWORD: usize = 400;

/// The rows of a pair's screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Folder,
    Server,
    User,
    Password,
    Mode,
}

const SHOWN: [Row; 5] = [
    Row::Folder,
    Row::Server,
    Row::User,
    Row::Password,
    Row::Mode,
];

impl Row {
    const fn title(self) -> &'static str {
        match self {
            Self::Folder => "file-sync.setting.folder",
            Self::Server => "file-sync.setting.server",
            Self::User => "file-sync.setting.user",
            Self::Password => "file-sync.password",
            Self::Mode => "file-sync.setting.mode",
        }
    }
}

/// A screen: its rows and the note below them.
#[derive(Debug, Default)]
pub struct View {
    list: ItemList,
    note: String,
    /// The screen's region in the last frame.
    region: Option<FrameRect>,
}

impl View {
    /// The room below the rows in the last frame, where the note is, in the
    /// region's pixels; `None` before a frame and where the last row is on
    /// another page.
    fn below_rows(&self) -> Option<FrameRect> {
        let region = self.region?;
        let last = self.list.rect_of(self.list.items().len().checked_sub(1)?)?;
        let top = (i32::from(last.y) + i32::from(last.height)).saturating_sub(region.y);
        let height = u32::try_from(i64::from(region.height) - i64::from(top)).ok()?;
        Some(FrameRect::new(0, top, region.width, height))
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

impl NativeFileSync {
    /// The note below the home's rows: what is going on, what the last sync
    /// said, or what the plugin is for.
    fn home_note(&self, host: &Host<'_>, say: &mut Messages) -> Result<String, Error> {
        if let Some((name, done, all)) = self.sync.progress() {
            return say.format(
                host,
                "file-sync.progress",
                &[
                    Arg::Text {
                        name: "name",
                        value: name,
                    },
                    Arg::Int {
                        name: "done",
                        value: done as i64 + 1,
                    },
                    Arg::Int {
                        name: "all",
                        value: all as i64,
                    },
                ],
            );
        }
        if self.sync.running() {
            return say.copy(host, "file-sync.syncing");
        }
        if self.said.is_empty() {
            say.copy(host, "file-sync.help")
        } else {
            copy(&self.said)
        }
    }

    /// The home's rows: *Sync now* and the pairs.
    pub(crate) fn build_home(&self, host: &Host<'_>, view: &mut View) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let mut items = Vec::new();
        let title = say.copy(host, "file-sync.sync")?;
        let value = if self.sync.running() {
            say.copy(host, "file-sync.syncing.short")?
        } else {
            copy("›")?
        };
        push(&mut items, Item::row(&title, &value)?)?;
        for n in 1..=PAIRS {
            let pair = pairs::read(host, n)?;
            let title = say.format(
                host,
                "file-sync.pair.row",
                &[Arg::Int {
                    name: "n",
                    value: n as i64,
                }],
            )?;
            let value = if pair.ready() {
                let server = paperpaw_sdk::web::host(&pair.url);
                paperpaw_ui_kit::text::concat(&[&pair.folder, " @ ", server])?
            } else {
                say.copy(host, "file-sync.pair.empty")?
            };
            push(&mut items, Item::row(&title, &value)?)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus);
        view.note = self.home_note(host, &mut say)?;
        Ok(())
    }

    /// The rows of the screen of the pair being edited.
    pub(crate) fn build_pair(&self, host: &Host<'_>, view: &mut View) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let pair = pairs::read(host, self.editing)?;
        let mut items = Vec::new();
        for row in SHOWN {
            let value = match row {
                Row::Folder => copy(&pair.folder)?,
                Row::Server => copy(&pair.url)?,
                Row::User => copy(&pair.user)?,
                Row::Password => copy(if self.password_kept(host)? { DOTS } else { "" })?,
                Row::Mode => say.copy(
                    host,
                    if pair.mode == crate::native_plan::Mode::TwoWay {
                        MODE[0]
                    } else {
                        MODE[1]
                    },
                )?,
            };
            let title = say.copy(host, row.title())?;
            push(&mut items, Item::row(&title, &value)?)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus);
        view.note = say.copy(host, "file-sync.pair.help")?;
        Ok(())
    }

    /// Whether the password of the pair being edited is kept (a device
    /// without secrets keeps none).
    fn password_kept(&self, host: &Host<'_>) -> Result<bool, Error> {
        match &self.secrets {
            Some(secrets) => keep::present(secrets, host, &pairs::secret_name(self.editing)?),
            None => Ok(false),
        }
    }

    /// A step of the sync: only the home's note says something else, so only
    /// the room below its rows is drawn again (they stay as they are while a
    /// sync runs). Nothing is drawn while a pair's screen is open above it:
    /// the home is built anew when it is shown again.
    pub(crate) fn refresh_progress(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.home.top(HOME).is_none() || self.pair_view.top(PAIR).is_some() {
            return Ok(());
        }
        let note = self.home_note(host, &mut Messages::new()?)?;
        let Some(top) = self.home.top(HOME) else {
            return Ok(());
        };
        if top.note == note {
            return Ok(());
        }
        top.note = note;
        let mut scratch = [0; 64];
        let mut sender = CommandSender::new(host, &mut scratch);
        match top.below_rows() {
            Some(area) => sender.queue_redraw_area(RedrawMode::Fast, area),
            None => sender.queue_redraw(RedrawMode::Partial),
        }
    }

    /// Rebuilds the rows of the open screens and asks for a redraw.
    pub(crate) fn refresh_home(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut changed = false;
        if let Some(top) = self.home.top(HOME) {
            let mut view = View::default();
            view.list.set_focus(top.list.focus());
            self.build_home(host, &mut view)?;
            if let Some(top) = self.home.top(HOME) {
                *top = view;
            }
            changed = true;
        }
        if let Some(top) = self.pair_view.top(PAIR) {
            let mut view = View::default();
            view.list.set_focus(top.list.focus());
            self.build_pair(host, &mut view)?;
            if let Some(top) = self.pair_view.top(PAIR) {
                *top = view;
            }
            changed = true;
        }
        if changed {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
        }
        Ok(())
    }

    pub(crate) fn draw_screen(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        token: u32,
    ) -> Result<(), Error> {
        let (key, view) = if token == HOME {
            ("file-sync.title", self.home.top(HOME))
        } else {
            ("file-sync.pair", self.pair_view.top(PAIR))
        };
        let view = view.ok_or(Error::State)?;
        let title = Messages::new()?.copy(host, key)?;
        let footer = note(&view.note);
        view.list.draw_with(host, frame, &title, true, footer)?;
        view.region = Some(frame.rect);
        Ok(())
    }

    pub(crate) fn screen_input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let view = if token == HOME {
            self.home.top(HOME)
        } else {
            self.pair_view.top(PAIR)
        };
        let Some(view) = view else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, input)?;
        if let ItemStep::Chosen(index) = step {
            if token == HOME {
                self.home_chosen(host, index)?;
            } else if let Some(row) = SHOWN.get(index).copied() {
                self.pair_chosen(host, row)?;
            }
        }
        Ok(ItemList::handled(step))
    }

    fn home_chosen(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        if index == 0 {
            return self.sync_now(host);
        }
        self.editing = index.clamp(1, PAIRS);
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).open(Open::screen("file-sync.pair")?)
    }

    fn pair_chosen(&mut self, host: &Host<'_>, row: Row) -> Result<(), Error> {
        if row == Row::Mode {
            return self.toggle_mode(host);
        }
        self.ask_row(host, row)
    }

    fn toggle_mode(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let pair = pairs::read(host, self.editing)?;
        let next = if pair.mode == crate::native_plan::Mode::TwoWay {
            MODE[1]
        } else {
            MODE[0]
        };
        let key = pairs::key("mode", self.editing)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).set_setting(&key, SettingValue::Text(next))?;
        self.refresh_home(host)
    }

    /// Asks for the text of `row` (the password masked).
    fn ask_row(&mut self, host: &Host<'_>, row: Row) -> Result<(), Error> {
        self.ensure(host)?;
        let mut say = Messages::new()?;
        let title = say.copy(host, row.title())?;
        let pair = pairs::read(host, self.editing)?;
        let initial = match row {
            Row::Folder => Some(copy(&pair.folder)?),
            Row::Server => Some(copy(&pair.url)?),
            Row::User => Some(copy(&pair.user)?),
            _ => None,
        };
        let question = match row {
            Row::Password => Question::text(&title, "")?.secret()?,
            Row::Server => Question::text(&title, "")?.mode(TextMode::Url)?,
            _ => Question::text(&title, "")?,
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let asked = ui.ask(host, question, &mut input, &mut out);
        // The question stays open when the filling fails (the field is then
        // empty); it is tracked either way so its answer is still taken.
        if let (Ok(request), Some(initial)) = (&asked, initial.as_deref().filter(|i| !i.is_empty()))
        {
            ui.text_write(
                host,
                request.text(),
                0,
                initial,
                TextWrite::Last,
                &mut input,
                &mut out,
            )
            .ok();
        }
        wipe(&mut input);
        wipe(&mut out);
        self.typing = Some((asked?, row));
        Ok(())
    }

    /// An answer to a question the plugin asked.
    pub(crate) fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
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

    /// Keeps what was typed for `row`. An empty password keeps what was kept
    /// (the field never shows it): a confirm by mistake loses nothing; one is
    /// taken away with a single space. Another server takes the password away.
    fn apply(&mut self, host: &Host<'_>, row: Row, typed: &str) -> Result<(), Error> {
        let text = typed.trim();
        let n = self.editing;
        let mut say = Messages::new()?;
        match row {
            Row::Folder => {
                let folder = folder_of(text)?;
                if !text.is_empty() && folder.is_empty() {
                    let said = say.copy(host, "file-sync.bad-folder")?;
                    self.notice(host, &said);
                    return Ok(());
                }
                self.set_text(host, &pairs::key("folder", n)?, &folder)?;
            }
            Row::Server => {
                if !text.is_empty() && folder_url(text).is_none() {
                    let said = say.copy(host, "file-sync.bad-url")?;
                    self.notice(host, &said);
                    return Ok(());
                }
                if !self.password_goes_with(host, text)? {
                    return Ok(());
                }
                self.set_text(host, &pairs::key("server", n)?, text)?;
            }
            Row::User => self.set_text(host, &pairs::key("user", n)?, text)?,
            Row::Password => {
                if typed.is_empty() {
                    return Ok(());
                }
                if !self.keep_password(host, pairs::password_of(typed))? {
                    return Ok(());
                }
            }
            Row::Mode => return Ok(()),
        }
        // On to the next row.
        let at = SHOWN.iter().position(|r| *r == row).unwrap_or(0);
        if let Some(top) = self.pair_view.top(PAIR) {
            top.list.set_focus((at + 1).min(SHOWN.len() - 1));
        }
        self.refresh_home(host)
    }

    fn set_text(&self, host: &Host<'_>, key: &str, value: &str) -> Result<(), Error> {
        let mut scratch = [0; 1200];
        CommandSender::new(host, &mut scratch).set_setting(key, SettingValue::Text(value))
    }

    /// Whether the pair being edited may take the server address `url`: the
    /// kept password is for the server it was typed for, so an address of
    /// another server takes it away (said). `false` where it cannot be taken
    /// away: the address stays as it was then.
    fn password_goes_with(&self, host: &Host<'_>, url: &str) -> Result<bool, Error> {
        let old = pairs::read(host, self.editing)?.url;
        if old.is_empty() || pairs::same_server(&old, url) || !self.password_kept(host)? {
            return Ok(true);
        }
        if !self.keep_password(host, "")? {
            return Ok(false);
        }
        let said = Messages::new()?.copy(host, "file-sync.password-dropped")?;
        self.notice(host, &said);
        Ok(true)
    }

    /// Keeps the password (empty takes it away); said where it cannot be.
    /// `true` where it was kept.
    fn keep_password(&self, host: &Host<'_>, password: &str) -> Result<bool, Error> {
        let mut say = Messages::new()?;
        let name = pairs::secret_name(self.editing)?;
        let notice = match &self.secrets {
            None => Some("core.secrets-unsupported"),
            Some(_) if password.len() > MAX_PASSWORD => Some("file-sync.not-kept"),
            Some(secrets) => {
                let label = say.format(
                    host,
                    "file-sync.secret.password",
                    &[Arg::Int {
                        name: "n",
                        value: self.editing as i64,
                    }],
                )?;
                match keep::keep(secrets, host, &name, &label, password) {
                    Ok(()) => None,
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(Error::Unsupported) => Some("core.secrets-unsupported"),
                    Err(_) => Some("file-sync.not-kept"),
                }
            }
        };
        let Some(key) = notice else {
            return Ok(true);
        };
        let text = say.copy(host, key)?;
        self.notice(host, &text);
        Ok(false)
    }

    /// The pair's number of the first open screen state, for a new view.
    pub(crate) fn open_home(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let mut view = View::default();
        self.build_home(host, &mut view)?;
        self.home.open(instance, view)
    }

    pub(crate) fn open_pair(
        &mut self,
        host: &Host<'_>,
        instance: paperpaw_sdk::native::ui::Instance,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let mut view = View::default();
        self.build_pair(host, &mut view)?;
        self.pair_view.open(instance, view)
    }
}
