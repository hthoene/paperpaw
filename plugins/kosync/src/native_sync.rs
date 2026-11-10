//! The progress sync's decisions over time, without host calls: which book is
//! open and where, what was sent and when, what is held back until the
//! server's progress is known, what the user answered to an offer, and what
//! waits while the power mode is Offline. It is interface 6's `State`
//! (`lib.rs`) with every allocation fallible and every host effect an
//! [`Action`] the caller carries out: a request to make, an offer to show, a
//! notice to say. The caller says what the host knows at the moment
//! ([`Ctx`]) and reads the server's answers ([`Answer`], with
//! `native_model::read_answer`).
//!
//! What stays with the caller: the book's fingerprint and the XPointer of a
//! place (they read the book), the request (`native_model::progress_body`,
//! `server`, the account), the work declaration (`Action::PushLater`), and
//! the device id.
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::push, text::copy};

use crate::native_model::{self, Remote, Seen, Verdict};

/// The progress is sent at most this often while reading.
pub const PUSH_EVERY_MS: u64 = 60_000;
/// The same book's progress is fetched again only after this long.
pub const PULL_EVERY_MS: u64 = 60_000;
/// Books switched from while Offline, their progress sent once allowed.
pub const MOST_UNSENT: usize = 4;
/// Offers left without a choice, kept so they do not come again for the same
/// position.
pub const MOST_DISMISSED: usize = 4;
/// Books left whose progress is held back while their fetch is out: the books
/// switched from while Offline ([`MOST_UNSENT`]) and the book open when the
/// network came back, so none is dropped; the oldest goes first beyond that.
pub const MOST_LEFT: usize = MOST_UNSENT + 1;

/// Where the reader is in which book.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Position {
    /// The book's path.
    pub path: String,
    /// The place: section and offset (`doc_at`).
    pub at: (u32, u32),
    /// Reading progress from 0 to 1, as the reader shows it.
    pub progress: f64,
}

impl Position {
    /// A position.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(path: &str, at: (u32, u32), progress: f64) -> Result<Self, Error> {
        Ok(Self {
            path: copy(path)?,
            at,
            progress,
        })
    }

    /// A copy of this position.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn try_clone(&self) -> Result<Self, Error> {
        Self::new(&self.path, self.at, self.progress)
    }
}

/// What the host knows when the engine is asked.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct Ctx {
    /// A monotonic clock, in milliseconds.
    pub now_ms: u64,
    /// The device has a network (`caps().network`).
    pub network: bool,
    /// *Sync by themselves* is on (`kosync.auto`).
    pub auto: bool,
    /// The user and the account's key are set.
    pub signed_in: bool,
    /// The power mode lets the network be used (not Offline).
    pub online: bool,
    /// Sending now turns on no radio that would be off otherwise.
    pub radio_on_anyway: bool,
}

impl Ctx {
    /// A context at `now_ms`: a network, *sync by themselves* on, signed in,
    /// online, the radio on anyway.
    #[must_use]
    pub const fn new(now_ms: u64) -> Self {
        Self {
            now_ms,
            network: true,
            auto: true,
            signed_in: true,
            online: true,
            radio_on_anyway: true,
        }
    }

    /// Syncs by itself at all.
    #[must_use]
    pub const fn active(&self) -> bool {
        self.network && self.auto && self.signed_in
    }

    /// Syncs by itself now.
    #[must_use]
    pub const fn may_send(&self) -> bool {
        self.active() && self.online
    }
}

/// A notice said to the user (only for a sync they asked for).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Notice {
    /// The server refused or failed: the text of `native_model::failure`.
    Failed(u32),
    /// The server's answer was no progress.
    Unreadable(u32),
    /// Nothing further elsewhere.
    UpToDate(u32),
}

/// The offer to go where another device is.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Offer {
    /// The book.
    pub path: String,
    /// The position there, as KOReader's XPointer (may be empty).
    pub xpointer: String,
    /// The server's progress, as text (the key of *dismissed* and *declined*).
    pub percentage: String,
    /// The other device's name.
    pub device: String,
}

/// What the caller carries out.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum Action {
    /// Ask the server for the book's progress; the answer is
    /// [`Engine::pulled`] with these three.
    Fetch {
        /// The book.
        path: String,
        /// This device's progress (0..1).
        local: f64,
        /// The user asked for it.
        manual: bool,
    },
    /// Put the position to the server. When it cannot be made (the book
    /// does not read) or fails, call [`Engine::not_sent`]. When a `Fetch`
    /// cannot be made, call [`Engine::not_fetched`].
    Send(Position),
    /// Show the offer.
    Offer(Offer),
    /// Say something.
    Notice(Notice),
    /// Declare the work that sends the progress when the radio is on anyway,
    /// at the latest in half an hour (once: it is not declared again while
    /// declared).
    PushLater,
}

/// Sending held back for a book while its progress on the server is being
/// fetched and, if another device is further there, until the user chose
/// whether to go there: sending first would overwrite the further position.
#[derive(Debug, PartialEq)]
struct Hold {
    path: String,
    /// Send this device's position when the hold ends with it kept.
    send: bool,
    /// Send it even if it was sent already.
    force: bool,
    /// The position to send then, kept as it was when it was held back.
    position: Option<Position>,
}

impl Hold {
    fn new(path: &str) -> Result<Self, Error> {
        Ok(Self {
            path: copy(path)?,
            send: false,
            force: false,
            position: None,
        })
    }
}

/// A fetch's answer.
#[derive(Debug)]
#[non_exhaustive]
pub struct Answer<'a> {
    /// The book the fetch was for.
    pub path: &'a str,
    /// The user asked for it.
    pub manual: bool,
    /// This device's progress when it was asked.
    pub local: f64,
    /// The HTTP status.
    pub status: u32,
    /// What a 200 answer says (`native_model::read_answer`); `None` for any
    /// other status and for an answer that is no progress.
    pub remote: Option<&'a Remote>,
}

impl<'a> Answer<'a> {
    /// A fetch's answer.
    #[must_use]
    pub const fn new(
        path: &'a str,
        manual: bool,
        local: f64,
        status: u32,
        remote: Option<&'a Remote>,
    ) -> Self {
        Self {
            path,
            manual,
            local,
            status,
            remote,
        }
    }
}

/// What the user answered to an offer.
#[derive(Debug)]
pub struct Answered<'a> {
    /// The book.
    pub path: &'a str,
    /// The progress offered, as text.
    pub percentage: &'a str,
    /// They went there.
    pub went: bool,
    /// They chose to stay here.
    pub stay: bool,
}

/// The sync's state.
#[derive(Debug, Default)]
pub struct Engine {
    device_id: String,
    current: Option<Position>,
    sent: Option<Position>,
    last_push: Option<u64>,
    last_pull: Option<(String, u64)>,
    read: Option<String>,
    hold: Option<Hold>,
    left: Vec<Hold>,
    declined: Option<(String, String)>,
    dismissed: Vec<(String, String)>,
    unsent: Vec<Position>,
}

fn is(path: &Option<Position>, other: &str) -> bool {
    path.as_ref().is_some_and(|c| c.path == other)
}

impl Engine {
    /// An engine for the device with this id (the progress sent from here is
    /// known by it).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(device_id: &str) -> Result<Self, Error> {
        Ok(Self {
            device_id: copy(device_id)?,
            ..Self::default()
        })
    }

    /// This device's id.
    #[must_use]
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// The position of the open book, if any.
    #[must_use]
    pub fn current(&self) -> Option<&Position> {
        self.current.as_ref()
    }

    /// The position was not sent after all (the book would not read, the
    /// server said no): again with the next page.
    pub fn not_sent(&mut self) {
        self.sent = None;
        self.last_push = None;
    }

    /// The fetch of the book at `path` could not be made (it does not read):
    /// asked again at the next chance, not after a minute. Its sending stays
    /// held back, as when the fetch fails.
    pub fn not_fetched(&mut self, path: &str) {
        if self.last_pull.as_ref().is_some_and(|(p, _)| p == path) {
            self.last_pull = None;
        }
    }

    /// The account changed: what was learnt about the server's progress is
    /// not known any more.
    pub fn account_changed(&mut self) {
        self.read = None;
        self.sent = None;
        self.last_push = None;
        self.last_pull = None;
    }

    /// Sends what waited while Offline, then fetches the open book's latest.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn allowed(&mut self, ctx: Ctx, out: &mut Vec<Action>) -> Result<(), Error> {
        if !ctx.may_send() {
            return Ok(());
        }
        let open = self.current.as_ref().map(|c| c.path.as_str());
        let mut waiting = core::mem::take(&mut self.unsent);
        waiting.retain(|p| Some(p.path.as_str()) != open);
        for position in waiting {
            self.pull_left(ctx, out, position)?;
        }
        if let Some(path) = self.current.as_ref().map(|c| copy(&c.path)).transpose()? {
            self.pull(ctx, out, &path, false)?;
        }
        self.push(ctx, out, false)
    }

    /// A book was opened.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn book_opened(
        &mut self,
        ctx: Ctx,
        position: Position,
        out: &mut Vec<Action>,
    ) -> Result<(), Error> {
        let other = self
            .current
            .as_ref()
            .is_some_and(|c| c.path != position.path);
        if ctx.active() && other {
            if ctx.may_send() {
                self.push(ctx, out, false)?;
            } else if let Some(last) = self.current.as_ref().map(Position::try_clone).transpose()? {
                // Offline: sent once allowed, silently.
                self.unsent.retain(|u| u.path != last.path);
                push(&mut self.unsent, last)?;
                let extra = self.unsent.len().saturating_sub(MOST_UNSENT);
                self.unsent.drain(..extra);
            }
        }
        let path = copy(&position.path)?;
        if other {
            self.read = None;
        }
        self.current = Some(position);
        if !self.pulled_lately(ctx, &path) && ctx.may_send() {
            self.pull(ctx, out, &path, false)?;
        }
        Ok(())
    }

    /// A page turned.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn page_turned(
        &mut self,
        ctx: Ctx,
        position: Position,
        out: &mut Vec<Action>,
    ) -> Result<(), Error> {
        self.current = Some(position);
        let due = self
            .last_push
            .is_none_or(|at| ctx.now_ms.saturating_sub(at) >= PUSH_EVERY_MS);
        if due && ctx.may_send() && ctx.radio_on_anyway {
            self.push(ctx, out, false)
        } else if ctx.active() {
            push(out, Action::PushLater)
        } else {
            Ok(())
        }
    }

    /// The book was closed, where it was left.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn book_closed(
        &mut self,
        ctx: Ctx,
        position: Position,
        out: &mut Vec<Action>,
    ) -> Result<(), Error> {
        self.current = Some(position);
        self.send_now(ctx, out)
    }

    /// The declared work is due, or the device is going to sleep: send what
    /// was not sent.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn send_now(&mut self, ctx: Ctx, out: &mut Vec<Action>) -> Result<(), Error> {
        if ctx.may_send() {
            self.push(ctx, out, false)?;
        }
        Ok(())
    }

    /// The device woke: maybe read on another device meanwhile.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn resumed(&mut self, ctx: Ctx, out: &mut Vec<Action>) -> Result<(), Error> {
        let Some(path) = self.current.as_ref().map(|c| copy(&c.path)).transpose()? else {
            return Ok(());
        };
        if !self.pulled_lately(ctx, &path) && ctx.may_send() {
            self.pull(ctx, out, &path, false)?;
        }
        Ok(())
    }

    /// *Sync progress*: the server's first, sent after its answer unless
    /// another device is further and the user goes there. `false` when there
    /// is no account (the caller opens the account screen); nothing happens
    /// when no book is open.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn sync_now(&mut self, ctx: Ctx, out: &mut Vec<Action>) -> Result<bool, Error> {
        if !ctx.network || !ctx.signed_in {
            return Ok(false);
        }
        if let Some(path) = self.current.as_ref().map(|c| copy(&c.path)).transpose()? {
            self.pull(ctx, out, &path, true)?;
        }
        Ok(true)
    }

    /// Sends the current position, unless it was sent already or is held back
    /// (see [`Hold`]). The server's progress not read yet for the book (its
    /// fetch failed, say), it is fetched first.
    fn push(&mut self, ctx: Ctx, out: &mut Vec<Action>, force: bool) -> Result<(), Error> {
        let Some(position) = self.current.as_ref().map(Position::try_clone).transpose()? else {
            return Ok(());
        };
        if self.read.as_deref() != Some(position.path.as_str()) {
            if !self.pulled_lately(ctx, &position.path) {
                self.pull(ctx, out, &position.path, false)?;
            }
            if self.hold.as_ref().is_none_or(|h| h.path != position.path) {
                // Not fetched now (tried a moment ago): held all the same.
                let held = self.hold_for(&position.path)?;
                self.hold = Some(match held {
                    Some(held) => held,
                    None => Hold::new(&position.path)?,
                });
            }
        }
        if let Some(hold) = self.hold.as_mut().filter(|h| h.path == position.path) {
            // Sent when the hold ends, if this position is kept.
            hold.send = true;
            hold.force |= force;
            hold.position = Some(position);
            return Ok(());
        }
        self.send(ctx, out, position, force)
    }

    /// Sends `position`, unless it was sent already (or `force`).
    fn send(
        &mut self,
        ctx: Ctx,
        out: &mut Vec<Action>,
        position: Position,
        force: bool,
    ) -> Result<(), Error> {
        if !force && self.sent.as_ref() == Some(&position) {
            return Ok(());
        }
        if !ctx.signed_in {
            return Ok(());
        }
        let tracked = position.try_clone()?;
        push(out, Action::Send(position))?;
        self.sent = Some(tracked);
        self.last_push = Some(ctx.now_ms);
        Ok(())
    }

    /// Asks for the latest position of the book at `path`, holding back its
    /// sending until the answer.
    fn pull(
        &mut self,
        ctx: Ctx,
        out: &mut Vec<Action>,
        path: &str,
        manual: bool,
    ) -> Result<(), Error> {
        if !ctx.signed_in {
            return Ok(());
        }
        let local = self
            .current
            .as_ref()
            .filter(|c| c.path == path)
            .map_or(0.0, |c| c.progress);
        push(
            out,
            Action::Fetch {
                path: copy(path)?,
                local,
                manual,
            },
        )?;
        self.last_pull = Some((copy(path)?, ctx.now_ms));
        let held = self.hold_for(path)?;
        let (send, force) = held.as_ref().map_or((false, false), |h| (h.send, h.force));
        self.hold = Some(Hold {
            path: copy(path)?,
            send: manual || send,
            force: manual || force,
            position: held.and_then(|h| h.position),
        });
        Ok(())
    }

    /// Asks for the latest position of a book switched from: `position` is
    /// sent with the answer unless another device is further there.
    fn pull_left(
        &mut self,
        ctx: Ctx,
        out: &mut Vec<Action>,
        position: Position,
    ) -> Result<(), Error> {
        if !ctx.signed_in {
            return Ok(());
        }
        push(
            out,
            Action::Fetch {
                path: copy(&position.path)?,
                local: position.progress,
                manual: false,
            },
        )?;
        self.left.retain(|h| h.path != position.path);
        let hold = Hold {
            send: true,
            ..Hold::new(&position.path)?
        };
        self.keep_left(Hold {
            position: Some(position),
            ..hold
        })
    }

    /// Keeps the hold of a book left, the oldest going when there are too many.
    fn keep_left(&mut self, hold: Hold) -> Result<(), Error> {
        push(&mut self.left, hold)?;
        let extra = self.left.len().saturating_sub(MOST_LEFT);
        self.left.drain(..extra);
        Ok(())
    }

    /// Takes the hold of the book at `path`, also one kept among the books
    /// left; another book's hold is kept there, its answer still to come.
    fn hold_for(&mut self, path: &str) -> Result<Option<Hold>, Error> {
        let mut held = self.hold.take();
        if let Some(other) = held.take_if(|h| h.path != path)
            && other.send
        {
            self.left.retain(|h| h.path != other.path);
            self.keep_left(other)?;
        }
        if held.is_some() {
            return Ok(held);
        }
        Ok(self
            .left
            .iter()
            .position(|h| h.path == path)
            .map(|at| self.left.swap_remove(at)))
    }

    /// Whether the book at `path` was fetched for less than a while ago.
    fn pulled_lately(&self, ctx: Ctx, path: &str) -> bool {
        self.last_pull
            .as_ref()
            .is_some_and(|(p, at)| p == path && ctx.now_ms.saturating_sub(*at) < PULL_EVERY_MS)
    }

    /// The server's progress of the open book at `path` could not be read:
    /// its sending stays held back until a fetch succeeds.
    fn unread(&mut self, path: &str, hold: Option<Hold>) -> Result<(), Error> {
        if self.read.as_deref() == Some(path) {
            self.read = None;
        }
        let held = match hold {
            Some(hold) => Some(hold),
            None => self.hold_for(path)?,
        };
        self.hold = Some(match held {
            Some(held) => held,
            None => Hold::new(path)?,
        });
        Ok(())
    }

    /// The answer to a fetch.
    ///
    /// # Errors
    /// `NoMemory`.
    #[allow(
        clippy::too_many_lines,
        reason = "one decision, as interface 6 wrote it"
    )]
    pub fn pulled(
        &mut self,
        ctx: Ctx,
        answer: &Answer<'_>,
        out: &mut Vec<Action>,
    ) -> Result<(), Error> {
        let path = answer.path;
        let hold = self.hold.take_if(|h| h.path == path);
        // The book was left meanwhile: its position held back goes, unless
        // another device is further (no offer then, the book is closed).
        if !is(&self.current, path) {
            if let Some(at) = self.left.iter().position(|h| h.path == path) {
                let left = self.left.swap_remove(at);
                let from_here = answer
                    .remote
                    .is_some_and(|r| r.device_id.as_deref() == Some(self.device_id.as_str()));
                if let Some(position) = left.position
                    && native_model::send_left_behind(
                        answer.status,
                        answer.remote.map(|r| r.progress),
                        from_here,
                        position.progress,
                    )
                {
                    self.send(ctx, out, position, left.force)?;
                }
            }
            return Ok(());
        }
        if answer.status != 200 {
            if answer.manual {
                push(out, Action::Notice(Notice::Failed(answer.status)))?;
            }
            return self.unread(path, hold);
        }
        let Some(remote) = answer.remote else {
            // Not knowing the server's progress, nothing may overwrite it.
            if answer.manual {
                push(out, Action::Notice(Notice::Unreadable(answer.status)))?;
            }
            return self.unread(path, hold);
        };
        self.read = Some(copy(path)?);
        let offered = native_model::progress_text(remote.progress)?;
        let declined = self
            .declined
            .as_ref()
            .is_some_and(|(p, r)| p == path && *r == offered);
        let dismissed = self
            .dismissed
            .iter()
            .any(|(p, r)| p == path && *r == offered);
        let verdict = native_model::judge(Seen {
            remote: remote.progress,
            local: answer.local,
            sent_from_here: remote.device_id.as_deref() == Some(self.device_id.as_str()),
            declined,
            dismissed,
            manual: answer.manual,
        });
        match verdict {
            Verdict::HoldQuietly => {
                // Not now, the user said: held back still, without asking.
                self.hold = Some(match hold {
                    Some(hold) => hold,
                    None => Hold::new(path)?,
                });
            }
            Verdict::Offer => {
                // Further on another device: offer to go there, and send
                // nothing until the user chose.
                push(
                    out,
                    Action::Offer(Offer {
                        path: copy(path)?,
                        xpointer: copy(remote.xpointer.as_deref().unwrap_or_default())?,
                        percentage: offered,
                        device: copy(remote.device.as_deref().unwrap_or("?"))?,
                    }),
                )?;
                self.hold = Some(match hold {
                    Some(hold) => hold,
                    None => Hold::new(path)?,
                });
            }
            Verdict::UpToDate => {
                if answer.manual {
                    push(out, Action::Notice(Notice::UpToDate(answer.status)))?;
                }
                if let Some(hold) = hold.filter(|h| h.send) {
                    self.push(ctx, out, hold.force)?;
                }
            }
        }
        Ok(())
    }

    /// The user closed the offer to go where another device is.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn answered(
        &mut self,
        ctx: Ctx,
        answer: &Answered<'_>,
        out: &mut Vec<Action>,
    ) -> Result<(), Error> {
        let hold = self.hold.take_if(|h| h.path == answer.path);
        self.dismissed.retain(|(p, _)| p != answer.path);
        // Gone there: the reader tells the new position as it shows it.
        if answer.went {
            return Ok(());
        }
        if !answer.stay {
            // Left without a choice: not now. Held back still, and offered
            // again when the server's progress is another.
            push(
                &mut self.dismissed,
                (copy(answer.path)?, copy(answer.percentage)?),
            )?;
            let extra = self.dismissed.len().saturating_sub(MOST_DISMISSED);
            self.dismissed.drain(..extra);
            if self.hold.is_none() {
                self.hold = Some(match hold {
                    Some(hold) => hold,
                    None => Hold::new(answer.path)?,
                });
            } else {
                // Another book's hold meanwhile: fetched first again.
                self.read = None;
            }
            return Ok(());
        }
        self.declined = Some((copy(answer.path)?, copy(answer.percentage)?));
        // This device's position is the one to keep.
        if is(&self.current, answer.path) {
            self.push(ctx, out, true)?;
        }
        Ok(())
    }
}
