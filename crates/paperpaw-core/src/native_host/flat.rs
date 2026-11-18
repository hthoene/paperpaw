//! What a shell's flat plugin instance is to the kernel: its committed
//! registrations, the device's one dispatch budget, and why an event was not
//! delivered. The events themselves go the production route
//! (`core_events.rs`); the old adapter that registered them as a Rust plugin
//! is gone (#244).

use super::shared::Shared;
use super::{DispatchDepth, Error, environment::EnvironmentScope};
use core::{fmt, pin::Pin};

#[path = "flat/page.rs"]
pub(super) mod page;

/// Why one registered event could not be delivered. The shell logs the failure.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropReason {
    /// The owner, environment scope or dispatch-depth budget was busy.
    Busy,
    /// The registration or owner was no longer live.
    State,
    /// Event parameters were missing, malformed or could not be encoded.
    Producer,
    /// The callback returned an error or a reply length beyond its output buffer.
    Plugin(i32),
}

/// A committed registration, with event tags distinct from record tags.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatRegistration {
    /// An event subscription and its index in the committed batch.
    Event { event_tag: u32, index: usize },
    /// Another callback kind, identified by its wire record tag.
    /// Declarations are applied separately and never passed as Other.
    Other { record_tag: u32 },
}

/// A valid registration has no production dispatcher in this adapter yet.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatError {
    /// An event tag or other registration record tag cannot be routed.
    Unrouted { tag: u32 },
    /// Actual host allocation failed; no instance is retained.
    Allocation,
}

impl fmt::Display for FlatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allocation => f.write_str("flat host allocation failed"),
            Self::Unrouted { tag } => write!(f, "flat callback tag {tag} has no live dispatcher"),
        }
    }
}
impl core::error::Error for FlatError {}

/// Shell-owned flat plugin instance, retained until all subscriptions are gone.
/// Implementations must keep the owner and its module alive during callbacks.
pub trait FlatInstance {
    /// Visit every committed callback registration (events, tick, draw and any
    /// later callback kind), in batch order, on every call. Declarations taken
    /// with `take_declarations` (setting specs and other data) are not visited:
    /// the shell applies them before activation, and a declaration it cannot
    /// apply refuses the plugin at load; it is never dropped. The committed
    /// registrations and their indices must not change once handed over.
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration));

    /// The real pinned environment scope used by this instance's host table.
    fn environment(&self) -> Pin<&EnvironmentScope>;

    /// Invoke the validated event callback under RegistrationBatch admission.
    /// Payload and output remain borrowed only until this call returns.
    /// The shell forwards the exclusive output slice as a PpBuffer, including
    /// its non-null dangling pointer when empty. A nonnegative result is the
    /// reply length; it must not exceed the output length.
    ///
    /// # Errors
    /// Returns the owner/admission error without accessing foreign state.
    fn dispatch_event(
        &self,
        index: usize,
        payload: &[u8],
        out: &mut [u8],
        depth: &DispatchDepth,
    ) -> Result<i32, Error>;

    /// Log a failure against the plugin. Never retry, panic or invoke plugin code.
    /// Lifecycle failures must not prevent the shell from powering off.
    fn event_dropped(&self, tag: u32, why: DropReason);
}

/// Single-threaded dispatch budget shared by all flat plugins on a device.
/// Construction and reference-count growth refuse without panic.
#[derive(Debug)]
#[must_use]
pub struct FlatDepth(Shared<DispatchDepth>);

impl FlatDepth {
    /// Allocate the device's actual shared depth counter.
    ///
    /// # Errors
    /// Returns Allocation if the actual allocation fails.
    pub fn try_new() -> Result<Self, FlatError> {
        Shared::try_new(DispatchDepth::new())
            .map(Self)
            .map_err(|_| FlatError::Allocation)
    }

    /// Borrow the same device-wide budget for direct host dispatch.
    #[must_use]
    pub fn as_depth(&self) -> &DispatchDepth {
        &self.0
    }

    /// Whether `other` is this very counter (one device has one).
    #[must_use]
    pub(crate) fn is_same_counter(&self, other: &Self) -> bool {
        core::ptr::eq(self.as_depth(), other.as_depth())
    }

    /// Retain the budget without allocating; None means count overflow.
    #[must_use]
    pub fn try_clone(&self) -> Option<Self> {
        self.0.try_clone().map(Self)
    }
}
