//! Live event delivery through the kernel's existing plugin subscriptions.
//!
//! The shell retains the pinned owner, committed registrations and executable
//! module. Only host-owned Rust objects use this adapter; the foreign callback
//! still receives one bounded byte batch, never an SDK object.

use super::shared::{self, Shared};
use super::{DispatchDepth, Error, FlatHost, environment::EnvironmentScope, events};
use alloc::{boxed::Box, format, rc::Rc, string::String};
use core::{fmt, pin::Pin};
use paperpaw_sdk::{Env, Id, Manifest, Params, Plugin, RegisterError, Registrar, Version};

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
    /// registrations and their indices must not change after `FlatPlugin::new`.
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

/// Host adapter for events 32–37. Shells return this through CodeLoader.
/// The shared depth must be the same for every flat instance on this device.
#[non_exhaustive]
pub struct FlatPlugin {
    manifest: Manifest,
    instance: Shared<Box<dyn FlatInstance>>,
    depth: FlatDepth,
    host: FlatHost,
}

impl fmt::Debug for FlatPlugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FlatPlugin")
            .field("manifest", &self.manifest)
            .finish_non_exhaustive()
    }
}

impl FlatPlugin {
    #[cfg(test)]
    pub(crate) fn reference_counts(&self) -> (u32, u32) {
        (self.instance.strong_count(), self.depth.0.strong_count())
    }

    /// Copy loader metadata after make with checked allocation only.
    ///
    /// # Errors
    /// Returns Allocation if the owned id or name cannot be copied.
    pub fn try_manifest(id: &Id, name: &str, version: Version) -> Result<Manifest, FlatError> {
        let id = id.try_clone().ok_or(FlatError::Allocation)?;
        let mut owned_name = String::new();
        owned_name
            .try_reserve_exact(name.len())
            .map_err(|_| FlatError::Allocation)?;
        owned_name.push_str(name);
        // Str adopts a boxed string. Refuse unexpected spare capacity rather
        // than letting that adoption attempt an unchecked shrink allocation.
        if owned_name.capacity() != owned_name.len() {
            return Err(FlatError::Allocation);
        }
        Ok(Manifest::new(id, owned_name, version))
    }

    /// Retain a shell-owned instance. Allocations occur only during construction
    /// and subscription registration, never during event encoding or dispatch.
    /// The instance Box and its private shared node cost two allocations per
    /// plugin. The device depth uses one shared allocation for all plugins.
    ///
    /// # Errors
    /// Refuses the entire instance when a committed callback cannot be routed.
    pub fn new(
        manifest: Manifest,
        instance: Box<dyn FlatInstance>,
        depth: FlatDepth,
        host: &FlatHost,
    ) -> Result<Self, FlatError> {
        let mut error = None;
        instance.registrations(&mut |registration| {
            let tag = match registration {
                FlatRegistration::Event { event_tag, .. } if event_id(event_tag).is_some() => {
                    return;
                }
                FlatRegistration::Event { event_tag, .. } => event_tag,
                FlatRegistration::Other { record_tag } => record_tag,
            };
            error.get_or_insert(FlatError::Unrouted { tag });
        });
        if let Some(error) = error {
            return Err(error);
        }
        Ok(Self {
            manifest,
            instance: Shared::try_new(instance).map_err(|_| FlatError::Allocation)?,
            depth,
            host: FlatHost {
                meter: Rc::clone(&host.meter),
                directory: host.try_directory().map_err(|_| FlatError::Allocation)?,
                stack_rows: Rc::clone(&host.stack_rows),
                font_removals: Rc::clone(&host.font_removals),
                registry_generation: Rc::clone(&host.registry_generation),
                log: host.log,
            },
        })
    }
}

impl Plugin for FlatPlugin {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

    /// The kernel discards the entire registrar on error, including subscriptions
    /// added before a later subscription failed. Contributions are staged only
    /// after registration succeeds.
    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
        let mut result = Ok(());
        self.instance.registrations(&mut |registration| {
            let FlatRegistration::Event {
                event_tag: tag,
                index,
            } = registration
            else {
                result = Err(RegisterError::Invalid {
                    message: "flat registrations changed after construction".into(),
                });
                return;
            };
            if result.is_err() {
                return;
            }
            let Some(event) = event_id(tag) else {
                result = Err(RegisterError::Invalid {
                    message: format!("flat callback tag {tag} has no live dispatcher").into(),
                });
                return;
            };
            let Some(instance) = self.instance.try_clone() else {
                result = Err(RegisterError::Invalid {
                    message: "flat instance reference limit".into(),
                });
                return;
            };
            let Some(depth) = self.depth.try_clone() else {
                result = Err(RegisterError::Invalid {
                    message: "flat depth reference limit".into(),
                });
                return;
            };
            let directory = match self.host.try_directory() {
                Ok(directory) => directory,
                Err(_) => {
                    result = Err(RegisterError::Invalid {
                        message: "flat directory reference limit".into(),
                    });
                    return;
                }
            };
            let host = FlatHost {
                meter: Rc::clone(&self.host.meter),
                directory,
                stack_rows: Rc::clone(&self.host.stack_rows),
                font_removals: Rc::clone(&self.host.font_removals),
                registry_generation: Rc::clone(&self.host.registry_generation),
                log: self.host.log,
            };
            let handler: Option<paperpaw_sdk::EventHandler> = shared::try_box(
                move |env: &Env<'_>, params: &Params, _: &mut paperpaw_sdk::Commands| {
                    deliver(
                        &**instance,
                        depth.as_depth(),
                        &host,
                        tag,
                        index,
                        env,
                        params,
                    )
                },
            )
            .map(|handler| handler as paperpaw_sdk::EventHandler);
            let Some(handler) = handler else {
                result = Err(RegisterError::Invalid {
                    message: "flat handler allocation failed".into(),
                });
                return;
            };
            result = registrar.subscribe(event, handler);
        });
        result
    }
}

fn event_id(tag: u32) -> Option<Id> {
    use super::registrations::*;
    use paperpaw_sdk::builtin::events as builtin;
    Some(match tag {
        EVENT_STARTED_TAG => builtin::STARTED.id().clone(),
        EVENT_RESUME_TAG => builtin::RESUME.id().clone(),
        EVENT_SUSPEND_TAG => builtin::SUSPEND.id().clone(),
        EVENT_STOPPING_TAG => builtin::STOPPING.id().clone(),
        EVENT_BATTERY_MEASURED_TAG => builtin::BATTERY_MEASURED.id().clone(),
        EVENT_PAGE_TURNED_TAG => builtin::PAGE_TURNED.id().clone(),
        EVENT_TICK_TAG => super::native_tick::TICK.id().clone(),
        _ => return None,
    })
}

fn deliver(
    instance: &dyn FlatInstance,
    depth: &DispatchDepth,
    host: &FlatHost,
    tag: u32,
    index: usize,
    env: &Env<'_>,
    params: &Params,
) {
    // Only the page-turn path needs the larger buffer. Lifecycle events keep
    // their stack usage independent of the page strings' schema caps.
    let result = if tag == super::registrations::EVENT_PAGE_TURNED_TAG {
        page::deliver(instance, depth, host, index, env, params)
    } else {
        let mut bytes = [0; 12];
        encode_scalar(tag, env.now().millis(), &mut bytes)
            .map_err(|_| DropReason::Producer)
            .and_then(|len| invoke(instance, depth, host, index, env, &bytes[..len]))
    };
    if let Err(why) = result {
        instance.event_dropped(tag, why);
    }
}

fn encode_scalar(tag: u32, now: u64, bytes: &mut [u8]) -> Result<usize, i32> {
    use super::registrations::*;
    match tag {
        EVENT_STARTED_TAG => events::encode_started(now, bytes),
        EVENT_RESUME_TAG => events::encode_resume(now, bytes),
        EVENT_SUSPEND_TAG => events::encode_suspend(now, bytes),
        EVENT_STOPPING_TAG => events::encode_stopping(now, bytes),
        EVENT_BATTERY_MEASURED_TAG => events::encode_battery_measured(now, bytes),
        EVENT_TICK_TAG => tick_encode(now, bytes),
        _ => Err(paperpaw_native::host_glue::ERR_UNSUPPORTED),
    }
}

fn admission(error: Error) -> DropReason {
    match error {
        Error::Busy | Error::Depth => DropReason::Busy,
        _ => DropReason::State,
    }
}

fn invoke(
    instance: &dyn FlatInstance,
    depth: &DispatchDepth,
    host: &FlatHost,
    index: usize,
    env: &Env<'_>,
    bytes: &[u8],
) -> Result<(), DropReason> {
    let directory = host.upgrade_directory().map_err(DropReason::Plugin)?;
    let frame = crate::abi7::core_frame::CoreFrame {
        callback: None,
        instance: None,
        env,
        meter: &host.meter,
        depth: None,
        directory: directory.as_deref(),
        stack_generation: host.stack_rows.generation(),
        stack_rows: &host.stack_rows,
        font_removals: &host.font_removals,
        registry_generation: &host.registry_generation,
        log: host.log,
        usage: env.usage(),
        plugins: env.plugins(),
        work: env.work(),
        ui: None,
    };
    let status = instance
        .environment()
        .scope_core(&frame, || {
            instance.dispatch_event(index, bytes, &mut [], depth)
        })
        .map_err(admission)?
        .map_err(admission)?;
    reply_length(status, 0).map(|_| ())
}

// The caller checks the length before framing or reading any reply bytes.
fn reply_length(status: i32, capacity: usize) -> Result<usize, DropReason> {
    let length = usize::try_from(status).map_err(|_| DropReason::Plugin(status))?;
    if length > capacity {
        return Err(DropReason::Plugin(paperpaw_native::host_glue::ERR_INVALID));
    }
    Ok(length)
}

#[cfg(test)]
#[path = "flat/routing_tests.rs"]
mod routing_tests;

#[cfg(test)]
#[path = "flat/test_loader.rs"]
pub(crate) mod test_loader;
