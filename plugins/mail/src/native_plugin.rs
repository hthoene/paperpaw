//! The interface-7 Mail: its settings and events are declared to the host, and
//! the connection to the servers runs here (slice 6): one [`Job`](crate::native_model::job::Job) with the
//! socket of the host, the account read from the settings and the password
//! from `core.secrets` when a connection is made. The screens, the lists and the
//! widgets follow (see `PORTING-7.md`); the interface-6 plugin is still in place
//! beside this one.
use crate::declarations::{OPEN_TOKEN, SETTING_TOKEN, SOCKET_TOKEN, TICK_TOKEN, WRITE_TOKEN};
use crate::native_model::{
    account::Account,
    job::{Job, Run},
    link::Notice,
    mem::Error as MailError,
    session::{Outgoing, Task},
};
use crate::native_provider as provider;
use crate::native_reader::{READ_TOKEN, Reading};
use crate::native_screens::{Asked, BOXES_TOKEN, Check, LIST_TOKEN, View};
use crate::native_socket::HostNet;
use crate::native_values::{HostValues, SettingsValues};
use crate::native_widgets::{Tap, Widgets};
use crate::native_writer::{COMPOSE_TOKEN, Writing};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::CommandSender,
        events::{Event, Incoming, kinds},
        registrations,
        reply::Reply,
        secrets::Secrets,
        settings::SettingValue,
        sockets::{SocketId, Sockets},
        storage::DataStorage,
        ui::{Instance, Open, Ui},
    },
};
use paperpaw_ui_kit::{Screens, read_event};

mod image {
    include!(concat!(env!("OUT_DIR"), "/mail_declarations.rs"));
}

/// Mail's interface-7 state: the services, and the one connection there is.
#[derive(Debug, Default)]
pub struct NativeMail {
    pub(crate) job: Job,
    /// The socket of the connection, while there is one.
    pub(crate) socket: Option<SocketId>,
    /// The name of the server connected to (for the secure switch).
    pub(crate) server: String,
    pub(crate) sockets: Option<Sockets>,
    pub(crate) secrets: Option<Secrets>,
    pub(crate) ui: Option<Ui>,
    /// The open list screens.
    pub(crate) screens: Screens<View>,
    /// The open readers.
    pub(crate) readers: Screens<Reading>,
    /// The open compose windows.
    pub(crate) writers: Screens<Writing>,
    /// The check that is going, if one is.
    pub(crate) check: Option<Check>,
    /// The question *Trust this server?* that is open.
    pub(crate) asked: Option<Asked>,
    /// The account whose server the reader trusted, and the screen to check
    /// again from when it shows.
    pub(crate) pending: Option<(u8, Instance)>,
    /// The home widgets made for screens.
    pub(crate) widgets: Widgets,
}

impl Plugin for NativeMail {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::UI)
            .chain(image::SCREENS)
            .chain(image::WIDGETS)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::Socket, kinds::ActionRun, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::Answered, kinds::SettingChanged, kinds::Tick, kinds::WidgetMade, kinds::WidgetDropped, kinds::StageCall)
        else {
            return Ok(());
        };
        match event {
            Event::WidgetMade {
                widget, instance, ..
            } => self.widgets.made(widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.widgets.dropped(instance);
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.widgets.stage(host, payload, reply),
            Event::Tick { .. } if token == TICK_TOKEN => {
                self.widgets.tick(host);
                Ok(())
            }
            Event::SettingChanged { key, .. } if token == SETTING_TOKEN => {
                self.setting_changed(host, key)
            }
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
            Event::ActionRun {
                action: "mail.open",
                ..
            } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("mail.boxes")?)
            }
            Event::ActionRun {
                action: "mail.write",
                ..
            } if token == WRITE_TOKEN => self.open_compose(host, 0, None),
            Event::ScreenOpened {
                instance, handover, ..
            } if token == BOXES_TOKEN || token == LIST_TOKEN => {
                self.screen_opened(host, token, instance, handover)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == BOXES_TOKEN || token == LIST_TOKEN => {
                self.screen_shown(host, token, instance)
            }
            Event::ScreenOpened {
                instance, handover, ..
            } if token == READ_TOKEN => self.reader_opened(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == READ_TOKEN => self.reader_shown(host, instance),
            Event::ScreenOpened {
                instance, handover, ..
            } if token == COMPOSE_TOKEN => self.writer_opened(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == COMPOSE_TOKEN => self.writer_shown(instance),
            Event::ScreenClosed { instance, .. } => {
                self.screen_closed(host, instance);
                self.reader_closed(host, instance);
                self.writer_closed(host, instance);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, frame: DrawFrame<'_>) -> Result<(), Error> {
        if frame.purpose == DrawPurpose::Widget {
            return self.widgets.draw(host, frame);
        }
        if token == READ_TOKEN {
            return self.draw_reader(host, frame);
        }
        if token == COMPOSE_TOKEN {
            return self.draw_writer(host, frame);
        }
        self.draw_list(host, token, frame)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        // A tap on a home widget opens the folder, the folders or a new mail.
        if let Some(tap) = self.widgets.tapped(host, &input)? {
            self.tapped(host, tap)?;
            return Ok(Handled::Yes);
        }
        if token == READ_TOKEN {
            return self.input_reader(host, &input);
        }
        if token == COMPOSE_TOKEN {
            return self.input_writer(host, &input);
        }
        if token != BOXES_TOKEN && token != LIST_TOKEN {
            return Ok(Handled::No);
        }
        self.input_list(host, token, &input)
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
    pub(crate) fn services(&mut self, host: &Host<'_>) -> Result<(), Error> {
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
        if !self.job.working() {
            // What the card holds changed: the widgets show it.
            self.widgets.refresh(host);
        }
        result?;
        self.progressed(host)
    }

    /// What a tap on a home widget opens.
    fn tapped(&mut self, host: &Host<'_>, tap: Tap) -> Result<(), Error> {
        match tap {
            Tap::Write => self.open_compose(host, 0, None),
            Tap::Folder(account, folder) => {
                self.ensure_ui(host)?;
                self.open_folder(host, account, &folder)
            }
            Tap::Boxes => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("mail.boxes")?)
            }
        }
    }

    /// A setting changed: the widgets show it anew, and a provider chosen (or
    /// an address a provider is known by) sets the servers.
    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key.starts_with("mail.widget.") {
            self.widgets.refresh(host);
            return Ok(());
        }
        let changes = provider::after_change(&SettingsValues(host), key)?;
        let mut scratch = [0; 256];
        for change in &changes {
            let value = match change.value {
                provider::Value::Text(text) => SettingValue::Text(text),
                provider::Value::Int(number) => SettingValue::Int(number),
            };
            CommandSender::new(host, &mut scratch).set_setting(&change.id, value)?;
        }
        Ok(())
    }

    /// Lets go of the connection: its socket is closed (with small buffers
    /// of the stack, so that a refused allocation cannot leave it open) and the
    /// job, which holds the password, is reset.
    pub(crate) fn cancel(&mut self, host: &Host<'_>) {
        if let (Some(id), Some(sockets)) = (self.socket.take(), self.sockets.as_ref()) {
            let (mut input, mut reply) = ([0u8; 128], [0u8; 128]);
            // A socket that is gone already is fine; nothing else to do here.
            sockets.close(host, id, &mut input, &mut reply).ok();
        }
        self.job.reset();
    }
}
