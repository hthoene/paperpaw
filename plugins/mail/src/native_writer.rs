//! The compose window (`mail.compose`): the sender, the recipients, the
//! subject and the text, a row each, typed on the keyboard; *Send* below. What
//! is written is kept on the card as a draft until it is sent.
use crate::declarations::SCREEN_TOKEN;
use crate::native_model::account::Account;
use crate::native_model::browser::Words;
use crate::native_model::compose::{self, Draft, MAX_BODY, Refused};
use crate::native_model::fail::Fail;
use crate::native_model::job::Run;
use crate::native_model::mem::copy;
use crate::native_model::session::Outgoing;
use crate::native_model::store;
use crate::native_model::texts::{self, Say};
use crate::native_model::writer::{self, Part};
use crate::native_plugin::NativeMail;
use crate::native_values::HostValues;
use crate::native_words::HostWords;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled, Storage,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered},
        formatting::Arg,
        snapshot,
        storage::DataStorage,
        ui::{HandoverId, Instance, Question, RequestId, TextMode},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{SERVICE_BUFFER, buffer},
    uitext,
};

/// The token of the compose window.
pub(crate) const COMPOSE_TOKEN: u32 = SCREEN_TOKEN + 3;
/// The longest address list or subject typed (the text has [`MAX_BODY`]).
const LINE_LIMIT: usize = 2048;

/// One open compose window.
#[derive(Debug)]
pub(crate) struct Writing {
    accounts: Vec<Account>,
    /// Which of `accounts` sends.
    from: usize,
    draft: Draft,
    parts: Vec<Part>,
    list: ItemList,
    status: String,
    sent: bool,
    /// The question that is open, and for which row.
    asked: Option<(RequestId, Part)>,
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}

fn fail_error(fail: Fail) -> Error {
    match fail {
        Fail::NoMemory => Error::NoMemory,
        _ => Error::Io,
    }
}

impl Writing {
    fn account(&self) -> Option<&Account> {
        self.accounts.get(self.from)
    }

    /// The rows, made again from the draft.
    fn rebuild(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut words = HostWords::new(host)?;
        let sender = match self.account() {
            Some(account) => copy(&account.address)?,
            None => String::new(),
        };
        self.parts = writer::parts(self.accounts.len())?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(self.parts.len())
            .map_err(|_| Error::NoMemory)?;
        for part in &self.parts {
            let title = words.tr(part.title())?;
            let value = if *part == Part::Send {
                String::from("\u{203a}")
            } else {
                writer::value(*part, &self.draft, &sender, &mut words)?
            };
            items.push(Item::row(&title, &value)?);
        }
        let focus = self.list.focus();
        self.list.replace(if self.accounts.is_empty() {
            Vec::new()
        } else {
            items
        });
        self.list
            .set_focus(focus.min(self.parts.len().saturating_sub(1)));
        Ok(())
    }

    /// Keeps the draft on the card, as it is now.
    fn keep(&self, host: &Host<'_>) -> Result<(), Error> {
        let Some(account) = self.account() else {
            return Ok(());
        };
        let file = store::draft_file(account.number).map_err(fail_error)?;
        let text = self.draft.to_text()?;
        store::write_atomic(&DataStorage::new(host), &file, text.as_bytes()).map_err(fail_error)
    }
}

impl NativeMail {
    /// The compose window opened: a reply, or the draft kept, or a mail with
    /// the signature only. The account, and for a reply the folder, the mail
    /// and how, come in the handover.
    pub(crate) fn writer_opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let (mut account, mut folder, mut uid, mut all) = (0u8, None, None, false);
        if let Some(handover) = handover {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            for param in self.ui()?.params(host, handover, &mut input, &mut out)? {
                let param = param?;
                match param.key() {
                    "account" => account = param.value().parse().unwrap_or(0),
                    "folder" => folder = Some(copy(param.value())?),
                    "uid" => uid = param.value().parse::<u32>().ok(),
                    "reply" => all = param.value() == "all",
                    _ => {}
                }
            }
        }
        let accounts = self.accounts(host)?;
        let from = accounts
            .iter()
            .position(|a| a.number == account)
            .unwrap_or(0);
        let mut writing = Writing {
            accounts,
            from,
            draft: Draft::default(),
            parts: Vec::new(),
            list: ItemList::new(Vec::new()),
            status: String::new(),
            sent: false,
            asked: None,
        };
        if let Some(account) = writing.account().cloned() {
            writing.draft = self.starting_draft(host, &account, folder.as_deref().zip(uid), all)?;
        }
        writing.rebuild(host)?;
        self.writers.open(instance, writing)
    }

    /// A reply to the mail named, else the draft kept, else a mail with the
    /// signature only; from `account`.
    fn starting_draft(
        &mut self,
        host: &Host<'_>,
        account: &Account,
        reply: Option<(&str, u32)>,
        all: bool,
    ) -> Result<Draft, Error> {
        let storage = DataStorage::new(host);
        let letter = match reply {
            Some((folder, uid)) => {
                let file = store::text_file(account.number, folder, uid).map_err(fail_error)?;
                store::read_letter(&storage, &file).map_err(fail_error)?
            }
            None => None,
        };
        let mut draft = if let Some(letter) = letter {
            let who = writer::header_name(&letter.from)?;
            let intro = Messages::new()?.format(
                host,
                "mail.reply.intro",
                &[Arg::Text {
                    name: "who",
                    value: &who,
                }],
            )?;
            compose::reply_to(&letter, &account.address, all, &intro)?
        } else if let Some(text) = store::read_kept(
            &storage,
            &store::draft_file(account.number).map_err(fail_error)?,
            MAX_BODY + 4096,
        )
        .map_err(fail_error)?
        {
            Draft::from_text(&text)?
        } else {
            let signature = self.signature(host)?;
            Draft {
                body: writer::fresh_body(&signature)?,
                ..Draft::default()
            }
        };
        draft.from = copy(&account.address)?;
        draft.from_name = copy(&account.name)?;
        Ok(draft)
    }

    /// The signature set in the settings.
    fn signature(&mut self, host: &Host<'_>) -> Result<String, Error> {
        use crate::native_model::account::Values;
        self.services(host)?;
        let secrets = self.secrets.as_ref().ok_or(Error::State)?;
        HostValues { host, secrets }.text("mail.signature")
    }

    /// A compose window came on top.
    pub(crate) fn writer_shown(&mut self, instance: Instance) -> Result<(), Error> {
        self.writers.shown(COMPOSE_TOKEN, instance)
    }

    /// A compose window closed: its connection goes with it.
    pub(crate) fn writer_closed(&mut self, host: &Host<'_>, instance: Instance) {
        if self.check.as_ref().is_some_and(|c| c.owner == instance) {
            self.stop(host);
        }
        self.writers.close(instance);
    }

    /// Draws the compose window on top.
    pub(crate) fn draw_writer(
        &mut self,
        host: &Host<'_>,
        mut frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        let instance = self
            .writers
            .top_instance(COMPOSE_TOKEN)
            .ok_or(Error::State)?;
        let working =
            self.job.working() && self.check.as_ref().is_some_and(|c| c.owner == instance);
        let stage = match &self.job.run {
            Run::Working(stage) if working => Some(*stage),
            _ => None,
        };
        let writing = self.writers.get(instance).ok_or(Error::State)?;
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "mail.write")?;
        let status = match stage {
            Some(stage) => messages.copy(host, texts::stage(stage))?,
            None => copy(&writing.status)?,
        };
        let footer = if writing.accounts.is_empty() {
            let text = messages.copy(host, "mail.set-up.help")?;
            let heading = messages.copy(host, "mail.set-up")?;
            return writing.list.draw_with(
                host,
                &mut frame,
                &title,
                true,
                Footer::Empty {
                    title: &heading,
                    text: &text,
                },
            );
        } else if status.is_empty() {
            Footer::None
        } else {
            Footer::Note(&status)
        };
        writing
            .list
            .draw_with(host, &mut frame, &title, true, footer)
    }

    /// An input for the compose window on top.
    pub(crate) fn input_writer(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Some(instance) = self.writers.top_instance(COMPOSE_TOKEN) else {
            return Ok(Handled::No);
        };
        let writing = self.writers.get(instance).ok_or(Error::State)?;
        let step = writing.list.input(host, input)?;
        let ItemStep::Chosen(index) = step else {
            return Ok(ItemList::handled(step));
        };
        if writing.sent {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).back()?;
            return Ok(Handled::Yes);
        }
        let Some(part) = writing.parts.get(index).copied() else {
            return Ok(Handled::Yes);
        };
        self.choose_part(host, instance, part)?;
        Ok(Handled::Yes)
    }

    fn choose_part(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        part: Part,
    ) -> Result<(), Error> {
        match part {
            Part::From => {
                let writing = self.writers.get(instance).ok_or(Error::State)?;
                writing.from = (writing.from + 1) % writing.accounts.len().max(1);
                if let Some((address, name)) =
                    writing.account().map(|a| (copy(&a.address), copy(&a.name)))
                {
                    writing.draft.from = address?;
                    writing.draft.from_name = name?;
                }
                writing.rebuild(host)?;
                redraw(host)
            }
            Part::Send => self.send(host, instance),
            part if part.typed() => self.ask_part(host, instance, part),
            _ => Ok(()),
        }
    }

    /// Asks the reader to type the row `part`, starting from what is there.
    fn ask_part(&mut self, host: &Host<'_>, instance: Instance, part: Part) -> Result<(), Error> {
        let writing = self.writers.get(instance).ok_or(Error::State)?;
        let current = copy(writer::current(&writing.draft, part))?;
        let title = Messages::new()?.copy(host, part.title())?;
        let mut question = Question::text(&title, "")?;
        if matches!(part, Part::To | Part::Cc) {
            question = question.mode(TextMode::Email)?;
        }
        // A message has lines: the keyboard has a key for the break.
        if part == Part::Body {
            question = question.multiline()?;
        }
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self.ui()?.ask(host, question, &mut input, &mut out)?;
        uitext::write_all(self.ui()?, host, request.text(), &current)?;
        if let Some(writing) = self.writers.get(instance) {
            writing.asked = Some((request, part));
        }
        Ok(())
    }

    /// A typed answer for a compose window, if it is one: whether it was.
    pub(crate) fn writer_answered(
        &mut self,
        host: &Host<'_>,
        answer: Answered,
    ) -> Result<bool, Error> {
        let Some(instance) = self.writers.top_instance(COMPOSE_TOKEN) else {
            return Ok(false);
        };
        let Some(writing) = self.writers.get(instance) else {
            return Ok(false);
        };
        let Some((request, part)) = writing.asked else {
            return Ok(false);
        };
        if request != answer.request() {
            return Ok(false);
        }
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(true);
        }
        writing.asked = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(true);
        }
        let limit = if part == Part::Body {
            MAX_BODY * 2
        } else {
            LINE_LIMIT
        };
        let text = uitext::read_all(self.ui()?, host, request.text(), limit)?;
        let writing = self.writers.get(instance).ok_or(Error::State)?;
        writer::typed(&mut writing.draft, part, &text)?;
        writing.keep(host)?;
        writing.status = String::new();
        writing.rebuild(host)?;
        redraw(host)?;
        Ok(true)
    }

    /// Sends the draft: checked, made into a message with its id and the date,
    /// and handed to the SMTP server of the account.
    fn send(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        if self.job.working() {
            return Ok(());
        }
        let writing = self.writers.get(instance).ok_or(Error::State)?;
        let Some(account) = writing.account().map(|a| a.number) else {
            return Ok(());
        };
        let to = match writing.draft.check() {
            Ok(to) => to,
            Err(Refused::Problem(problem)) => {
                return self.say_writer(host, instance, Say::Key(writer::problem_key(problem)));
            }
            Err(_) => return Err(Error::NoMemory),
        };
        let now = snapshot::capture_snapshot(host)?
            .unix_seconds()
            .unwrap_or(0);
        let random = self.random(host);
        let id = writer::message_id(random.as_ref().map(|r| &r[..]), now)?;
        let writing = self.writers.get(instance).ok_or(Error::State)?;
        let message = writing.draft.message(&compose::date_header(now)?, &id)?;
        let outgoing = Outgoing::new(
            copy(&writing.draft.from)?,
            to,
            crate::native_model::smtp::data(&message)?,
        );
        writing.status = String::new();
        let mut queue = Vec::new();
        queue.try_reserve(1).map_err(|_| Error::NoMemory)?;
        queue.push(account);
        self.check = Some(crate::native_screens::Check::new(instance, queue));
        match self.start_smtp(host, account, outgoing) {
            Ok(()) => redraw(host),
            Err(error) => {
                self.check = None;
                let key = match error {
                    Error::Unsupported => "core.secrets-unsupported",
                    Error::NoMemory => "mail.fail.memory",
                    Error::NotFound => "mail.no-account",
                    Error::Busy => "mail.fail.busy",
                    _ => "mail.fail.lost",
                };
                self.say_writer(host, instance, Say::Key(key))
            }
        }
    }

    /// Eight random bytes from the device, if it gives them.
    fn random(&mut self, host: &Host<'_>) -> Option<[u8; 8]> {
        self.services(host).ok()?;
        let secrets = self.secrets.as_ref()?;
        let (mut input, mut out) = ([0u8; 64], [0u8; 128]);
        let bytes = secrets.random(host, 8, &mut input, &mut out).ok()?;
        let mut found = [0u8; 8];
        found.copy_from_slice(bytes.bytes().get(..8)?);
        paperpaw_sdk::native::secrets::wipe(&mut out);
        Some(found)
    }

    fn say_writer(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        say: Say<'_>,
    ) -> Result<(), Error> {
        let status = HostWords::new(host)?.say(say)?;
        if let Some(writing) = self.writers.get(instance) {
            writing.status = status;
        }
        redraw(host)
    }

    /// The connection of a compose window moved on: what it ended in.
    pub(crate) fn writer_progressed(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
    ) -> Result<(), Error> {
        match self.job.run.clone() {
            Run::Working(stage) => self.say_writer(host, instance, Say::Key(texts::stage(stage))),
            Run::Done => {
                self.check = None;
                self.job.run = Run::Idle;
                if let Some(writing) = self.writers.get(instance) {
                    writing.sent = true;
                    if let Some(a) = writing.account() {
                        let file = store::draft_file(a.number).map_err(fail_error)?;
                        // A draft that cannot be removed is sent twice at worst.
                        DataStorage::new(host).remove(&file).ok();
                    }
                }
                self.say_writer(host, instance, Say::Key("mail.sent"))
            }
            Run::Failed(failure) => {
                self.check = None;
                self.say_writer(host, instance, texts::failure(&failure))
            }
            Run::Untrusted { smtp, .. } => {
                self.check = None;
                let number = self
                    .writers
                    .get(instance)
                    .and_then(|w| w.account().map(|a| a.number));
                let say = match number {
                    Some(number) => self.pin_say(host, number, smtp)?,
                    None => return Ok(()),
                };
                self.say_writer(host, instance, say)
            }
            _ => Ok(()),
        }
    }
}
