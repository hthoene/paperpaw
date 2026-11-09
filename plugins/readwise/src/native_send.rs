//! Sending the highlights of the notes files that wait, one file after the
//! other, a request each: the same rules as interface 6, over a [`Wire`] (the
//! plugin's data folder, the card with the notes, the network, the token) so
//! that the decisions are tested without a host.
//!
//! An export puts its notes file in `waiting.txt` (a path per line, see
//! `docs/data-files.md`) before anything is sent; it leaves only once Readwise
//! took its highlights, or when it has none (or was deleted). What fails stays
//! waiting and goes with the next sending: the next export, or when the
//! network comes back. A token Readwise refuses is let go, so nothing is tried
//! by itself until another one is kept, or one is entered with the next export.
//!
//! Each request's body is written to a file first, a highlight at a time as the
//! notes are read in pieces, and sent from there. Readwise's answer is not
//! kept: the request asks for no more of it than a few bytes.
use alloc::{string::String, vec::Vec};
use core::ops::ControlFlow;

use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_ui_kit::{files::Volume, mem, text};

use crate::native_readwise::Parser;

/// The notes files waiting, in the data folder.
pub const WAITING: &str = "waiting.txt";
/// The body of the request being sent, in the data folder.
pub const BODY: &str = "body.json";
/// Where the list is written anew before it replaces the old.
const WAITING_NEW: &str = "waiting.txt.new";
/// The plugin's own number of the request, in its `http_done` event.
pub const REQUEST_TOKEN: u32 = 1;
/// Notes are read this much at a time.
const PIECE: usize = 512;
/// A line is cut here (as interface 6 cut it).
const MAX_LINE: usize = 4096;
/// The longest token that is kept and sent; Readwise's are far shorter.
pub const MAX_TOKEN: usize = 512;
/// A write to a file is at most this long (`storage::MAX_DATA`).
const MAX_WRITE: usize = paperpaw_sdk::native::storage::MAX_DATA;

/// A text that wipes itself when it is dropped: the token.
pub struct Secret(String);

impl Secret {
    /// A copy of `text` that wipes itself when it is dropped.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(text: &str) -> Result<Self, Error> {
        text::copy(text).map(Self)
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl core::fmt::Debug for Secret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Secret(..)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        mem::wipe_text(core::mem::take(&mut self.0));
    }
}

/// The `Authorization` header's value for `token`, wiped when dropped.
///
/// # Errors
/// `NoMemory`.
pub fn authorization(token: &Secret) -> Result<Secret, Error> {
    let line = text::format(format_args!("Token {}", token.as_str()))?;
    let secret = Secret::new(&line);
    mem::wipe_text(line);
    secret
}

/// What the sending needs of the world: the folders, the network, the token.
/// The host's in the plugin; a stand-in in tests.
pub trait Wire {
    /// The plugin's data folder.
    fn data(&self) -> &dyn Volume;
    /// The card the notes files are on.
    fn notes(&self) -> &dyn Volume;
    /// The token kept, if there is one.
    ///
    /// # Errors
    /// `NoMemory`; what the secrets service says, except that there is none.
    fn token(&mut self) -> Result<Option<Secret>, Error>;
    /// Whether the network can be used at all (the service is there and the
    /// plugin may use it).
    ///
    /// # Errors
    /// `Unsupported` or `Denied` without it.
    fn network(&mut self) -> Result<(), Error>;
    /// Sends the file [`BODY`] to Readwise with `token`, the end told with
    /// token [`REQUEST_TOKEN`].
    ///
    /// # Errors
    /// What the network service says.
    fn post(&mut self, token: &Secret) -> Result<RequestId, Error>;
    /// Ends the request in flight.
    fn cancel(&mut self);
}

/// What started a sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Start {
    /// A book's notes were exported, or the token entered after.
    Export,
    /// By itself (the work is due, the start): says only what went well.
    Retry,
}

/// What to tell the reader of a sending that ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Tell {
    /// This many highlights went to Readwise.
    Sent(i64),
    /// There was nothing to send.
    Nothing,
    /// There is no network for it.
    NoNetwork,
    /// Readwise could not be reached.
    Unreachable,
    /// Readwise refused the token.
    BadToken,
    /// Not every file could be sent.
    Failed,
}

/// What a step came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// Under way, or nothing to say (a sending that runs by itself says only
    /// what went well).
    Quiet,
    /// The sending ended: the reader is told.
    Told(Tell),
    /// There is no token: the reader is to be asked for it.
    NeedsToken,
}

/// The request in flight and the notes it carries.
#[derive(Debug)]
struct Flight {
    id: RequestId,
    path: String,
    count: usize,
}

/// A sending under way.
#[derive(Debug)]
struct Run {
    quiet: bool,
    token: Secret,
    queue: Vec<String>,
    flight: Option<Flight>,
    /// Highlights sent.
    sent: i64,
    /// Notes files not sent.
    failed: usize,
}

/// The sending: at most one at a time.
#[derive(Debug, Default)]
pub struct Sender {
    run: Option<Run>,
    /// The token Readwise refused: it stays kept, but is not sent by itself
    /// again; the next export asks for one. Another token kept, or this one
    /// entered anew, is sent again.
    refused: Option<Secret>,
}

/// The end of a request, as far as the sending reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Done {
    /// The request.
    pub id: RequestId,
    /// The plugin's own number of it.
    pub token: u32,
    /// Why it ended.
    pub reason: Reason,
    /// The HTTP status; 0 when none came.
    pub status: u32,
}

impl Done {
    /// The end of the request `id`.
    #[must_use]
    pub const fn new(id: RequestId, token: u32, reason: Reason, status: u32) -> Self {
        Self {
            id,
            token,
            reason,
            status,
        }
    }
}

/// A line without its break, cut at [`MAX_LINE`] bytes, as text.
fn lossy(bytes: &[u8]) -> Result<alloc::borrow::Cow<'_, str>, Error> {
    if let Ok(text) = core::str::from_utf8(bytes) {
        return Ok(alloc::borrow::Cow::Borrowed(text));
    }
    let mut out = String::new();
    out.try_reserve(bytes.len()).map_err(|_| Error::NoMemory)?;
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            out.try_reserve(3).map_err(|_| Error::NoMemory)?;
            out.push('\u{fffd}');
        }
    }
    Ok(alloc::borrow::Cow::Owned(out))
}

/// Reads the text file `name` a line at a time as it streams from the card, so
/// a file of any length is read in little memory: `visit` gets each line
/// without its line break (`\n` or `\r\n`, invalid UTF-8 replaced, a byte order
/// mark at the start left out) and says whether to go on. A line is cut at
/// 4,096 bytes.
///
/// # Errors
/// `NoMemory`, what the card says, or what `visit` says.
pub fn for_each_line(
    volume: &dyn Volume,
    name: &str,
    visit: &mut dyn FnMut(&str) -> Result<ControlFlow<()>, Error>,
) -> Result<(), Error> {
    let mut chunk = [0u8; PIECE];
    let mut line: Vec<u8> = Vec::new();
    let mut at = 0u64;
    let mut first = true;
    let mut emit = |line: &mut Vec<u8>| -> Result<ControlFlow<()>, Error> {
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        let mut bytes = line.as_slice();
        if first {
            bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        }
        first = false;
        let flow = visit(&lossy(bytes)?);
        line.clear();
        flow
    };
    loop {
        let n = volume.read_at(name, at, &mut chunk)?;
        if n == 0 {
            break;
        }
        at += n as u64;
        for piece in chunk
            .get(..n)
            .unwrap_or(&[])
            .split_inclusive(|b| *b == b'\n')
        {
            let (body, ends) = match piece.split_last() {
                Some((b'\n', body)) => (body, true),
                _ => (piece, false),
            };
            let take = body.len().min(MAX_LINE.saturating_sub(line.len()));
            line.try_reserve(take).map_err(|_| Error::NoMemory)?;
            line.extend_from_slice(body.get(..take).unwrap_or(&[]));
            if ends && emit(&mut line)?.is_break() {
                return Ok(());
            }
        }
    }
    if !line.is_empty() {
        let _ = emit(&mut line)?;
    }
    Ok(())
}

/// The notes files waiting, the oldest first.
///
/// # Errors
/// `NoMemory`; what the card says, except that there is no list.
pub fn waiting(volume: &dyn Volume) -> Result<Vec<String>, Error> {
    let mut paths: Vec<String> = Vec::new();
    let read = for_each_line(volume, WAITING, &mut |line| {
        if !line.is_empty() && !paths.iter().any(|p| p == line) {
            paths.try_reserve(1).map_err(|_| Error::NoMemory)?;
            paths.push(text::copy(line)?);
        }
        Ok(ControlFlow::Continue(()))
    });
    match read {
        Ok(()) | Err(Error::NotFound) => Ok(paths),
        Err(error) => Err(error),
    }
}

/// Adds the notes file `path` to those waiting, at the end.
///
/// # Errors
/// What the card says; the list is as it was then.
pub fn add_waiting(volume: &dyn Volume, path: &str) -> Result<(), Error> {
    rewrite(volume, path, Some(path))
}

/// The notes file `path` waits no more.
fn remove_waiting(volume: &dyn Volume, path: &str) -> Result<(), Error> {
    rewrite(volume, path, None)
}

/// Writes `waiting.txt` anew without the line `key`, with `line` at the end if
/// given: through a copy, line by line. On any error the list is left as it was.
fn rewrite(volume: &dyn Volume, key: &str, line: Option<&str>) -> Result<(), Error> {
    volume.write(WAITING_NEW, &[])?;
    let mut result = Ok(());
    let read = for_each_line(volume, WAITING, &mut |old| {
        if old.is_empty() || old == key {
            return Ok(ControlFlow::Continue(()));
        }
        result = append_line(volume, old);
        Ok(if result.is_ok() {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        })
    });
    match read {
        Ok(()) | Err(Error::NotFound) => {}
        Err(error) => result = Err(error),
    }
    if let Some(line) = line {
        result = result.and_then(|()| append_line(volume, line));
    }
    match result {
        Ok(()) => volume.rename(WAITING_NEW, WAITING),
        Err(error) => {
            let _ = volume.remove(WAITING_NEW);
            Err(error)
        }
    }
}

/// Appends `line` and its break, in as many writes as the storage takes.
fn append_line(volume: &dyn Volume, line: &str) -> Result<(), Error> {
    for part in line.as_bytes().chunks(MAX_WRITE) {
        volume.append(WAITING_NEW, part)?;
    }
    volume.append(WAITING_NEW, b"\n")
}

/// Writes the body of the request for the notes file `path` to [`BODY`], a
/// highlight at a time: how many highlights it holds.
fn write_body(wire: &dyn Wire, path: &str) -> Result<usize, Error> {
    let (data, notes) = (wire.data(), wire.notes());
    data.write(BODY, b"{\"highlights\":[")?;
    let mut parser = Parser::default();
    let mut count = 0;
    for_each_line(notes, path, &mut |line| {
        if let Some(highlight) = parser.line(line)? {
            append(data, &parser.json(&highlight)?, &mut count)?;
        }
        Ok(ControlFlow::Continue(()))
    })?;
    if let Some(highlight) = parser.finish() {
        append(data, &parser.json(&highlight)?, &mut count)?;
    }
    data.append(BODY, b"]}")?;
    Ok(count)
}

/// Appends the highlight `json` to [`BODY`], the `count`th, in calls the
/// storage takes.
fn append(data: &dyn Volume, json: &str, count: &mut usize) -> Result<(), Error> {
    if *count > 0 {
        data.append(BODY, b",")?;
    }
    for part in json.as_bytes().chunks(MAX_WRITE) {
        data.append(BODY, part)?;
    }
    *count += 1;
    Ok(())
}

impl Sender {
    /// A new one, with nothing under way.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            run: None,
            refused: None,
        }
    }

    /// Whether a sending is under way.
    #[must_use]
    pub const fn busy(&self) -> bool {
        self.run.is_some()
    }

    /// A notes file was exported: it waits, and the sending starts.
    ///
    /// # Errors
    /// `NoMemory`; what the card says while the file is put on the list (then
    /// nothing is sent).
    pub fn exported(&mut self, wire: &mut dyn Wire, path: &str) -> Result<Step, Error> {
        add_waiting(wire.data(), path)?;
        self.start(wire, Start::Export, None)
    }

    /// Sends the highlights that wait, with `token` if given (else the one
    /// kept), or says why it cannot.
    ///
    /// # Errors
    /// `NoMemory` (nothing is under way then); other failures count against
    /// the notes file they came with.
    pub fn start(
        &mut self,
        wire: &mut dyn Wire,
        start: Start,
        token: Option<Secret>,
    ) -> Result<Step, Error> {
        if token.is_some() {
            self.refused = None;
        }
        let waiting = match waiting(wire.data()) {
            Ok(waiting) => waiting,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // A list that cannot be read is tried again later.
            Err(_) => return Ok(Step::Quiet),
        };
        if let Some(run) = self.run.as_mut() {
            // Those exported meanwhile come along.
            for path in waiting {
                let current = run.flight.as_ref().is_some_and(|f| f.path == path);
                if !current && !run.queue.contains(&path) {
                    run.queue.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    run.queue.push(path);
                }
            }
            return Ok(Step::Quiet);
        }
        if waiting.is_empty() {
            return Ok(Step::Quiet);
        }
        let token = match token {
            Some(token) => token,
            None => match wire.token()? {
                Some(kept) if self.refused.as_ref() != Some(&kept) => kept,
                _ => {
                    return Ok(if start == Start::Export {
                        Step::NeedsToken
                    } else {
                        Step::Quiet
                    });
                }
            },
        };
        self.run = Some(Run {
            quiet: start == Start::Retry,
            token,
            queue: waiting,
            flight: None,
            sent: 0,
            failed: 0,
        });
        match wire.network() {
            Ok(()) => self.next(wire),
            Err(Error::Unsupported | Error::Denied) => Ok(self.end(wire, Tell::NoNetwork)),
            Err(error) => {
                self.end(wire, Tell::Failed);
                Err(error)
            }
        }
    }

    /// Cancels the sending under way; its request ends by itself and is
    /// ignored.
    pub fn cancel(&mut self, wire: &mut dyn Wire) {
        if let Some(run) = self.run.take()
            && run.flight.is_some()
        {
            wire.cancel();
        }
        let _ = wire.data().remove(BODY);
    }

    /// Sends the next notes file's highlights, or says how it went.
    fn next(&mut self, wire: &mut dyn Wire) -> Result<Step, Error> {
        loop {
            let Some(run) = self.run.as_mut() else {
                return Ok(Step::Quiet);
            };
            if run.queue.is_empty() {
                return Ok(self.finish(wire));
            }
            let path = run.queue.remove(0);
            if let Err(Error::NotFound) = wire.notes().size(&path) {
                // Its notes were deleted since.
                let _ = remove_waiting(wire.data(), &path);
                continue;
            }
            match write_body(wire, &path) {
                Ok(0) => {
                    let _ = remove_waiting(wire.data(), &path);
                }
                Ok(count) => {
                    let sent = match self.run.as_ref() {
                        Some(run) => wire.post(&run.token),
                        None => return Ok(Step::Quiet),
                    };
                    match sent {
                        Ok(id) => {
                            if let Some(run) = self.run.as_mut() {
                                run.flight = Some(Flight { id, path, count });
                            }
                            return Ok(Step::Quiet);
                        }
                        // Kept waiting, as if Readwise had failed; no network
                        // ends it all.
                        Err(Error::Unsupported | Error::Denied) => {
                            return Ok(self.end(wire, Tell::NoNetwork));
                        }
                        Err(Error::NoMemory) => {
                            self.end(wire, Tell::Failed);
                            return Err(Error::NoMemory);
                        }
                        Err(_) => self.fail_one(),
                    }
                }
                Err(Error::NoMemory) => {
                    self.end(wire, Tell::Failed);
                    return Err(Error::NoMemory);
                }
                // Kept waiting, as if Readwise had failed.
                Err(_) => self.fail_one(),
            }
        }
    }

    fn fail_one(&mut self) {
        if let Some(run) = self.run.as_mut() {
            run.failed += 1;
        }
    }

    /// Readwise answered (or the request failed): the next file, or the end.
    ///
    /// # Errors
    /// `NoMemory` (the sending ends, the files keep waiting).
    pub fn done(&mut self, wire: &mut dyn Wire, done: &Done) -> Result<Step, Error> {
        if done.token != REQUEST_TOKEN {
            return Ok(Step::Quiet);
        }
        let Some(run) = self.run.as_mut() else {
            return Ok(Step::Quiet);
        };
        if run.flight.as_ref().map(|f| f.id) != Some(done.id) {
            return Ok(Step::Quiet);
        }
        let Some(flight) = run.flight.take() else {
            return Ok(Step::Quiet);
        };
        match done.reason {
            // By this plugin or by a newer request: nothing to learn.
            Reason::Cancelled | Reason::Replaced => {
                self.run = None;
                let _ = wire.data().remove(BODY);
                Ok(Step::Quiet)
            }
            // The run's 10 minutes are over: no request is to follow outside it.
            Reason::WorkEnded => Ok(self.end(wire, Tell::Unreachable)),
            Reason::Ok => self.answered(wire, flight, done.status),
            // No answer came (interface 6: status 0).
            _ => Ok(self.end(wire, Tell::Unreachable)),
        }
    }

    fn answered(
        &mut self,
        wire: &mut dyn Wire,
        flight: Flight,
        status: u32,
    ) -> Result<Step, Error> {
        match status {
            200..=299 => {
                // A list that cannot be written keeps the file: it is sent
                // again, and Readwise keeps each highlight once.
                let _ = remove_waiting(wire.data(), &flight.path);
                if let Some(run) = self.run.as_mut() {
                    run.sent += i64::try_from(flight.count).unwrap_or(i64::MAX);
                }
                self.next(wire)
            }
            401 | 403 => {
                // Asked for anew with the next export; kept meanwhile, as
                // Readwise may refuse it only for a while.
                let _ = wire.data().remove(BODY);
                let Some(run) = self.run.take() else {
                    return Ok(Step::Quiet);
                };
                let quiet = run.quiet;
                self.refused = Some(run.token);
                Ok(if quiet {
                    Step::Quiet
                } else {
                    Step::Told(Tell::BadToken)
                })
            }
            _ => {
                self.fail_one();
                self.next(wire)
            }
        }
    }

    /// All notes files were tried: says how it went.
    fn finish(&mut self, wire: &mut dyn Wire) -> Step {
        let Some(run) = self.run.as_ref() else {
            return Step::Quiet;
        };
        if run.failed > 0 {
            return self.end(wire, Tell::Failed);
        }
        let sent = run.sent;
        if sent == 0 {
            return self.end(wire, Tell::Nothing);
        }
        if let Some(run) = self.run.as_mut() {
            run.quiet = false;
        }
        self.end(wire, Tell::Sent(sent))
    }

    /// Ends the sending with `tell`; a sending that runs by itself tells only
    /// what went well.
    fn end(&mut self, wire: &mut dyn Wire, tell: Tell) -> Step {
        let _ = wire.data().remove(BODY);
        let quiet = self.run.take().is_none_or(|run| run.quiet);
        if quiet { Step::Quiet } else { Step::Told(tell) }
    }
}
