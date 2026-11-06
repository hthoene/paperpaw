//! Callback-scoped access to the flat host table.

use core::{ffi::c_void, fmt};
use paperpaw_native::host_glue::{self as abi, PpHost};

/// A host refusal, preserving unknown statuses for forward compatibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The host does not implement this operation.
    Unsupported,
    /// An argument is malformed.
    Invalid,
    /// A buffer or bounded request does not fit.
    NoSpace,
    /// Allocation failed.
    NoMemory,
    /// Text is not UTF-8.
    Utf8,
    /// The owner, callback or handle is no longer live.
    State,
    /// The operation was cancelled.
    Cancelled,
    /// Storage or another device operation failed.
    Io,
    /// A bounded resource is in use.
    Busy,
    /// A file, key or service was not found.
    NotFound,
    /// Storage is full or its reserve would be consumed.
    Full,
    /// A status unknown to this SDK, including an invalid positive status.
    Other(i32),
}

impl Error {
    pub(crate) const fn from_status(status: i32) -> Self {
        match status {
            abi::ERR_UNSUPPORTED => Self::Unsupported,
            abi::ERR_INVALID => Self::Invalid,
            abi::ERR_NO_SPACE => Self::NoSpace,
            abi::ERR_NO_MEMORY => Self::NoMemory,
            abi::ERR_UTF8 => Self::Utf8,
            abi::ERR_STATE => Self::State,
            abi::ERR_CANCELLED => Self::Cancelled,
            abi::ERR_IO => Self::Io,
            abi::ERR_BUSY => Self::Busy,
            abi::ERR_NOT_FOUND => Self::NotFound,
            abi::ERR_FULL => Self::Full,
            other => Self::Other(other),
        }
    }

    pub(crate) const fn status(self) -> i32 {
        match self {
            Self::Unsupported => abi::ERR_UNSUPPORTED,
            Self::Invalid => abi::ERR_INVALID,
            Self::NoSpace => abi::ERR_NO_SPACE,
            Self::NoMemory => abi::ERR_NO_MEMORY,
            Self::Utf8 => abi::ERR_UTF8,
            Self::State => abi::ERR_STATE,
            Self::Cancelled => abi::ERR_CANCELLED,
            Self::Io => abi::ERR_IO,
            Self::Busy => abi::ERR_BUSY,
            Self::NotFound => abi::ERR_NOT_FOUND,
            Self::Full => abi::ERR_FULL,
            Self::Other(other) if other < 0 => other,
            Self::Other(_) => abi::ERR_INVALID,
        }
    }
}

impl From<Error> for i32 {
    fn from(error: Error) -> Self {
        match error {
            Error::Other(code) => code,
            error => error.status(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "host status {}", self.status())
    }
}
impl core::error::Error for Error {}

pub(crate) fn status(result: i32) -> Result<(), Error> {
    if result == abi::OK {
        Ok(())
    } else {
        Err(Error::from_status(result))
    }
}

/// The host binding for one plugin callback. It cannot outlive that callback,
/// be copied, or be sent or shared between threads. Only entry glue creates it.
///
/// ```compile_fail
/// use paperpaw_sdk::native::Host;
/// fn escape<'a>(host: &'a Host<'a>) -> &'static Host<'static> { host }
/// ```
/// ```compile_fail
/// use paperpaw_sdk::native::Host;
/// fn send<T: Send>() {}
/// send::<Host<'static>>();
/// ```
/// ```compile_fail
/// use paperpaw_sdk::native::Host;
/// fn sync<T: Sync>() {}
/// sync::<Host<'static>>();
/// ```
pub struct Host<'cb> {
    pub(super) table: &'cb PpHost,
    pub(super) ctx: *mut c_void,
}

impl fmt::Debug for Host<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host").finish_non_exhaustive()
    }
}

/// A flat native plugin. The host admits callbacks exclusively and retains the
/// table and context until destruction. Use [`crate::native_plugin!`] to export it.
pub trait Plugin: Sized + 'static {
    /// Construct the instance and submit its registrations through the host.
    ///
    /// # Errors
    /// Returns a host refusal; a failed instance is never activated.
    fn make(host: &Host<'_>) -> Result<Self, Error>;

    /// Handle one borrowed event batch for a committed callback token.
    ///
    /// # Errors
    /// Returns a refusal; the host logs it and applies its delivery policy.
    fn event(&mut self, _host: &Host<'_>, _token: u32, _input: &[u8]) -> Result<(), Error> {
        Err(Error::Unsupported)
    }

    /// Draw using the canvas installed for this callback.
    ///
    /// # Errors
    /// Returns a refusal when drawing cannot complete.
    fn draw(&mut self, _host: &Host<'_>, _token: u32) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}

pub mod entry;

#[cfg(test)]
mod tests;
