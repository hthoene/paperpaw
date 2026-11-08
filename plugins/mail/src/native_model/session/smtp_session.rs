//! The SMTP side of a mail session (see the parent module).
use super::super::codec;
use super::super::fail::Fail;
use super::super::imap::{Fault, LineBuffer};
use super::super::mem::{Error, Secret, copy, push, wipe_text};
use super::super::smtp::{self, Extensions, Reply, ReplyParser};
use super::{
    Endpoint, Event, Failure, Input, Io, Security, Stage, Wire, failure_of, io_failure, one,
};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::sockets::Reason;

/// A message to send: the envelope and the bytes DATA takes
/// ([`smtp::data`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Outgoing {
    /// The sender's address.
    pub from: String,
    /// The recipients' addresses.
    pub to: Vec<String>,
    /// The bytes DATA takes.
    pub data: Vec<u8>,
}

impl Outgoing {
    /// A message of `data` from `from` to `to`.
    #[must_use]
    pub fn new(from: String, to: Vec<String>, data: Vec<u8>) -> Self {
        Self { from, to, data }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Greeting,
    Ehlo,
    StartTls,
    SecureEhlo,
    AuthPlain,
    LoginUser,
    LoginName,
    LoginPassword,
    MailFrom,
    Rcpt(usize),
    Data,
    Body,
    Quit,
}

/// The SMTP side: one message sent in one connection.
#[derive(Debug)]
pub struct Smtp {
    endpoint: Endpoint,
    message: Outgoing,
    wire: Wire,
    replies: ReplyParser,
    step: Step,
    extensions: Extensions,
    secure: bool,
    finished: bool,
}

impl Smtp {
    /// A session that sends `message` at `endpoint`.
    #[must_use]
    pub fn new(endpoint: Endpoint, message: Outgoing) -> Self {
        Self {
            endpoint,
            message,
            wire: Wire::default(),
            replies: ReplyParser::default(),
            step: Step::Greeting,
            extensions: Extensions::default(),
            secure: false,
            finished: false,
        }
    }

    /// Opens the connection.
    ///
    /// # Errors
    /// [`Fail::NoMemory`] where not even the failure can be told.
    pub fn start(&mut self, io: &mut dyn Io) -> Result<Vec<Event>, Fail> {
        let mut events = Vec::new();
        let result = self.begin(io, &mut events);
        self.settle(io, events, result)
    }

    fn begin(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        let e = &self.endpoint;
        let connected = match e.security {
            Security::Implicit => io.connect_tls(&e.host, e.port, e.pin),
            Security::StartTls => io.connect_plain(&e.host, e.port),
        };
        push(events, Event::Stage(Stage::Connecting))?;
        self.went(io, events, connected)?;
        Ok(())
    }

    fn settle(
        &mut self,
        io: &mut dyn Io,
        events: Vec<Event>,
        result: Result<(), Fail>,
    ) -> Result<Vec<Event>, Fail> {
        match result {
            Ok(()) => Ok(events),
            Err(Fail::NoMemory) => {
                self.finished = true;
                io.close();
                drop(events);
                one(Event::Failed(Failure::NoMemory))
            }
            Err(other) => Err(other),
        }
    }

    fn fail(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        failure: Failure,
    ) -> Result<(), Fail> {
        self.finished = true;
        io.close();
        Ok(push(events, Event::Failed(failure))?)
    }

    fn went(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        result: Result<(), Error>,
    ) -> Result<bool, Fail> {
        match result {
            Ok(()) => Ok(true),
            Err(error) => match io_failure(error) {
                None => Err(Fail::NoMemory),
                Some(failure) => {
                    self.fail(io, events, failure)?;
                    Ok(false)
                }
            },
        }
    }

    /// Sends `parts` joined and `\r\n` after them, from a buffer that wipes
    /// itself, and waits for the reply of `step`.
    fn say(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        step: Step,
        parts: &[&str],
    ) -> Result<(), Fail> {
        self.step = step;
        let mut size = 2;
        for part in parts {
            size += part.len();
        }
        let mut line = Secret::with_capacity(size)?;
        for part in parts {
            line.push_str(part)?;
        }
        line.push_str("\r\n")?;
        let sent = io.send(line.as_str().as_bytes());
        drop(line);
        self.went(io, events, sent)?;
        Ok(())
    }

    /// What happened on the socket.
    ///
    /// # Errors
    /// [`Fail::NoMemory`] where not even the failure can be told.
    pub fn step(&mut self, input: Input<'_>, io: &mut dyn Io) -> Result<Vec<Event>, Fail> {
        let mut events = Vec::new();
        let result = self.take(input, io, &mut events);
        self.settle(io, events, result)
    }

    fn more(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<bool, Fail> {
        let asked = self.wire.more(io);
        self.went(io, events, asked)
    }

    fn take(
        &mut self,
        input: Input<'_>,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
    ) -> Result<(), Fail> {
        if self.finished {
            return Ok(());
        }
        match input {
            Input::Connected => {
                self.more(io, events)?;
            }
            Input::Secured { time_checked, .. } => {
                if !time_checked {
                    return self.fail(io, events, Failure::ClockNotSet);
                }
                self.secure = true;
                self.wire.switching = false;
                if self.step == Step::StartTls {
                    // The server forgets what it offered: asked again.
                    self.say(io, events, Step::SecureEhlo, &["EHLO localhost"])?;
                    if self.finished {
                        return Ok(());
                    }
                }
                self.more(io, events)?;
            }
            Input::Untrusted {
                certificate,
                key,
                why,
            } => {
                self.finished = true;
                io.close();
                push(
                    events,
                    Event::Untrusted {
                        certificate,
                        key,
                        why,
                    },
                )?;
            }
            Input::Bytes(bytes) => {
                self.wire.reading = false;
                if bytes.is_empty() {
                    return self.lost(io, events);
                }
                match self.wire.buf.push(bytes) {
                    Ok(()) => self.run(io, events)?,
                    Err(Fault::NoMemory) => return Err(Fail::NoMemory),
                    Err(_) => return self.fail(io, events, Failure::Protocol),
                }
            }
            Input::Saved(_) => {}
            Input::Closed => return self.lost(io, events),
            Input::Failed(reason) => {
                self.finished = true;
                push(events, Event::Failed(failure_of(reason)))?;
            }
        }
        Ok(())
    }

    fn lost(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        self.finished = true;
        io.close();
        push(
            events,
            if self.step == Step::Quit {
                Event::Done
            } else {
                Event::Failed(Failure::Network(Reason::Lost))
            },
        )?;
        Ok(())
    }

    fn run(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        loop {
            if self.finished {
                return Ok(());
            }
            match self.wire.buf.line() {
                Err(Fault::NoMemory) => return Err(Fail::NoMemory),
                Err(_) => return self.fail(io, events, Failure::Protocol),
                Ok(None) => {
                    self.more(io, events)?;
                    return Ok(());
                }
                Ok(Some(line)) => {
                    if let Some(reply) = self.replies.line(&line)? {
                        self.reply(&reply, io, events)?;
                    }
                }
            }
        }
    }

    /// The server's words of `reply`, for a failure.
    fn refused(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        reply: &Reply,
    ) -> Result<(), Fail> {
        let text = copy(reply.text())?;
        self.fail(io, events, Failure::Refused(text))
    }

    fn login_refused(
        &mut self,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
        reply: &Reply,
    ) -> Result<(), Fail> {
        let text = copy(reply.text())?;
        self.fail(io, events, Failure::Login(text))
    }

    fn reply(
        &mut self,
        reply: &Reply,
        io: &mut dyn Io,
        events: &mut Vec<Event>,
    ) -> Result<(), Fail> {
        if reply.code == 0 {
            return self.fail(io, events, Failure::Protocol);
        }
        if reply.code == 421 {
            let text = copy(reply.text())?;
            return self.fail(io, events, Failure::Temporary(text));
        }
        match self.step {
            Step::Greeting => {
                if reply.code != 220 {
                    return self.refused(io, events, reply);
                }
                self.say(io, events, Step::Ehlo, &["EHLO localhost"])
            }
            Step::Ehlo => {
                if !reply.is_ok() {
                    return self.refused(io, events, reply);
                }
                self.extensions = Extensions::of(reply);
                if self.secure {
                    self.authenticate(io, events)
                } else if self.extensions.starttls {
                    self.say(io, events, Step::StartTls, &["STARTTLS"])
                } else {
                    // Never a password in the clear.
                    self.fail(io, events, Failure::Unsafe)
                }
            }
            Step::StartTls => {
                if reply.code != 220 {
                    return self.fail(io, events, Failure::Unsafe);
                }
                // Nothing is sent or read until the switch is done.
                self.wire.buf = LineBuffer::default();
                self.wire.reading = false;
                self.wire.switching = true;
                let started = io.start_tls(self.endpoint.pin);
                self.went(io, events, started)?;
                Ok(())
            }
            Step::SecureEhlo => {
                if !reply.is_ok() {
                    return self.refused(io, events, reply);
                }
                self.extensions = Extensions::of(reply);
                self.authenticate(io, events)
            }
            Step::AuthPlain | Step::LoginPassword => {
                if reply.code == 235 {
                    self.envelope(io, events)
                } else {
                    self.login_refused(io, events, reply)
                }
            }
            Step::LoginUser => {
                if reply.code != 334 {
                    return self.login_refused(io, events, reply);
                }
                let user = codec::base64_line(self.endpoint.user.as_bytes())?;
                self.say(io, events, Step::LoginName, &[&user])
            }
            Step::LoginName => {
                if reply.code != 334 {
                    return self.login_refused(io, events, reply);
                }
                let password = codec::base64_line(self.endpoint.password.as_str().as_bytes())?;
                let said = self.say(io, events, Step::LoginPassword, &[&password]);
                wipe_text(password);
                said
            }
            Step::MailFrom => {
                if !reply.is_ok() {
                    return self.refused(io, events, reply);
                }
                self.rcpt(0, io, events)
            }
            Step::Rcpt(n) => {
                if !reply.is_ok() {
                    let who = match self.message.to.get(n) {
                        Some(who) => copy(who)?,
                        None => String::new(),
                    };
                    let text = copy(reply.text())?;
                    let failure = if reply.is_temporary() {
                        Failure::Temporary(text)
                    } else {
                        Failure::Recipient(who, text)
                    };
                    return self.fail(io, events, failure);
                }
                self.rcpt(n + 1, io, events)
            }
            Step::Data => {
                if reply.code != 354 {
                    return self.refused(io, events, reply);
                }
                push(events, Event::Stage(Stage::Sending))?;
                self.step = Step::Body;
                let sent = io.send_message(core::mem::take(&mut self.message.data));
                self.went(io, events, sent)?;
                Ok(())
            }
            Step::Body => {
                if !reply.is_ok() {
                    let text = copy(reply.text())?;
                    let failure = if reply.is_temporary() {
                        Failure::Temporary(text)
                    } else {
                        Failure::Refused(text)
                    };
                    return self.fail(io, events, failure);
                }
                self.say(io, events, Step::Quit, &["QUIT"])
            }
            Step::Quit => {
                self.finished = true;
                io.close();
                push(events, Event::Done)?;
                Ok(())
            }
        }
    }

    fn authenticate(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        push(events, Event::Stage(Stage::LoggingIn))?;
        if self.extensions.auth_plain {
            let line = smtp::auth_plain(&self.endpoint.user, self.endpoint.password.as_str())?;
            self.step = Step::AuthPlain;
            let sent = io.send(line.as_str().as_bytes());
            drop(line);
            self.went(io, events, sent)?;
            Ok(())
        } else if self.extensions.auth_login {
            self.say(io, events, Step::LoginUser, &["AUTH LOGIN"])
        } else {
            self.fail(io, events, Failure::Unsafe)
        }
    }

    fn envelope(&mut self, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        if self.extensions.size > 0 && self.message.data.len() as u64 > self.extensions.size {
            let text = copy("the message is larger than the server takes")?;
            return self.fail(io, events, Failure::Refused(text));
        }
        let line = smtp::mail_from(&self.message.from)?;
        self.step = Step::MailFrom;
        let sent = io.send(line.as_bytes());
        self.went(io, events, sent)?;
        Ok(())
    }

    fn rcpt(&mut self, n: usize, io: &mut dyn Io, events: &mut Vec<Event>) -> Result<(), Fail> {
        match self.message.to.get(n) {
            Some(address) => {
                let line = smtp::rcpt_to(address)?;
                self.step = Step::Rcpt(n);
                let sent = io.send(line.as_bytes());
                self.went(io, events, sent)?;
                Ok(())
            }
            None => self.say(io, events, Step::Data, &["DATA"]),
        }
    }
}
