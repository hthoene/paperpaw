//! What the plugin does for the screens that needs the host: ask the reader and
//! read the answer, say a text once, open a screen with parameters, read the
//! parameters a screen was opened with. Each call holds its buffers for its own
//! length only (#248).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    events::{AnswerOutcome, Answered},
    ui::{HandoverId, Notify, Open, Param, Question, RequestId, TextMode, Ui},
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer},
    text::copy,
};

use crate::native_screens::Ask;

/// The longest answer typed that is read back (a search or a page number).
const MOST_TYPED: usize = 256;
/// The most parameters a screen is opened with.
const MOST_PARAMS: usize = 8;
/// The most a query takes (`max_input` of the ui service's ops).
const REQUEST_BYTES: usize = 512;

/// Asks the reader `ask`: the dialog opens after the callback returns, its end
/// comes as an `Answered` event with the request returned here.
///
/// # Errors
/// `NoMemory`, or what the host refuses.
pub fn ask(host: &Host<'_>, ui: &Ui, ask: &Ask) -> Result<RequestId, Error> {
    let mut input = [0; REQUEST_BYTES];
    let mut out = buffer(SERVICE_BUFFER)?;
    match ask {
        Ask::Text { title, number } => {
            let mut question = Question::text(title, "")?;
            if *number {
                question = question.mode(TextMode::Number)?;
            }
            ui.ask(host, question, &mut input, &mut out)
        }
        Ask::Choice { title, options } => {
            let mut lines = String::new();
            for (n, option) in options.iter().enumerate() {
                lines
                    .try_reserve(option.len() + 1)
                    .map_err(|_| Error::NoMemory)?;
                if n > 0 {
                    lines.push('\n');
                }
                lines.push_str(option);
            }
            ui.ask(
                host,
                Question::choice(title, "", &lines)?,
                &mut input,
                &mut out,
            )
        }
    }
}

/// What a closed dialog came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Still being typed, or not for the dialog asked: nothing yet.
    Wait,
    /// The reader left the dialog without answering.
    Cancelled,
    /// A choice, counted from 0.
    Choice(usize),
    /// A text typed.
    Text(String),
}

/// Reads an answer of the question `asked`.
///
/// # Errors
/// `NoMemory`, or what the host refuses when the text is read back.
pub fn reply(
    host: &Host<'_>,
    ui: &Ui,
    asked: RequestId,
    text: bool,
    answer: &Answered,
) -> Result<Reply, Error> {
    if answer.request() != asked || answer.outcome() == AnswerOutcome::Typing {
        return Ok(Reply::Wait);
    }
    if answer.outcome() != AnswerOutcome::Answered {
        return Ok(Reply::Cancelled);
    }
    if !text {
        return Ok(Reply::Choice(answer.choice() as usize));
    }
    let mut input = [0; REQUEST_BYTES];
    let mut out = buffer(SERVICE_BUFFER)?;
    let part = ui.text_read(host, answer.request().text(), 0, &mut input, &mut out)?;
    if part.data().len() > MOST_TYPED {
        return Err(Error::NoSpace);
    }
    Ok(Reply::Text(copy(part.data())?))
}

/// Says `text` once.
///
/// # Errors
/// `NoMemory`, or what the host refuses.
pub fn notify(host: &Host<'_>, text: &str) -> Result<(), Error> {
    let mut scratch = [0; 1024];
    CommandSender::new(host, &mut scratch).notify(Notify::new(text)?)
}

/// Opens the screen `screen` (its id, `store.<name>`) with `params`.
///
/// # Errors
/// `NoMemory`, or what the host refuses.
pub fn open(
    host: &Host<'_>,
    ui: &Ui,
    screen: &str,
    params: &[(&'static str, String)],
) -> Result<(), Error> {
    let mut scratch = [0; REQUEST_BYTES];
    if params.is_empty() {
        return CommandSender::new(host, &mut scratch).open(Open::screen(screen)?);
    }
    let mut made: Vec<Param<'_>> = Vec::new();
    made.try_reserve_exact(params.len().min(MOST_PARAMS))
        .map_err(|_| Error::NoMemory)?;
    for (name, value) in params.iter().take(MOST_PARAMS) {
        made.push(Param::new(name, value)?);
    }
    let mut out = buffer(SERVICE_BUFFER)?;
    let handover = ui.handover(host, &made, &mut scratch, &mut out)?;
    let mut commands = [0; REQUEST_BYTES];
    CommandSender::new(host, &mut commands).open(Open::screen(screen)?.handover(handover))
}

/// The parameters a screen was opened with, as (name, value).
///
/// # Errors
/// `NoMemory`, or what the host refuses.
pub fn params(
    host: &Host<'_>,
    ui: &Ui,
    handover: Option<HandoverId>,
) -> Result<Vec<(String, String)>, Error> {
    let mut list = Vec::new();
    let Some(handover) = handover else {
        return Ok(list);
    };
    let mut input = [0; REQUEST_BYTES];
    let mut out = buffer(SERVICE_BUFFER)?;
    for param in ui.params(host, handover, &mut input, &mut out)? {
        let param = param?;
        list.try_reserve(1).map_err(|_| Error::NoMemory)?;
        list.push((copy(param.key())?, copy(param.value())?));
    }
    Ok(list)
}

/// A copy of `text`.
///
/// # Errors
/// `NoMemory`.
pub fn own(text: &str) -> Result<String, Error> {
    copy(text)
}

/// Opens the text file at `path` (relative to the card) in the reader.
///
/// # Errors
/// `NoMemory`, or what the host refuses.
pub fn open_reader(host: &Host<'_>, ui: &Ui, path: &str) -> Result<(), Error> {
    let params = [Param::new("path", path)?];
    let mut scratch = [0; REQUEST_BYTES];
    let mut out = buffer(SERVICE_BUFFER)?;
    let handover = ui.handover(host, &params, &mut scratch, &mut out)?;
    let mut commands = [0; REQUEST_BYTES];
    CommandSender::new(host, &mut commands).open(Open::slot("core.reader")?.handover(handover))
}
