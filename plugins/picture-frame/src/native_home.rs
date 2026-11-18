//! The home screen of the Picture frame: the slideshow, the fetch from the
//! photo server and the server's account (its address, the user and the
//! password, which is kept among the plugin's secrets, the rest as settings).
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    events::{AnswerOutcome, Answered},
    formatting::Arg,
    settings::SettingValue,
    ui::{Question, TextMode, TextWrite},
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{SERVICE_BUFFER, buffer, push, wipe},
};

use crate::native_keep::{self as keep, PASSWORD, Secret};
use crate::native_link::folder_url;
use crate::native_plugin::{NativePictureFrame, SERVER, USER};

/// What a kept secret is shown as.
const DOTS: &str = "••••••";
/// The longest password kept (the header must fit a request).
const MAX_PASSWORD: usize = 400;

/// The rows of the home screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Slideshow,
    Sync,
    Server,
    User,
    Password,
}

const SHOWN: [Row; 5] = [
    Row::Slideshow,
    Row::Sync,
    Row::Server,
    Row::User,
    Row::Password,
];

impl Row {
    const fn title(self) -> &'static str {
        match self {
            Self::Slideshow => "picture-frame.slideshow",
            Self::Sync => "picture-frame.sync",
            Self::Server => "picture-frame.setting.server",
            Self::User => "picture-frame.setting.user",
            Self::Password => "picture-frame.password",
        }
    }
}

/// The home screen: its rows and the note below them.
#[derive(Debug, Default)]
pub struct Home {
    list: ItemList,
    note: String,
}

/// The footer for a note: none where it is empty.
fn note(text: &str) -> Footer<'_> {
    if text.is_empty() {
        Footer::None
    } else {
        Footer::Note(text)
    }
}

impl NativePictureFrame {
    /// The rows and the note below them, as things are now.
    pub(crate) fn build_home(&self, host: &Host<'_>, view: &mut Home) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let mut items = Vec::new();
        for row in SHOWN {
            let value = match row {
                Row::Slideshow => say.format(
                    host,
                    "picture-frame.count",
                    &[Arg::Count(self.photos.len() as i64)],
                )?,
                Row::Sync => paperpaw_ui_kit::text::copy("›")?,
                Row::Server => Self::text_setting(host, SERVER)?,
                Row::User => Self::text_setting(host, USER)?,
                Row::Password => {
                    paperpaw_ui_kit::text::copy(if self.password_kept(host)? { DOTS } else { "" })?
                }
            };
            let title = say.copy(host, row.title())?;
            push(&mut items, Item::row(&title, &value)?)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus);
        if view.note.is_empty() {
            view.note = if self.said.is_empty() {
                say.copy(host, "picture-frame.help")?
            } else {
                paperpaw_ui_kit::text::copy(&self.said)?
            };
        }
        Ok(())
    }

    /// Whether the password is kept (a device without secrets keeps none).
    fn password_kept(&self, host: &Host<'_>) -> Result<bool, Error> {
        match &self.secrets {
            Some(secrets) => keep::present(secrets, host, PASSWORD),
            None => Ok(false),
        }
    }

    /// Rebuilds the rows of the open home screen and asks for a redraw.
    pub(crate) fn refresh_home(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut view = Home::default();
        let Some(top) = self.home.top(crate::declarations::HOME) else {
            return Ok(());
        };
        view.list.set_focus(top.list.focus());
        view.note = paperpaw_ui_kit::text::copy(&self.said)?;
        self.build_home(host, &mut view)?;
        if let Some(top) = self.home.top(crate::declarations::HOME) {
            *top = view;
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    pub(crate) fn draw_home(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        let view = self
            .home
            .top(crate::declarations::HOME)
            .ok_or(Error::State)?;
        let title = Messages::new()?.copy(host, "picture-frame.title")?;
        let footer = note(&view.note);
        view.list.draw_with(host, frame, &title, true, footer)
    }

    pub(crate) fn home_input(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(view) = self.home.top(crate::declarations::HOME) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, input)?;
        if let ItemStep::Chosen(index) = step
            && let Some(row) = SHOWN.get(index).copied()
        {
            self.chosen(host, row)?;
        }
        Ok(ItemList::handled(step))
    }

    fn chosen(&mut self, host: &Host<'_>, row: Row) -> Result<(), Error> {
        match row {
            Row::Slideshow => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(paperpaw_sdk::native::ui::Open::screen(
                    "picture-frame.show",
                )?)
            }
            Row::Sync => self.sync_now(host),
            Row::Server | Row::User | Row::Password => self.ask_row(host, row),
        }
    }

    /// Asks for the text of `row` (the password masked).
    fn ask_row(&mut self, host: &Host<'_>, row: Row) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let title = say.copy(host, row.title())?;
        let initial = match row {
            Row::Server => Some(Self::text_setting(host, SERVER)?),
            Row::User => Some(Self::text_setting(host, USER)?),
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
    /// taken away with a single space.
    fn apply(&mut self, host: &Host<'_>, row: Row, typed: &str) -> Result<(), Error> {
        let text = typed.trim();
        match row {
            Row::Server => {
                if !text.is_empty() && folder_url(text).is_none() {
                    let said = Messages::new()?.copy(host, "picture-frame.bad-url")?;
                    self.notice(host, &said);
                    return Ok(());
                }
                let mut scratch = [0; 1200];
                CommandSender::new(host, &mut scratch)
                    .set_setting(SERVER, SettingValue::Text(text))?;
            }
            Row::User => {
                let mut scratch = [0; 1200];
                CommandSender::new(host, &mut scratch)
                    .set_setting(USER, SettingValue::Text(text))?;
            }
            Row::Password => {
                if typed.is_empty() {
                    return Ok(());
                }
                self.keep_password(host, text)?;
            }
            Row::Slideshow | Row::Sync => return Ok(()),
        }
        // On to the next row.
        let at = SHOWN.iter().position(|r| *r == row).unwrap_or(0);
        if let Some(top) = self.home.top(crate::declarations::HOME) {
            top.list.set_focus((at + 1).min(SHOWN.len() - 1));
        }
        self.refresh_home(host)
    }

    /// Keeps the password (an empty text takes it away); said where it cannot be.
    fn keep_password(&self, host: &Host<'_>, password: &str) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let notice = match &self.secrets {
            None => Some("core.secrets-unsupported"),
            Some(_) if password.len() > MAX_PASSWORD => Some("picture-frame.not-kept"),
            Some(secrets) => {
                let label = say.copy(host, "picture-frame.secret.password")?;
                match keep::keep(secrets, host, PASSWORD, &label, password) {
                    Ok(()) => None,
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(Error::Unsupported) => Some("core.secrets-unsupported"),
                    Err(_) => Some("picture-frame.not-kept"),
                }
            }
        };
        if let Some(key) = notice {
            let text = say.copy(host, key)?;
            self.notice(host, &text);
        }
        Ok(())
    }
}
