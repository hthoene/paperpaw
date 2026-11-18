//! The questions the screens ask and what their answers do: which view to
//! show, what to do with a source, and the questions that make a source (its
//! kind, address, user name, password and name). One question is open at a
//! time.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    events::{AnswerOutcome, Answered},
    formatting::Arg,
    ui::{Question, RequestId, TextMode, TextWrite},
};
use paperpaw_ui_kit::{
    Messages,
    dav::secret::{self, Secret},
    mem::{SERVICE_BUFFER, buffer, push, wipe},
    text::copy,
};

use crate::model::{Kind, NAME_MAX, URL_MAX, USER_MAX, clean};
use crate::native_plugin::{NativeCalendar, SOURCES, notice, open, secret_name};
use crate::native_screens::{Mode, kind_key, week_of};

/// What the views' menu offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    Agenda,
    Day,
    Week,
    Month,
    Birthdays,
    Sources,
    Sync,
}

/// What can be done with a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    Sync,
    Name,
    Address,
    User,
    Password,
    Shade,
    Remove,
}

/// A source being made: what was answered so far.
#[derive(Debug)]
pub(crate) struct Draft {
    kind: Kind,
    url: String,
    user: String,
    password: Option<Secret>,
}

/// What a question was for.
#[derive(Debug)]
pub(crate) enum Asked {
    /// Which view to show, or what to do besides.
    Views(Vec<Choice>),
    /// What to do with this source.
    Source(u8, Vec<Act>),
    /// The kind of a new source, and then its address, user name, password
    /// and name.
    Kind,
    Address,
    User,
    Password,
    Name,
    /// A field of a source that is there, asked for anew.
    Field(u8, Act),
    /// Whether to remove the source.
    Remove(u8),
}

const KINDS: [Kind; 4] = [Kind::Caldav, Kind::Ics, Kind::File, Kind::Contacts];

/// A name for a source from its address: the host of a server, the name of a
/// file.
fn default_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let name = if url.contains("://") {
        rest.split('/').next().unwrap_or("")
    } else {
        rest.trim_end_matches('/').rsplit('/').next().unwrap_or("")
    };
    clean(name, NAME_MAX).unwrap_or_default()
}

impl NativeCalendar {
    /// Asks to choose one of `labels`.
    fn ask_choice(
        &mut self,
        host: &Host<'_>,
        title: &str,
        labels: &[String],
        asked: Asked,
    ) -> Result<(), Error> {
        if self.asked.is_some() {
            return Ok(());
        }
        let choices = paperpaw_ui_kit::mem::join(labels, "\n")?;
        let question = Question::choice(title, "", &choices)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self
            .handles
            .ui(host)?
            .ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, asked));
        Ok(())
    }

    /// Asks for a text on the keyboard, with `initial` to start with.
    fn ask_text(
        &mut self,
        host: &Host<'_>,
        title: &str,
        initial: &str,
        (asked, mode): (Asked, Option<TextMode>),
    ) -> Result<(), Error> {
        if self.asked.is_some() {
            return Ok(());
        }
        let secret_field = matches!(asked, Asked::Password | Asked::Field(_, Act::Password));
        let mut question = Question::text(title, "")?;
        if secret_field {
            question = question.secret()?;
        } else if let Some(mode) = mode {
            question = question.mode(mode)?;
        }
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.handles.ui(host)?;
        let request = ui.ask(host, question, &mut input, &mut out)?;
        // The question stays open when the filling fails (the field is then
        // empty); it is tracked either way so its answer is still taken.
        if !initial.is_empty() {
            let _ = ui.text_write(
                host,
                request.text(),
                0,
                initial,
                TextWrite::Last,
                &mut input,
                &mut out,
            );
        }
        self.asked = Some((request, asked));
        Ok(())
    }

    /// What was typed for the question `request`, wiped when let go.
    fn typed(&mut self, host: &Host<'_>, request: RequestId) -> Result<Secret, Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.handles.ui(host)?;
        let text = match ui.text_read(host, request.text(), 0, &mut input, &mut out) {
            Ok(part) if part.data().len() as u64 == u64::from(part.total()) => {
                Secret::copy(part.data())
            }
            Ok(_) => Err(Error::NoSpace),
            Err(error) => Err(error),
        };
        wipe(&mut input);
        wipe(&mut out);
        text
    }

    /// The views' menu: each view, the sources and *Sync now*.
    pub(crate) fn ask_views(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let (mut picks, mut labels) = (Vec::new(), Vec::new());
        for (pick, key) in [
            (Choice::Agenda, "calendar.view.agenda"),
            (Choice::Day, "calendar.view.day"),
            (Choice::Week, "calendar.view.week"),
            (Choice::Month, "calendar.view.month"),
            (Choice::Birthdays, "calendar.view.birthdays"),
            (Choice::Sources, "calendar.view.sources"),
            (Choice::Sync, "calendar.action.sync"),
        ] {
            push(&mut picks, pick)?;
            push(&mut labels, say.copy(host, key)?)?;
        }
        let title = say.copy(host, "calendar.view.title")?;
        self.ask_choice(host, &title, &labels, Asked::Views(picks))
    }

    /// What can be done with a source.
    pub(crate) fn ask_source(&mut self, host: &Host<'_>, id: u8) -> Result<(), Error> {
        if !self.usable(host)? {
            return Ok(());
        }
        let Some(source) = self.store.source(id) else {
            return Ok(());
        };
        let (account, name) = (source.kind.account(), copy(&source.name)?);
        let mut say = Messages::new()?;
        let (mut acts, mut labels) = (Vec::new(), Vec::new());
        for (act, key, wanted) in [
            (Act::Sync, "calendar.source.sync", true),
            (Act::Name, "calendar.source.name", true),
            (Act::Address, "calendar.source.address", true),
            (Act::User, "calendar.source.user", account),
            (Act::Password, "calendar.source.password", account),
            (Act::Shade, "calendar.source.shade", true),
            (Act::Remove, "calendar.source.remove", true),
        ] {
            if wanted {
                push(&mut acts, act)?;
                push(&mut labels, say.copy(host, key)?)?;
            }
        }
        self.ask_choice(host, &name, &labels, Asked::Source(id, acts))
    }

    /// *Add a calendar*: asks for its kind.
    pub(crate) fn ask_kind(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if !self.usable(host)? {
            return Ok(());
        }
        let mut say = Messages::new()?;
        let mut labels = Vec::new();
        for kind in KINDS {
            push(&mut labels, say.copy(host, kind_key(kind))?)?;
        }
        let title = say.copy(host, "calendar.source.add")?;
        self.ask_choice(host, &title, &labels, Asked::Kind)
    }

    fn ask_for(&mut self, host: &Host<'_>, asked: Asked, initial: &str) -> Result<(), Error> {
        let (key, mode) = match &asked {
            Asked::Address | Asked::Field(_, Act::Address) => {
                let file = self
                    .draft
                    .as_ref()
                    .map_or_else(|| false, |draft| draft.kind == Kind::File)
                    || matches!(asked, Asked::Field(id, _) if self.store.source(id).is_some_and(|s| s.kind == Kind::File));
                (
                    if file {
                        "calendar.source.path"
                    } else {
                        "calendar.source.address"
                    },
                    (!file).then_some(TextMode::Url),
                )
            }
            Asked::User | Asked::Field(_, Act::User) => ("calendar.source.user", None),
            Asked::Password | Asked::Field(_, Act::Password) => ("calendar.source.password", None),
            _ => ("calendar.source.name", None),
        };
        let title = Messages::new()?.copy(host, key)?;
        self.ask_text(host, &title, initial, (asked, mode))
    }

    /// An answer to a question the plugin asked.
    pub(crate) fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        if !matches!(&self.asked, Some((request, _)) if *request == answer.request()) {
            return Ok(());
        }
        let Some((request, asked)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            self.draft = None;
            return Ok(());
        }
        let choice = usize::try_from(answer.choice()).unwrap_or(usize::MAX);
        match asked {
            Asked::Views(picks) => match picks.get(choice) {
                Some(pick) => self.picked(host, *pick),
                None => Ok(()),
            },
            Asked::Source(id, acts) => match acts.get(choice) {
                Some(act) => self.act(host, id, *act),
                None => Ok(()),
            },
            Asked::Kind => match KINDS.get(choice) {
                Some(kind) => {
                    self.draft = Some(Draft {
                        kind: *kind,
                        url: String::new(),
                        user: String::new(),
                        password: None,
                    });
                    self.ask_for(host, Asked::Address, "")
                }
                None => Ok(()),
            },
            Asked::Remove(id) => {
                if choice == 1 {
                    self.remove_source(host, id)?;
                }
                Ok(())
            }
            typed_for => {
                let typed = self.typed(host, request)?;
                self.text_answered(host, typed_for, typed)
            }
        }
    }

    /// A choice of the views' menu.
    fn picked(&mut self, host: &Host<'_>, pick: Choice) -> Result<(), Error> {
        let (day, week_start) = self.view_start(host)?;
        let week_start = u8::try_from(week_start).unwrap_or(1);
        let today = crate::native_plugin::clock_now(host)?.today;
        // A dated view keeps its day, the others begin today.
        let day = if matches!(self.mode, Mode::Agenda | Mode::Birthdays | Mode::Detail) {
            today
        } else {
            day
        };
        match pick {
            Choice::Agenda => self.set_view(host, Mode::Agenda),
            Choice::Day => self.set_view(host, Mode::Day(day)),
            Choice::Week => self.set_view(host, Mode::Week(week_of(day, week_start))),
            Choice::Month => self.set_view(host, Mode::Month(day)),
            Choice::Birthdays => self.set_view(host, Mode::Birthdays),
            Choice::Sources => open(host, SOURCES),
            Choice::Sync => self.sync_now(host),
        }
    }

    /// Something to do with the source `id`.
    fn act(&mut self, host: &Host<'_>, id: u8, act: Act) -> Result<(), Error> {
        let Some(source) = self.store.source(id) else {
            return Ok(());
        };
        let mut say = Messages::new()?;
        match act {
            Act::Sync => self.sync_source(host, Some(id)),
            Act::Shade => {
                if let Some(source) = self.store.source_mut(id) {
                    source.shade = (source.shade + 1) % 4;
                }
                self.changed(host)
            }
            Act::Remove => {
                let title = say.format(
                    host,
                    "calendar.source.remove.ask",
                    &[Arg::Text {
                        name: "name",
                        value: &source.name,
                    }],
                )?;
                let labels = [
                    say.copy(host, "calendar.source.remove.no")?,
                    say.copy(host, "calendar.source.remove.yes")?,
                ];
                self.ask_choice(host, &title, &labels, Asked::Remove(id))
            }
            Act::Name | Act::Address | Act::User => {
                let initial = match act {
                    Act::Name => copy(&source.name)?,
                    Act::Address => copy(&source.url)?,
                    _ => copy(&source.user)?,
                };
                self.ask_for(host, Asked::Field(id, act), &initial)
            }
            // A password is never shown in its field.
            Act::Password => self.ask_for(host, Asked::Field(id, act), ""),
        }
    }

    fn remove_source(&mut self, host: &Host<'_>, id: u8) -> Result<(), Error> {
        self.store.remove_source(id);
        if let Ok(secrets) = self.handles.secrets(host) {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            if let Ok(name) = paperpaw_sdk::native::secrets::SecretName::new(&secret_name(id)?) {
                let _ = secrets.remove(host, name, &mut input, &mut out);
            }
        }
        self.changed(host)
    }

    /// Keeps the password of source `id` with the reader's passwords.
    fn keep_password(&mut self, host: &Host<'_>, id: u8, password: &str) -> Result<(), Error> {
        let name = self.store.source(id).map_or("", |s| s.name.as_str());
        let label = Messages::new()?.format(
            host,
            "calendar.secret",
            &[Arg::Text {
                name: "name",
                value: name,
            }],
        )?;
        let kept = match self.handles.secrets(host) {
            Ok(secrets) => secret::keep(secrets, host, &secret_name(id)?, &label, password),
            Err(error) => Err(error),
        };
        match kept {
            Ok(()) => Ok(()),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => {
                notice(host, "calendar.password.not-kept");
                Ok(())
            }
        }
    }

    /// A text typed for a source being made or changed.
    fn text_answered(&mut self, host: &Host<'_>, asked: Asked, typed: Secret) -> Result<(), Error> {
        let text = typed.as_str().trim();
        match asked {
            Asked::Address => {
                let Some(draft) = self.draft.as_mut() else {
                    return Ok(());
                };
                if text.is_empty() {
                    self.draft = None;
                    return Ok(());
                }
                draft.url = clean(text, URL_MAX)?;
                let next = if draft.kind.account() {
                    Asked::User
                } else {
                    Asked::Name
                };
                let initial = if matches!(next, Asked::Name) {
                    default_name(&draft.url)
                } else {
                    String::new()
                };
                self.ask_for(host, next, &initial)
            }
            Asked::User => {
                let Some(draft) = self.draft.as_mut() else {
                    return Ok(());
                };
                draft.user = clean(text, USER_MAX)?;
                let next = if draft.user.is_empty() {
                    Asked::Name
                } else {
                    Asked::Password
                };
                let initial = if matches!(next, Asked::Name) {
                    default_name(&draft.url)
                } else {
                    String::new()
                };
                self.ask_for(host, next, &initial)
            }
            Asked::Password => {
                let Some(draft) = self.draft.as_mut() else {
                    return Ok(());
                };
                draft.password = Some(Secret::copy(typed.as_str())?);
                let initial = default_name(&draft.url);
                self.ask_for(host, Asked::Name, &initial)
            }
            Asked::Name => self.make_source(host, text),
            Asked::Field(id, act) => self.field_answered(host, id, act, typed.as_str()),
            _ => Ok(()),
        }
    }

    /// The source is made from the answers: kept, shown, and read at once.
    fn make_source(&mut self, host: &Host<'_>, name: &str) -> Result<(), Error> {
        let Some(draft) = self.draft.take() else {
            return Ok(());
        };
        let name = if name.is_empty() {
            default_name(&draft.url)
        } else {
            copy(name)?
        };
        match self
            .store
            .add_source(draft.kind, &name, &draft.url, &draft.user)
        {
            Ok(id) => {
                if let Some(password) = &draft.password {
                    self.keep_password(host, id, password.as_str())?;
                }
                self.changed(host)?;
                self.sync_source(host, Some(id))
            }
            Err(Error::Full) => {
                notice(host, "calendar.source.full");
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// A field of a source typed anew.
    fn field_answered(
        &mut self,
        host: &Host<'_>,
        id: u8,
        act: Act,
        text: &str,
    ) -> Result<(), Error> {
        let text = text.trim();
        match act {
            Act::Password => {
                self.keep_password(host, id, text)?;
                self.source_changed(id);
            }
            Act::Name | Act::Address | Act::User => {
                let Some(source) = self.store.source_mut(id) else {
                    return Ok(());
                };
                match act {
                    Act::Name => source.name = clean(text, NAME_MAX)?,
                    Act::Address if !text.is_empty() => {
                        source.url = clean(text, URL_MAX)?;
                        self.source_changed(id);
                    }
                    Act::User => {
                        source.user = clean(text, USER_MAX)?;
                        self.source_changed(id);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.changed(host)
    }
}
