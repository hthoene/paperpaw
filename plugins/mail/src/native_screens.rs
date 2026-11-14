//! The list screens of the interface-7 Mail: *Mail* (the folders of every
//! account) and a folder (its mails). Each opens with what the card keeps and
//! checks for mail when its first row is chosen, holding the one connection
//! the device has while it does.
use crate::declarations::SCREEN_TOKEN;
use crate::native_model::account::{Account, MAX, Offer, field, key, offer, pin_text};
use crate::native_model::browser::{self, Line, Mode, Target, Words};
use crate::native_model::fail::Fail;
use crate::native_model::mem::{Error as MailError, copy};
use crate::native_model::store::MAX_MESSAGES;
use crate::native_model::texts::{self, Say};
use crate::native_plugin::NativeMail;
use crate::native_values::HostValues;
use crate::native_words::HostWords;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered},
        formatting::Arg,
        settings::SettingValue,
        ui::{Instance, Open, Param, Question, RequestId, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{SERVICE_BUFFER, buffer},
    text,
};

/// The parameters of a folder's screen.
const ACCOUNT: &str = "account";
const FOLDER: &str = "folder";
/// The slot of the settings, and the group opened there.
const SETTINGS_SLOT: &str = "core.settings";

/// The token of the screen of the folders, and of a folder's.
pub(crate) const BOXES_TOKEN: u32 = SCREEN_TOKEN;
pub(crate) const LIST_TOKEN: u32 = SCREEN_TOKEN + 1;

/// One open list screen.
#[derive(Debug)]
pub(crate) struct View {
    mode: Mode,
    title: String,
    list: ItemList,
    /// What each item does when chosen (one for each item of the list).
    targets: Vec<Target>,
    /// What the screen says under its rows: where a check is, or how it went.
    status: String,
    /// A screen opened above ended the check: do it again when this one shows.
    interrupted: bool,
    /// The reader was asked to trust a server: check again when back.
    asked: bool,
}

/// The check that is going, and the accounts still to check.
#[derive(Debug)]
pub(crate) struct Check {
    pub(crate) owner: Instance,
    /// The account now checked first, then those after it.
    queue: Vec<u8>,
}

impl Check {
    /// A check of `queue` for the screen `owner`.
    pub(crate) fn new(owner: Instance, queue: Vec<u8>) -> Self {
        Self { owner, queue }
    }

    /// The account at the front is done (or passed by).
    fn pass(&mut self) {
        if !self.queue.is_empty() {
            self.queue.remove(0);
        }
    }
}

/// The question *Trust this server?* that is open.
#[derive(Debug)]
pub(crate) struct Asked {
    request: RequestId,
    owner: Instance,
    account: u8,
    smtp: bool,
    key: [u8; 32],
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}

/// Items for `lines`, and what each does.
fn items_of(lines: Vec<Line>) -> Result<(Vec<Item>, Vec<Target>), Error> {
    let mut items = Vec::new();
    let mut targets = Vec::new();
    items
        .try_reserve_exact(lines.len())
        .map_err(|_| Error::NoMemory)?;
    targets
        .try_reserve_exact(lines.len())
        .map_err(|_| Error::NoMemory)?;
    for line in lines {
        items.push(Item::noted(&line.title, &line.value, &line.note)?);
        targets.push(line.target);
    }
    Ok((items, targets))
}

fn fail_error(fail: Fail) -> Error {
    match fail {
        Fail::NoMemory => Error::NoMemory,
        _ => Error::Io,
    }
}

impl NativeMail {
    pub(crate) fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    pub(crate) fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    /// The accounts that are set up.
    pub(crate) fn accounts(&mut self, host: &Host<'_>) -> Result<Vec<Account>, Error> {
        self.services(host)?;
        let secrets = self.secrets.as_ref().ok_or(Error::State)?;
        let values = HostValues { host, secrets };
        let mut accounts = Vec::new();
        for number in 1..=MAX {
            if let Some(account) = Account::read(&values, number)? {
                accounts.try_reserve(1).map_err(|_| Error::NoMemory)?;
                accounts.push(account);
            }
        }
        Ok(accounts)
    }

    /// The lines of `mode`, and the title of its screen.
    fn lines_of(&mut self, host: &Host<'_>, mode: &Mode) -> Result<(String, Vec<Line>), Error> {
        let accounts = match mode {
            Mode::Boxes => self.accounts(host)?,
            _ => Vec::new(),
        };
        let storage = paperpaw_sdk::native::storage::DataStorage::new(host);
        let mut words = HostWords::new(host)?;
        match mode {
            Mode::Boxes => Ok((
                words.tr("mail.title")?,
                browser::boxes(&storage, &mut words, &accounts).map_err(fail_error)?,
            )),
            Mode::Folder { account, raw } => Ok((
                browser::folder_title(&storage, &mut words, *account, raw).map_err(fail_error)?,
                browser::folder(&storage, &mut words, *account, raw).map_err(fail_error)?,
            )),
        }
    }

    /// Reads the lines of the screen `instance` again.
    fn reload(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let view = self.screens.get(instance).ok_or(Error::State)?;
        let mode = view.mode.clone();
        let (title, lines) = self.lines_of(host, &mode)?;
        let (items, targets) = items_of(lines)?;
        let view = self.screens.get(instance).ok_or(Error::State)?;
        view.title = title;
        view.targets = targets;
        // Rows are replaced; the focus stays where it was, or on the last row.
        let focus = view.list.focus();
        view.list.replace(items);
        let last = view.list.items().len().saturating_sub(1);
        view.list.set_focus(focus.min(last));
        Ok(())
    }

    /// A list screen opened: the account and the folder come in the handover.
    pub(crate) fn screen_opened(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let mode = if token == LIST_TOKEN {
            self.mode_from(host, handover)?
        } else {
            Mode::Boxes
        };
        // A card that cannot be read shows an empty list that says so, not a
        // screen that cannot be drawn.
        let (title, lines, status) = match self.lines_of(host, &mode) {
            Ok((title, lines)) => (title, lines, String::new()),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                let mut words = HostWords::new(host)?;
                (
                    words.tr("mail.title")?,
                    Vec::new(),
                    words.tr("mail.fail.storage")?,
                )
            }
        };
        let (items, targets) = items_of(lines)?;
        let view = View {
            mode,
            title,
            list: ItemList::new(items),
            targets,
            status,
            interrupted: false,
            asked: false,
        };
        self.screens.open(instance, view)
    }

    fn mode_from(
        &self,
        host: &Host<'_>,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<Mode, Error> {
        let handover = handover.ok_or(Error::Invalid)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let (mut account, mut raw) = (None, None);
        for param in self.ui()?.params(host, handover, &mut input, &mut out)? {
            let param = param?;
            match param.key() {
                ACCOUNT => account = param.value().parse::<u8>().ok(),
                FOLDER => raw = Some(copy(param.value())?),
                _ => {}
            }
        }
        match (account, raw) {
            (Some(account), Some(raw)) if (1..=MAX).contains(&account) => {
                Ok(Mode::Folder { account, raw })
            }
            _ => Err(Error::Invalid),
        }
    }

    /// A list screen came on top: it reads the card again, and checks again if
    /// a screen above had ended its check or the reader trusted a server.
    pub(crate) fn screen_shown(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: Instance,
    ) -> Result<(), Error> {
        self.screens.shown(token, instance)?;
        self.reload(host, instance)?;
        let view = self.screens.get(instance).ok_or(Error::State)?;
        let again = core::mem::take(&mut view.interrupted);
        let asked = core::mem::take(&mut view.asked);
        let pinned = asked && self.pinned(host, instance)?;
        if again || pinned {
            self.job.run = crate::native_model::job::Run::Idle;
            self.check(host, instance)?;
        }
        redraw(host)
    }

    /// Whether the reader trusted the server that was asked about: its pin is
    /// set now.
    fn pinned(&mut self, host: &Host<'_>, instance: Instance) -> Result<bool, Error> {
        let Some(number) = self.pending.take().filter(|n| n.1 == instance).map(|n| n.0) else {
            return Ok(false);
        };
        Ok(self
            .accounts(host)?
            .iter()
            .any(|a| a.number == number && (a.imap.pin.is_some() || a.smtp.pin.is_some())))
    }

    /// A list screen closed: its connection goes with it.
    pub(crate) fn screen_closed(&mut self, host: &Host<'_>, instance: Instance) {
        if self.check.as_ref().is_some_and(|c| c.owner == instance) {
            self.stop(host);
        }
        self.screens.close(instance);
    }

    /// Lets go of the connection and the accounts still to check.
    pub(crate) fn stop(&mut self, host: &Host<'_>) {
        self.check = None;
        self.cancel(host);
    }

    /// Draws the list on top.
    pub(crate) fn draw_list(
        &mut self,
        host: &Host<'_>,
        token: u32,
        mut frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        let View {
            title,
            list,
            status,
            ..
        } = view;
        let footer = if status.is_empty() {
            Footer::None
        } else {
            Footer::Note(status)
        };
        list.draw_with(host, &mut frame, title, true, footer)
    }

    /// An input for the list on top.
    pub(crate) fn input_list(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        let Some(instance) = self.screens.top_instance(token) else {
            return Ok(Handled::No);
        };
        let view = self.screens.get(instance).ok_or(Error::State)?;
        let step = view.list.input(host, input)?;
        let ItemStep::Chosen(index) = step else {
            return Ok(ItemList::handled(step));
        };
        let Some(target) = view.targets.get(index).cloned() else {
            return Ok(Handled::Yes);
        };
        self.choose(host, instance, &target)?;
        Ok(Handled::Yes)
    }

    fn choose(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        target: &Target,
    ) -> Result<(), Error> {
        if matches!(target, Target::Check) {
            return self.check(host, instance);
        }
        // A screen that opens above is the one the device can hold the
        // connection for.
        if self.check.as_ref().is_some_and(|c| c.owner == instance) {
            self.stop(host);
            if let Some(view) = self.screens.get(instance) {
                view.interrupted = true;
            }
        }
        match target {
            Target::Folder { account, raw } => self.open_folder(host, *account, raw),
            Target::Settings => self.open_settings(host),
            Target::Message { uid } => {
                let Some(Mode::Folder { account, raw }) =
                    self.screens.get(instance).map(|v| v.mode.clone())
                else {
                    return Ok(());
                };
                self.open_reader(host, account, &raw, *uid)
            }
            Target::Write => {
                let account = match self.screens.get(instance).map(|v| v.mode.clone()) {
                    Some(Mode::Folder { account, .. }) => account,
                    _ => 0,
                };
                self.open_compose(host, account, None)
            }
            _ => Ok(()),
        }
    }

    fn open_reader(&self, host: &Host<'_>, account: u8, raw: &str, uid: u32) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let (number, uid) = (
            text::format(format_args!("{account}"))?,
            text::format(format_args!("{uid}"))?,
        );
        let params = [
            Param::new(ACCOUNT, &number)?,
            Param::new(FOLDER, raw)?,
            Param::new("uid", &uid)?,
        ];
        let handover = self.ui()?.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::screen("mail.read")?.handover(handover))
    }

    /// Opens the compose window for `account` (0: the first), or as a reply
    /// to the mail `uid` of `folder` (to all where the last is true).
    pub(crate) fn open_compose(
        &self,
        host: &Host<'_>,
        account: u8,
        reply: Option<(&str, u32, bool)>,
    ) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let number = text::format(format_args!("{account}"))?;
        let mut params: Vec<Param<'_>> = Vec::new();
        params.try_reserve(4).map_err(|_| Error::NoMemory)?;
        params.push(Param::new(ACCOUNT, &number)?);
        let uid_text;
        if let Some((folder, uid, all)) = reply {
            uid_text = text::format(format_args!("{uid}"))?;
            params.push(Param::new(FOLDER, folder)?);
            params.push(Param::new("uid", &uid_text)?);
            params.push(Param::new("reply", if all { "all" } else { "one" })?);
        }
        let handover = self.ui()?.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .open(Open::screen("mail.compose")?.handover(handover))
    }

    pub(crate) fn open_folder(&self, host: &Host<'_>, account: u8, raw: &str) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let number = text::format(format_args!("{account}"))?;
        let params = [Param::new(ACCOUNT, &number)?, Param::new(FOLDER, raw)?];
        let handover = self.ui()?.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::screen("mail.list")?.handover(handover))
    }

    fn open_settings(&self, host: &Host<'_>) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let params = [Param::new(
            paperpaw_sdk::builtin::params::ID,
            crate::declarations::GROUP,
        )?];
        let handover = self.ui()?.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::slot(SETTINGS_SLOT)?.handover(handover))
    }

    /// Sets what the screen says under its rows, and draws it.
    pub(crate) fn say_in(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        say: Say<'_>,
    ) -> Result<(), Error> {
        let mut words = HostWords::new(host)?;
        let status = words.say(say)?;
        if let Some(view) = self.screens.get(instance) {
            view.status = status;
        }
        redraw(host)
    }

    /// Starts checking the account at the front of the queue of the check.
    fn next_account(&mut self, host: &Host<'_>) -> Result<(), Error> {
        loop {
            let Some(check) = &self.check else {
                return Ok(());
            };
            let owner = check.owner;
            let Some(&number) = check.queue.first() else {
                self.check = None;
                return Ok(());
            };
            let mode = self.screens.get(owner).ok_or(Error::State)?.mode.clone();
            let fetch = self.fetch_count(host)?;
            let now = paperpaw_sdk::native::snapshot::capture_snapshot(host)?
                .unix_seconds()
                .unwrap_or(0);
            let tasks = browser::tasks(&mode, number, fetch, now).map_err(fail_error)?;
            match self.start_imap(host, number, tasks) {
                Ok(()) => {
                    if let Some(view) = self.screens.get(owner) {
                        view.status = String::new();
                    }
                    return redraw(host);
                }
                // An account that is not set up (any more) is passed by.
                Err(MailError::NotFound) => {
                    if let Some(check) = &mut self.check {
                        check.pass();
                    }
                }
                Err(error) => {
                    self.check = None;
                    return self.say_start_failure(host, owner, error);
                }
            }
        }
    }

    fn say_start_failure(
        &mut self,
        host: &Host<'_>,
        owner: Instance,
        error: Error,
    ) -> Result<(), Error> {
        let key = match error {
            Error::Unsupported => "core.secrets-unsupported",
            Error::NoMemory => "mail.fail.memory",
            Error::Busy => "mail.fail.busy",
            _ => "mail.fail.lost",
        };
        self.say_in(host, owner, Say::Key(key))
    }

    /// How many mails a check fetches of a folder (the setting, 1 to 50).
    fn fetch_count(&mut self, host: &Host<'_>) -> Result<u32, Error> {
        use crate::native_model::account::Values;
        self.services(host)?;
        let secrets = self.secrets.as_ref().ok_or(Error::State)?;
        let fetch = HostValues { host, secrets }.int("mail.fetch")?;
        let most = i64::from(MAX_MESSAGES);
        Ok(u32::try_from(fetch.clamp(1, most)).unwrap_or(MAX_MESSAGES))
    }

    /// Checks for mail from the screen `instance`: every account for *Mail*,
    /// the account of a folder for a folder.
    pub(crate) fn check(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        if self.job.working() {
            return Ok(());
        }
        let view = self.screens.get(instance).ok_or(Error::State)?;
        let mut queue = Vec::new();
        match &view.mode {
            Mode::Folder { account, .. } => {
                queue.try_reserve(1).map_err(|_| Error::NoMemory)?;
                queue.push(*account);
            }
            _ => {
                for account in self.accounts(host)? {
                    queue.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    queue.push(account.number);
                }
            }
        }
        self.check = Some(Check {
            owner: instance,
            queue,
        });
        self.next_account(host)
    }

    /// The connection moved on: what it ended in.
    pub(crate) fn progressed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        use crate::native_model::job::Run;
        let Some(owner) = self.check.as_ref().map(|c| c.owner) else {
            return Ok(());
        };
        if self.readers.get(owner).is_some() {
            return self.reader_progressed(host, owner);
        }
        if self.writers.get(owner).is_some() {
            return self.writer_progressed(host, owner);
        }
        match self.job.run.clone() {
            Run::Working(stage) => self.say_in(host, owner, Say::Key(texts::stage(stage))),
            Run::Done => {
                if let Some(check) = &mut self.check {
                    check.pass();
                }
                self.reload(host, owner)?;
                self.say_in(host, owner, Say::Key("mail.up-to-date"))?;
                self.job.run = Run::Idle;
                self.next_account(host)
            }
            Run::Failed(failure) => {
                self.check = None;
                self.say_in(host, owner, texts::failure(&failure))
            }
            Run::Untrusted { smtp, key, why, .. } => {
                let number = self
                    .check
                    .as_ref()
                    .and_then(|c| c.queue.first().copied())
                    .ok_or(Error::State)?;
                self.check = None;
                self.untrusted(host, owner, number, smtp, key, why)
            }
            _ => Ok(()),
        }
    }

    /// What a reader or the compose window says of a server that is not
    /// trusted: where the list asks, to open the list; else why it is not
    /// offered ([`texts::pin_message`]).
    pub(crate) fn pin_say(
        &mut self,
        host: &Host<'_>,
        number: u8,
        smtp: bool,
    ) -> Result<Say<'static>, Error> {
        self.services(host)?;
        let secrets = self.secrets.as_ref().ok_or(Error::State)?;
        let offered = offer(&HostValues { host, secrets }, number, smtp)?;
        Ok(Say::Key(texts::pin_message(offered)))
    }

    /// A server whose certificate no authority vouches for: the reader is
    /// asked once whether to trust its key (a provider's server, or one whose
    /// trusted key changed, is not offered: said so, not connected).
    fn untrusted(
        &mut self,
        host: &Host<'_>,
        owner: Instance,
        number: u8,
        smtp: bool,
        key: [u8; 32],
        why: paperpaw_sdk::native::sockets::Untrusted,
    ) -> Result<(), Error> {
        self.services(host)?;
        let secrets = self.secrets.as_ref().ok_or(Error::State)?;
        let offered = offer(&HostValues { host, secrets }, number, smtp)?;
        if offered != Offer::Ask {
            return self.say_in(host, owner, Say::Key(texts::pin_message(offered)));
        }
        let mut messages = Messages::new()?;
        let hex = pin_text(&key)?;
        let shown = browser::grouped(&hex)?;
        let why = messages.copy(host, texts::distrust(why))?;
        let title = messages.copy(host, "mail.pin.title")?;
        let body = messages.format(
            host,
            "mail.pin.text",
            &[
                Arg::Text {
                    name: "why",
                    value: &why,
                },
                Arg::Text {
                    name: "hash",
                    value: &shown,
                },
            ],
        )?;
        let labels = text::concat(&[
            &messages.copy(host, "mail.pin.yes")?,
            "\n",
            &messages.copy(host, "mail.pin.no")?,
        ])?;
        let question = Question::confirm(&title, &body)?.confirm_labels(&labels)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self.ui()?.ask(host, question, &mut input, &mut out)?;
        self.asked = Some(Asked {
            request,
            owner,
            account: number,
            smtp,
            key,
        });
        if let Some(view) = self.screens.get(owner) {
            view.asked = true;
        }
        Ok(())
    }

    /// The answer to *Trust this server?*: the reader's yes keeps the key.
    pub(crate) fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if self.writer_answered(host, answer)? {
            return Ok(());
        }
        let Some(asked) = &self.asked else {
            return Ok(());
        };
        if asked.request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some(asked) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered || answer.choice() != 1 {
            return Ok(());
        }
        self.pending = Some((asked.account, asked.owner));
        let id = key(
            asked.account,
            if asked.smtp {
                field::SMTP_PIN
            } else {
                field::IMAP_PIN
            },
        )?;
        let pin = pin_text(&asked.key)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).set_setting(&id, SettingValue::Text(&pin))
    }
}
