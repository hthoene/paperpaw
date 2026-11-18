//! The sync's shell: what a chosen row does (ask for the folder, the user or
//! the password with the host's keyboard, start a sync, take over KOReader's
//! statistics), what a finished request and an answered question lead to, and
//! the messages shown on the screen. The engine is `native_webdav`, the calls
//! are `native_webdav_net`, the view is `native_sync_screen`.
use crate::files::DataFolder;
use crate::native_sync_screen::{Row, Shown, SyncView};
use crate::native_webdav::{self as webdav, Both, Facts, Say};
use crate::native_webdav_net::{Dav, Kept, Outcome};
use crate::texts::{HostWords, Words};
use alloc::string::String;
use paperpaw_sdk::native::commands::{CommandSender, RedrawMode};
use paperpaw_sdk::native::events::{AnswerOutcome, Answered};
use paperpaw_sdk::native::net::HttpDone;
use paperpaw_sdk::native::settings::{self, SettingValue};
use paperpaw_sdk::native::storage::{self as host_storage, DataStorage, Root};
use paperpaw_sdk::native::ui::{Notify, Question, RequestId, TextMode, TextWrite, Ui};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{DirEntry, Storage, StorageError};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer, wipe_text};
use paperpaw_ui_kit::text::{concat, copy};
use paperpaw_ui_kit::{Messages, uitext};

/// The settings of the folder and the user.
const URL: &str = "stats.webdav-url";
const USER: &str = "stats.webdav-user";
/// The longest the keyboard's answer may be (a folder address is 1,024).
const TYPED_LIMIT: usize = 1_024;
/// How long a go-online lasts.
const ONLINE_MS: u32 = 600_000;

/// What an open question is for.
#[derive(Debug, PartialEq, Eq)]
enum Asked {
    Folder,
    User,
    Password,
    /// Whether to remove here what was removed there, for the file with this
    /// ETag.
    Decide {
        etag: String,
    },
    /// Whether to go online for the sync.
    Online,
}

/// The sync: the driver, the question open, whether the offer to go online
/// was made for this sync.
#[derive(Debug, Default)]
pub(crate) struct SyncShell {
    dav: Dav,
    asked: Option<(RequestId, Asked)>,
    offered: bool,
}

/// The user's files (wire root 1), read only.
struct Card<'h, 'cb> {
    host: &'h Host<'cb>,
}

impl Storage for Card<'_, '_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        host_storage::read_at(self.host, Root::Files, path, offset, buf).map_err(refused)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        host_storage::size(self.host, Root::Files, path).map_err(refused)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }
}

fn refused(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Invalid => StorageError::InvalidPath,
        _ => StorageError::Io,
    }
}

/// A text setting, or empty where it is not set.
fn setting(host: &Host<'_>, key: &str) -> Result<String, Error> {
    let mut scratch = buffer(SERVICE_BUFFER)?;
    match settings::read_setting(host, key, &mut scratch) {
        Ok(SettingValue::Text(text)) => copy(text),
        Ok(_) | Err(Error::NotFound) => Ok(String::new()),
        Err(error) => Err(error),
    }
}

/// Sets the text setting `key`.
fn set(host: &Host<'_>, key: &str, value: &str) -> Result<(), Error> {
    let mut bytes = buffer(value.len().saturating_add(256))?;
    CommandSender::new(host, &mut bytes).set_setting(key, SettingValue::Text(value))
}

/// Asks for the screen to be drawn again (it may be gone: not an error).
fn redraw(host: &Host<'_>) {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(RedrawMode::Partial)
        .unwrap_or(());
}

impl SyncShell {
    /// Moves a password of an earlier version into the secrets (at the start).
    /// Best effort: the recording goes on whatever it says.
    pub(crate) fn started(&mut self, host: &Host<'_>) {
        self.dav.move_old_password(host).unwrap_or(());
    }

    /// The view of what is set now.
    ///
    /// # Errors
    /// `NoMemory` and what the host says.
    pub(crate) fn view(&mut self, host: &Host<'_>) -> Result<SyncView, Error> {
        let (folder, user) = (setting(host, URL)?, setting(host, USER)?);
        let shown = self.shown(host, &folder, &user)?;
        SyncView::new(&mut HostWords::new(host)?, shown)
    }

    fn shown<'a>(
        &mut self,
        host: &Host<'_>,
        folder: &'a str,
        user: &'a str,
    ) -> Result<Shown<'a>, Error> {
        Ok(Shown {
            folder,
            user,
            password: self.dav.has_password(host)?,
            koreader: webdav::koreader_file(&Card { host }).is_some(),
        })
    }

    /// Makes the rows show what is set now.
    fn refresh(&mut self, host: &Host<'_>, view: &mut SyncView) -> Result<(), Error> {
        let (folder, user) = (setting(host, URL)?, setting(host, USER)?);
        let shown = self.shown(host, &folder, &user)?;
        view.refresh(&mut HostWords::new(host)?, shown)
    }

    /// A chosen row.
    ///
    /// # Errors
    /// `NoMemory` and what the host says.
    pub(crate) fn activate(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        view: &mut SyncView,
        row: Row,
    ) -> Result<(), Error> {
        match row {
            Row::Folder => self.ask_text(
                host,
                ui,
                "stats.sync.folder",
                &setting(host, URL)?,
                Asked::Folder,
            ),
            Row::User => self.ask_text(
                host,
                ui,
                "stats.sync.user",
                &setting(host, USER)?,
                Asked::User,
            ),
            Row::Password => self.ask_text(host, ui, "stats.sync.password", "", Asked::Password),
            Row::SyncNow => {
                if self.dav.running() {
                    return Ok(());
                }
                self.offered = false;
                self.begin(host, ui, Some(view))
            }
            Row::Import => {
                let data = DataStorage::new(host);
                let folder = DataFolder(&data);
                let card = Card { host };
                let both = Both {
                    data: &folder,
                    card: &card,
                };
                let say = webdav::import(&both, &card)?;
                self.say(host, Some(view), &say)
            }
        }
    }

    /// Starts a sync with what is set now.
    fn begin(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        view: Option<&mut SyncView>,
    ) -> Result<(), Error> {
        let (folder, user) = (setting(host, URL)?, setting(host, USER)?);
        let facts = Facts {
            network: true,
            folder: &folder,
            user: &user,
        };
        let data = DataStorage::new(host);
        let outcome = self.dav.start(host, &DataFolder(&data), facts)?;
        self.after(host, ui, view, outcome)
    }

    /// What a call of the driver left to do.
    fn after(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        mut view: Option<&mut SyncView>,
        outcome: Outcome,
    ) -> Result<(), Error> {
        match outcome {
            Outcome::Waiting => self.say(host, view, &Say::Running),
            Outcome::Said(say) => self.say(host, view, &say),
            Outcome::Asked { etag } => {
                self.say(host, view.as_deref_mut(), &Say::Asked)?;
                let mut words = HostWords::new(host)?;
                let title = words.copy("stats.sync.ask")?;
                let text = words.copy("stats.sync.ask.text")?;
                let labels = concat(&[
                    &words.copy("stats.sync.remove")?,
                    "\n",
                    &words.copy("stats.sync.keep")?,
                ])?;
                let question = Question::confirm(&title, &text)?.confirm_labels(&labels)?;
                self.ask(host, ui, question, Asked::Decide { etag })
            }
            Outcome::Offline if !self.offered => {
                // Offline mode: asked once for this sync.
                self.offered = true;
                let mut messages = Messages::new()?;
                let title = messages.copy(host, "core.offline.text")?;
                let labels = concat(&[
                    &messages.copy(host, "core.offline.go-online")?,
                    "\n",
                    &messages.copy(host, "core.offline.not-now")?,
                ])?;
                let question = Question::confirm(&title, "")?.confirm_labels(&labels)?;
                self.ask(host, ui, question, Asked::Online)
            }
            Outcome::Offline => self.say(host, view, &Say::NoNetwork),
        }
    }

    /// Shows `say` below the rows.
    fn say(
        &mut self,
        host: &Host<'_>,
        view: Option<&mut SyncView>,
        say: &Say,
    ) -> Result<(), Error> {
        if let Some(view) = view {
            view.say(say.text(&mut HostWords::new(host)?)?);
            redraw(host);
        }
        Ok(())
    }

    fn ask(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        question: Question<'_>,
        asked: Asked,
    ) -> Result<(), Error> {
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = ui.ask(host, question, &mut input, &mut output)?;
        self.asked = Some((request, asked));
        Ok(())
    }

    /// Asks for text on the keyboard, with `initial` in it.
    fn ask_text(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        title: &str,
        initial: &str,
        asked: Asked,
    ) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, title)?;
        let question = Question::text(&title, "")?;
        let question = match asked {
            Asked::Folder => question.mode(TextMode::Url)?,
            Asked::Password => question.secret()?,
            _ => question,
        };
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = ui.ask(host, question, &mut input, &mut output)?;
        if !initial.is_empty() {
            ui.text_write(
                host,
                request.text(),
                0,
                initial,
                TextWrite::Last,
                &mut input,
                &mut output,
            )?;
        }
        self.asked = Some((request, asked));
        Ok(())
    }

    /// The end of a request.
    ///
    /// # Errors
    /// `NoMemory` and what the host says.
    pub(crate) fn http_done(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        view: Option<&mut SyncView>,
        done: &HttpDone,
    ) -> Result<(), Error> {
        let (folder, user) = (setting(host, URL)?, setting(host, USER)?);
        let facts = Facts {
            network: true,
            folder: &folder,
            user: &user,
        };
        let data = DataStorage::new(host);
        match self.dav.done(host, &DataFolder(&data), facts, done)? {
            Some(outcome) => self.after(host, ui, view, outcome),
            None => Ok(()),
        }
    }

    /// An answer of the host to a question: what it was for, done.
    ///
    /// # Errors
    /// `NoMemory` and what the host says.
    pub(crate) fn answered(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        mut view: Option<&mut SyncView>,
        answer: &Answered,
    ) -> Result<(), Error> {
        let Some((request, _)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, asked)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match asked {
            Asked::Folder => {
                let typed = uitext::read_all(ui, host, request.text(), TYPED_LIMIT)?;
                set(host, URL, &webdav::folder(&typed)?)?;
                self.entered(host, view, Row::User)
            }
            Asked::User => {
                let typed = uitext::read_all(ui, host, request.text(), TYPED_LIMIT)?;
                set(host, USER, typed.trim())?;
                self.entered(host, view, Row::Password)
            }
            Asked::Password => {
                let typed = uitext::read_all(ui, host, request.text(), TYPED_LIMIT)?;
                let kept = self.dav.keep_password(host, &typed);
                wipe_text(typed);
                if kept? == Kept::Unsupported {
                    let text = Messages::new()?.copy(host, "core.secrets-unsupported")?;
                    let mut scratch = [0; 512];
                    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)?;
                }
                self.entered(host, view, Row::SyncNow)
            }
            Asked::Decide { etag } => {
                let remove = answer.choice() == 1;
                let (folder, user) = (setting(host, URL)?, setting(host, USER)?);
                let facts = Facts {
                    network: true,
                    folder: &folder,
                    user: &user,
                };
                let data = DataStorage::new(host);
                match self
                    .dav
                    .decide(host, &DataFolder(&data), facts, (&etag, remove))?
                {
                    Some(outcome) => self.after(host, ui, view.as_deref_mut(), outcome),
                    None => Ok(()),
                }
            }
            Asked::Online => {
                if answer.choice() == 1 {
                    // Refused (a device that cannot): the sync is tried all the
                    // same and says again that the device is offline.
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch)
                        .go_online(ONLINE_MS)
                        .unwrap_or(());
                    self.begin(host, ui, view.as_deref_mut())
                } else {
                    self.say(host, view, &Say::NoNetwork)
                }
            }
        }
    }

    /// A value was typed: the rows show it, the focus goes on to `next`.
    fn entered(
        &mut self,
        host: &Host<'_>,
        view: Option<&mut SyncView>,
        next: Row,
    ) -> Result<(), Error> {
        if let Some(view) = view {
            self.refresh(host, view)?;
            view.focus(next);
            redraw(host);
        }
        Ok(())
    }
}
