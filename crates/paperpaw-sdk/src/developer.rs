//! The developer link over USB (see
//! [ADR 0031](../../../docs/adr/0031-developer-link.md)): a plugin of
//! developer tools that the reader installed brings what test builds do
//! over USB (files in and out, presses, the screen, the memory, timings)
//! to released firmware. The firmware itself stays without it.
//!
//! A plugin that declares [`Uses::DEVELOPER`](crate::Uses::DEVELOPER)
//! opens the link with [`open`] and hears everything on the event it
//! names, each a [`DevEvent`]: opened, bytes from the computer, the
//! screen it asked for, closed, failed. It writes with [`write`](fn@write), asks
//! for the screen with [`screen`] and closes with [`close`]. Presses,
//! actions, files and the memory are the plugin's as for any other
//! (`Command::Input`, `Command::RunAction`, `Env::storage`,
//! `Env::memory`), so the tools need no commands of their own for them.
//!
//! The kernel keeps it safe, not the plugin: the link opens only while
//! the reader allowed it under *Developer*, confirmed on the device; the
//! status line shows it while it is open; a restart ends it unless the
//! reader kept it on; secrets are never readable over it; the firmware's
//! own log and the update path go first on the wire.
//!
//! In this release the link is reserved, not carried out: every [`open`]
//! is answered [`DevError::NotAvailable`]. A later firmware fills it in
//! without a new plugin interface; the requests and events have room for
//! more ([`DevCommand`] and [`DevEvent`] are not exhaustive).

use alloc::vec::Vec;

use crate::command::Command;
use crate::event::EventData;
use crate::id::Id;
use crate::params::Params;

/// What a developer plugin asks of the link ([`open`] and the others).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DevCommand {
    /// Opens link `link`, heard as `events` (each a [`DevEvent`]).
    Open { link: Id, events: Id },
    /// Sends bytes to the computer.
    Write { link: Id, data: Vec<u8> },
    /// The frame shown now, sent as [`DevEvent::Screen`].
    Screen { link: Id },
    /// Closes it.
    Close { link: Id },
}

/// A request on its way to the kernel, with the plugin that sent it
/// (filled in by the kernel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevRequest {
    command: DevCommand,
    from: Option<Id>,
}

#[cfg(feature = "kernel")]
impl DevRequest {
    pub fn command(&self) -> &DevCommand {
        &self.command
    }

    /// Says that plugin `plugin` sent it.
    pub fn set_from(&mut self, plugin: Id) {
        self.from = Some(plugin);
    }

    pub fn into_parts(self) -> (DevCommand, Option<Id>) {
        (self.command, self.from)
    }
}

fn request(command: DevCommand) -> Command {
    Command::Developer(alloc::boxed::Box::new(DevRequest {
        command,
        from: None,
    }))
}

/// Opens the link `link`, heard as `events`.
pub fn open(link: Id, events: Id) -> Command {
    request(DevCommand::Open { link, events })
}

/// Sends `data` to the computer over `link`.
pub fn write(link: Id, data: Vec<u8>) -> Command {
    request(DevCommand::Write { link, data })
}

/// Asks for the frame shown now ([`DevEvent::Screen`]).
pub fn screen(link: Id) -> Command {
    request(DevCommand::Screen { link })
}

/// Closes `link`.
pub fn close(link: Id) -> Command {
    request(DevCommand::Close { link })
}

/// Why a link closed ([`DevEvent::Closed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DevClose {
    /// The plugin closed it.
    Asked,
    /// The cable was pulled, or the computer let go.
    Unplugged,
    /// The reader switched developer tools off.
    SwitchedOff,
}

/// Why a request failed ([`DevEvent::Failed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DevError {
    /// This firmware has no developer link (yet), or no USB.
    NotAvailable,
    /// The plugin does not declare [`Uses::DEVELOPER`](crate::Uses::DEVELOPER).
    NotDeclared,
    /// The reader has not allowed developer tools.
    NotAllowed,
    /// Another plugin has the link.
    Busy,
    /// No such link is open.
    NotOpen,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

/// What happened on a link, heard on the event named in [`open`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DevEvent {
    Opened {
        link: Id,
    },
    /// Bytes from the computer.
    Received {
        link: Id,
        data: Vec<u8>,
    },
    /// The frame shown: `width` × `height` pixels, a byte for each
    /// (0 black, 255 white), row after row.
    Screen {
        link: Id,
        width: u16,
        height: u16,
        data: Vec<u8>,
    },
    Closed {
        link: Id,
        why: DevClose,
    },
    Failed {
        link: Id,
        error: DevError,
    },
}

impl DevEvent {
    /// The link it is about.
    pub fn link(&self) -> &Id {
        match self {
            DevEvent::Opened { link }
            | DevEvent::Received { link, .. }
            | DevEvent::Screen { link, .. }
            | DevEvent::Closed { link, .. }
            | DevEvent::Failed { link, .. } => link,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            DevEvent::Opened { .. } => "opened",
            DevEvent::Received { .. } => "received",
            DevEvent::Screen { .. } => "screen",
            DevEvent::Closed { .. } => "closed",
            DevEvent::Failed { .. } => "failed",
        }
    }
}

/// Written for observers and rules as its kind and link, never what went
/// over it; heard whole by the plugin it is for, carried as it is.
impl EventData for DevEvent {
    fn to_params(&self) -> Params {
        Params::new().with("kind", self.kind()).with(
            crate::builtin::params::ID,
            alloc::string::String::from(self.link().as_str()),
        )
    }

    fn from_params(params: &Params) -> Option<Self> {
        params.carried::<DevEvent>().cloned()
    }

    fn into_params(self) -> Params {
        self.to_params().carrying(self)
    }
}
