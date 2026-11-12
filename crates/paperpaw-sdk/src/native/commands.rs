//! Typed command submission using caller-owned scratch storage.
use super::{
    Error, Host,
    binding::status,
    settings::SettingValue,
    wire::{commands_wire as wire, settings_wire::SettingValueRef},
};
use core::fmt;
use paperpaw_native::host_glue::PpBytes;

mod bluetooth;
mod device;
mod evidence;
mod image;
mod item_state;
mod lookup;
#[cfg(test)]
#[path = "commands/menu_alloc_tests.rs"]
pub(crate) mod menu_alloc_tests;
mod registry;
mod uix;
pub use device::{CheckKind, FirmwareOptions, InstallRoot, PowerAction};
pub use evidence::{Evidence, MAX_REVOCATIONS, MAX_ROTATIONS, Revocation, Rotation};
pub use item_state::ItemState;
pub use registry::{
    KeyBinding, PluginEnabled, PluginOutcome, PresetMode, PreviewSetting, ResetTarget,
};

/// Refresh requested after redrawing the submitting plugin's top screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RedrawMode {
    /// Redraw without requesting a display refresh.
    RedrawOnly,
    /// Request the fastest refresh.
    Fast,
    /// Request a partial refresh.
    Partial,
    /// Request a full refresh.
    Full,
}

impl RedrawMode {
    const fn wire(self) -> u32 {
        match self {
            Self::RedrawOnly => 0,
            Self::Fast => 1,
            Self::Partial => 2,
            Self::Full => 3,
        }
    }
}

/// Submit typed commands using caller-owned transport scratch. Menu replacement
/// temporarily allocates its joined list fallibly and releases it before submit.
/// Each method stages its batch immediately. Dropping sends nothing, and an
/// error does not prevent later calls. The host applies accepted commands after
/// the callback returns, in submission order.
///
/// The sender cannot retain local scratch beyond its scope:
///
/// ```compile_fail
/// use paperpaw_sdk::native::{Host, commands::CommandSender};
/// fn escape<'cb>(host: &'cb Host<'cb>) -> CommandSender<'cb, 'static> {
///     let mut scratch = [0; 128];
///     CommandSender::new(host, &mut scratch)
/// }
/// ```
pub struct CommandSender<'cb, 'buf> {
    host: &'cb Host<'cb>,
    scratch: &'buf mut [u8],
}

impl fmt::Debug for CommandSender<'_, '_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandSender")
            .field("capacity", &self.scratch.len())
            .finish_non_exhaustive()
    }
}

impl<'cb, 'buf> CommandSender<'cb, 'buf> {
    /// Bind a callback's host and reusable command encoding buffer.
    pub fn new(host: &'cb Host<'cb>, scratch: &'buf mut [u8]) -> Self {
        Self { host, scratch }
    }

    /// Stage one setting change. The host validates setting ownership, kind and
    /// authority before accepting the batch. No command is submitted on a local
    /// error; earlier accepted batches remain staged.
    ///
    /// # Errors
    /// Returns Denied for `core.grant.*`, Invalid for absent values or an empty
    /// key, NoSpace for encoded limits or short scratch, or the host's refusal.
    pub fn set_setting(
        &mut self,
        key: impl AsRef<str>,
        value: SettingValue<'_>,
    ) -> Result<(), Error> {
        let key = key.as_ref();
        if key.starts_with("core.grant.") {
            return Err(Error::Denied);
        }
        let value = match value {
            SettingValue::Absent => SettingValueRef::Absent,
            SettingValue::Bool(value) => SettingValueRef::Bool(value),
            SettingValue::Int(value) => SettingValueRef::Int(value),
            SettingValue::Text(value) => SettingValueRef::Text(value),
        };
        self.submit(wire::Command::SetSetting { key, value })
    }

    /// Stage opening a registered screen, including another plugin's screen.
    ///
    /// # Errors
    /// Returns Invalid for an empty or overlong id, NoSpace for short scratch,
    /// or the host's refusal, including NotFound for an unregistered screen.
    pub fn open_screen(&mut self, screen: impl AsRef<str>) -> Result<(), Error> {
        let screen = screen.as_ref();
        check_id(screen, wire::COMMAND_MAX_SCREEN_BYTES)?;
        self.submit(wire::Command::OpenScreen { screen })
    }

    /// Stage running a registered action, including another plugin's action.
    ///
    /// # Errors
    /// Returns Invalid for an empty or overlong id, NoSpace for short scratch,
    /// or the host's refusal, including NotFound for an unregistered action.
    pub fn run_action(&mut self, action: impl AsRef<str>) -> Result<(), Error> {
        let action = action.as_ref();
        check_id(action, wire::COMMAND_MAX_ACTION_BYTES)?;
        self.submit(wire::Command::RunAction { action })
    }

    /// Stage a redraw and refresh request for this plugin's top screen.
    ///
    /// # Errors
    /// Returns NoSpace for short scratch or the host's refusal, including State
    /// when the submitting plugin does not own the screen on top.
    pub fn queue_redraw(&mut self, mode: RedrawMode) -> Result<(), Error> {
        self.submit(wire::Command::Redraw { mode: mode.wire() })
    }

    /// Stage a redraw and refresh request for part of this plugin's top
    /// screen: `area` from the top-left corner of the screen's region (the
    /// origin of [`DrawFrame::rect`], whatever bars show), clipped to
    /// the region. Several in one callback are drawn each on its own, so a
    /// few pictures on a page refresh as small areas, not as a page turn;
    /// the modes merge to the strongest. An empty area redraws nothing.
    ///
    /// Asked for with a whole [`queue_redraw`](Self::queue_redraw) in the
    /// same callback, the areas are drawn in the frame after: a page's text
    /// first, its pictures right after. That frame's draws have
    /// [`DrawFrame::id`](super::callbacks::DrawFrame::id) bit 0 set.
    ///
    /// [`DrawFrame::rect`]: super::callbacks::DrawFrame::rect
    ///
    /// # Errors
    /// Returns NoSpace for short scratch or the host's refusal, including State
    /// when the submitting plugin does not own the screen on top.
    pub fn queue_redraw_area(
        &mut self,
        mode: RedrawMode,
        area: super::callbacks::FrameRect,
    ) -> Result<(), Error> {
        self.submit(wire::Command::RedrawArea {
            mode: mode.wire(),
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height,
        })
    }

    /// Stage closing this plugin's screen on top.
    ///
    /// # Errors
    /// Returns NoSpace for short scratch or the host's refusal, including State
    /// when the submitting plugin does not own the screen on top.
    pub fn close_screen(&mut self) -> Result<(), Error> {
        self.submit(wire::Command::CloseScreen)
    }

    fn submit(&mut self, command: wire::Command<'_>) -> Result<(), Error> {
        self.submit_batch(&[command])
    }

    fn submit_batch(&mut self, commands: &[wire::Command<'_>]) -> Result<(), Error> {
        let length = self.encode_batch(commands)?;
        self.submit_encoded(length)
    }

    fn encode_batch(&mut self, commands: &[wire::Command<'_>]) -> Result<usize, Error> {
        wire::commands_encode(commands, self.scratch).map_err(Error::from_status)
    }

    fn submit_encoded(&mut self, length: usize) -> Result<(), Error> {
        // SAFETY: the host binding and initialized scratch prefix live for this
        // synchronous call; the host copies any command data it retains.
        status(unsafe {
            (self.host.table.commands_submit)(
                self.host.ctx,
                PpBytes {
                    ptr: self.scratch.as_ptr(),
                    len: length,
                },
            )
        })
    }
}

fn check_id(id: &str, maximum: usize) -> Result<(), Error> {
    if id.is_empty() || id.len() > maximum {
        Err(Error::Invalid)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
