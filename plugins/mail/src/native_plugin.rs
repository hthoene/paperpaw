//! The interface-7 Mail: its settings and events are declared to the host, and
//! the connection to the servers runs here (slice 6): one [`Job`](crate::native_model::job::Job) with the
//! socket of the host, the account read from the settings and the password
//! from `core.secrets` when a connection is made. The screens, the lists and the
//! widgets follow (see `PORTING-7.md`); the interface-6 plugin is still in place
//! beside this one.
use crate::declarations::SOCKET_TOKEN;
use crate::native_model::{
    account::Account,
    job::{Job, Run},
    link::Notice,
    mem::Error as MailError,
    session::{Outgoing, Task},
};
use crate::native_socket::HostNet;
use crate::native_values::HostValues;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{Event, Incoming, kinds},
    registrations,
    reply::Reply,
    secrets::Secrets,
    sockets::{SocketId, Sockets},
    storage::DataStorage,
};
use paperpaw_ui_kit::read_event;

mod image {
    include!(concat!(env!("OUT_DIR"), "/mail_declarations.rs"));
}

/// Mail's interface-7 state: the services, and the one connection there is.
#[derive(Debug, Default)]
pub struct NativeMail {
    job: Job,
    /// The socket of the connection, while there is one.
    socket: Option<SocketId>,
    /// The name of the server connected to (for the secure switch).
    server: String,
    sockets: Option<Sockets>,
    secrets: Option<Secrets>,
}

impl Plugin for NativeMail {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::EVENTS) {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::Socket) else {
            return Ok(());
        };
        match event {
            Event::Socket(socket) if token == SOCKET_TOKEN => {
                if self.socket != Some(socket.id()) {
                    // The socket of a connection that is over: nothing of ours.
                    return Ok(());
                }
                let notice = Notice {
                    kind: socket.kind(),
                    reason: socket.reason(),
                    len: socket.len(),
                };
                self.notice(host, notice)
            }
            _ => Ok(()),
        }
    }
}

impl NativeMail {
    /// How the connection stands.
    #[must_use]
    pub const fn run(&self) -> &Run {
        &self.job.run
    }

    /// Whether a connection is going.
    #[must_use]
    pub fn working(&self) -> bool {
        self.job.working()
    }

    /// The services, opened when first needed (a refusal is tried again).
    fn services(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.sockets.is_none() {
            self.sockets = Some(Sockets::open(host)?);
        }
        if self.secrets.is_none() {
            self.secrets = Some(Secrets::open(host)?);
        }
        Ok(())
    }

    /// Account `number`, with the password read for the connection and wiped
    /// after it: its IMAP endpoint.
    fn imap_of(
        &mut self,
        host: &Host<'_>,
        number: u8,
    ) -> Result<(crate::native_model::session::Endpoint, String), Error> {
        self.services(host)?;
        let secrets = self.secrets.as_ref().ok_or(Error::State)?;
        let values = HostValues { host, secrets };
        let account = Account::read(&values, number)?.ok_or(Error::NotFound)?;
        let password = account.password(&values)?;
        let name = crate::native_model::mem::copy(&account.imap.host)?;
        Ok((account.imap.endpoint(&account.user, password)?, name))
    }

    /// Starts `tasks` on the IMAP server of account `number`.
    ///
    /// # Errors
    /// `NotFound` where the account is not set up; `Busy` while a connection is
    /// open; `Unsupported` where the device keeps no secrets; the host's
    /// refusals; `NoMemory`.
    pub fn start_imap(
        &mut self,
        host: &Host<'_>,
        number: u8,
        tasks: Vec<Task>,
    ) -> Result<(), Error> {
        if self.job.working() {
            return Err(MailError::Busy);
        }
        let (endpoint, name) = self.imap_of(host, number)?;
        self.server = name;
        let sockets = self.sockets.as_ref().ok_or(Error::State)?;
        let net = HostNet::new(host, sockets, &mut self.socket, SOCKET_TOKEN, &self.server)?;
        self.job.start_imap(net, endpoint, tasks)
    }

    /// Sends `message` through the SMTP server of account `number`.
    ///
    /// # Errors
    /// As [`NativeMail::start_imap`].
    pub fn start_smtp(
        &mut self,
        host: &Host<'_>,
        number: u8,
        message: Outgoing,
    ) -> Result<(), Error> {
        if self.job.working() {
            return Err(MailError::Busy);
        }
        self.services(host)?;
        let secrets = self.secrets.as_ref().ok_or(Error::State)?;
        let values = HostValues { host, secrets };
        let account = Account::read(&values, number)?.ok_or(Error::NotFound)?;
        let password = account.password(&values)?;
        self.server = crate::native_model::mem::copy(&account.smtp.host)?;
        let endpoint = account.smtp.endpoint(&account.user, password)?;
        let sockets = self.sockets.as_ref().ok_or(Error::State)?;
        let net = HostNet::new(host, sockets, &mut self.socket, SOCKET_TOKEN, &self.server)?;
        self.job.start_smtp(net, endpoint, message)
    }

    /// A notice of the connection's socket.
    fn notice(&mut self, host: &Host<'_>, notice: Notice) -> Result<(), Error> {
        let sockets = self.sockets.as_ref().ok_or(Error::State)?;
        let storage = DataStorage::new(host);
        let net = HostNet::new(host, sockets, &mut self.socket, SOCKET_TOKEN, &self.server)?;
        let result = self.job.notice(net, notice, &storage);
        if !self.job.working() {
            // The connection is over; its socket is closed or ended.
            self.socket = None;
        }
        result
    }
}
