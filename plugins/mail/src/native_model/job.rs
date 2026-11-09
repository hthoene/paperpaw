//! One connection to a server, as the plugin keeps it between callbacks: which
//! session runs, how far it came and how it ended. The socket is lent for each
//! callback (see [`Link`]); a socket that is silent for too long ends with the
//! host's timeout (`idle_ms` of the connection), which reaches the session as a
//! failed socket, so no timer of the job's own is kept.
use super::fail::Fail;
use super::link::{Link, LinkState, Net, Notice};
use super::mem::Error;
use super::session::{Endpoint, Event, Failure, Imap, Input, Io, Outgoing, Smtp, Stage, Task};
use alloc::vec::Vec;
use paperpaw_sdk::{Storage, native::sockets::Untrusted};

/// Where a job is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Run {
    /// Nothing started.
    #[default]
    Idle,
    /// Working, and where.
    Working(Stage),
    /// Everything was done.
    Done,
    /// It ended without finishing.
    Failed(Failure),
    /// The server's certificate is not trusted: its hashes for the reader to
    /// compare, and which of the account's servers it is.
    Untrusted {
        /// The SMTP server, not the IMAP one.
        smtp: bool,
        /// SHA-256 of the leaf certificate.
        certificate: [u8; 32],
        /// SHA-256 of its public key.
        key: [u8; 32],
        /// Why.
        why: Untrusted,
    },
}

#[derive(Debug, Default)]
enum Session {
    #[default]
    None,
    Imap(Imap),
    Smtp(Smtp),
}

/// A connection, in the making or over.
#[derive(Debug, Default)]
pub struct Job {
    state: LinkState,
    session: Session,
    /// How far it is.
    pub run: Run,
}

impl Job {
    /// A job with nothing to do.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a connection is going.
    #[must_use]
    pub fn working(&self) -> bool {
        matches!(self.run, Run::Working(_))
    }

    /// Whether the connection saves a message into a file (it takes longer).
    #[must_use]
    pub fn receiving(&self) -> bool {
        matches!(&self.session, Session::Imap(imap) if imap.receiving())
    }

    /// Starts `tasks` on an IMAP server over `net`. Nothing is done while
    /// another connection of the job is open.
    ///
    /// # Errors
    /// [`Error::Busy`] while a connection is open.
    pub fn start_imap<N: Net>(
        &mut self,
        net: N,
        endpoint: Endpoint,
        tasks: Vec<Task>,
    ) -> Result<(), Error> {
        if self.working() {
            return Err(Error::Busy);
        }
        self.state = LinkState::default();
        let mut imap = Imap::new(endpoint, tasks);
        let mut link = Link::new(&mut self.state, net);
        let events = imap.start(&mut link);
        self.session = Session::Imap(imap);
        self.apply(events);
        Ok(())
    }

    /// Sends `message` through an SMTP server over `net`.
    ///
    /// # Errors
    /// [`Error::Busy`] while a connection is open.
    pub fn start_smtp<N: Net>(
        &mut self,
        net: N,
        endpoint: Endpoint,
        message: Outgoing,
    ) -> Result<(), Error> {
        if self.working() {
            return Err(Error::Busy);
        }
        self.state = LinkState::default();
        let mut smtp = Smtp::new(endpoint, message);
        let mut link = Link::new(&mut self.state, net);
        let events = smtp.start(&mut link);
        self.session = Session::Smtp(smtp);
        self.apply(events);
        Ok(())
    }

    /// Lets go of the connection: its socket is closed, what waited to be
    /// sent is wiped, and the job is idle again.
    pub fn cancel<N: Net>(&mut self, net: N) {
        let mut link = Link::new(&mut self.state, net);
        Io::close(&mut link);
        self.reset();
    }

    /// The job is idle again, whatever it was doing: the session (which holds
    /// the password) is dropped and what waited to be sent is wiped. The socket
    /// is the caller's to close.
    pub fn reset(&mut self) {
        self.state = LinkState::default();
        self.session = Session::None;
        self.run = Run::Idle;
    }

    /// A notice of the socket: the session takes what it means. A notice when
    /// no session runs is not for the job.
    ///
    /// # Errors
    /// What the host says where a piece of what came cannot be read or what
    /// waits cannot be sent: the job ends as failed, and the error is passed on
    /// so that the caller can close the socket.
    pub fn notice<N: Net>(
        &mut self,
        net: N,
        notice: Notice,
        storage: &dyn Storage,
    ) -> Result<(), Error> {
        if !self.working() {
            return Ok(());
        }
        let mut events: Vec<Result<Vec<Event>, Fail>> = Vec::new();
        let mut link = Link::new(&mut self.state, net);
        let session = &mut self.session;
        let result = link.notice(notice, |input: Input<'_>, io| {
            let told = match session {
                Session::Imap(imap) => imap.step(input, io, storage),
                Session::Smtp(smtp) => smtp.step(input, io),
                Session::None => return false,
            };
            let went_on = matches!(&told, Ok(told) if !ended(told));
            if events.try_reserve(1).is_ok() {
                events.push(told);
            }
            went_on
        });
        if result.is_err() {
            // A host error ends the job: the socket goes with it, not at the
            // host's idle timeout, and what waited to be sent is wiped.
            Io::close(&mut link);
        }
        for told in events {
            self.apply(told);
        }
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                self.run = Run::Failed(failure_of(error));
                self.session = Session::None;
                Err(error)
            }
        }
    }

    /// What a session told.
    fn apply(&mut self, told: Result<Vec<Event>, Fail>) {
        let smtp = matches!(self.session, Session::Smtp(_));
        let Ok(events) = told else {
            // Not even the failure could be told.
            self.run = Run::Failed(Failure::NoMemory);
            self.session = Session::None;
            return;
        };
        for event in events {
            match event {
                Event::Stage(stage) => self.run = Run::Working(stage),
                Event::Done => self.run = Run::Done,
                Event::Failed(failure) => self.run = Run::Failed(failure),
                Event::Untrusted {
                    certificate,
                    key,
                    why,
                } => {
                    self.run = Run::Untrusted {
                        smtp,
                        certificate,
                        key,
                        why,
                    }
                }
            }
        }
        if !self.working() {
            self.session = Session::None;
        }
    }
}

/// Whether `events` end the session.
fn ended(events: &[Event]) -> bool {
    events
        .iter()
        .any(|e| matches!(e, Event::Done | Event::Failed(_) | Event::Untrusted { .. }))
}

/// How a host error reads as the end of a connection.
fn failure_of(error: Error) -> Failure {
    match error {
        Error::NoMemory => Failure::NoMemory,
        Error::Busy => Failure::Busy,
        Error::Unsupported => Failure::NoTls,
        _ => Failure::Network(paperpaw_sdk::native::sockets::Reason::Io),
    }
}
