//! Live event delivery through the kernel's existing plugin subscriptions.
//!
//! The shell retains the pinned owner, committed registrations and executable
//! module. Only host-owned Rust objects use this adapter; the foreign callback
//! still receives one bounded byte batch, never an SDK object.

use super::{DispatchDepth, Error, FlatHost, environment::EnvironmentScope, events};
use alloc::{boxed::Box, format, rc::Rc};
use core::{fmt, pin::Pin};
use paperpaw_sdk::{Env, Id, Manifest, Params, Plugin, RegisterError, Registrar};

#[path = "flat/page.rs"]
mod page;

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
    /// The callback returned something other than OK, including invalid positives.
    Plugin(i32),
}

/// A committed registration, with event tags distinct from record tags.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatRegistration {
    /// An event subscription and its index in the committed batch.
    Event { event_tag: u32, index: usize },
    /// Another registration kind, identified by its wire record tag.
    Other { record_tag: u32 },
}

/// A valid registration has no production dispatcher in this adapter yet.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatError {
    /// An event tag or other registration record tag cannot be routed.
    Unrouted { tag: u32 },
}

impl fmt::Display for FlatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unrouted { tag } => write!(f, "flat callback tag {tag} has no live dispatcher"),
        }
    }
}
impl core::error::Error for FlatError {}

/// Shell-owned flat plugin instance, retained until all subscriptions are gone.
/// Implementations must keep the owner and its module alive during callbacks.
pub trait FlatInstance {
    /// Visit EVERY committed registration record, in batch order, on every call.
    /// Events include tick; draw and later kinds are Other with their record tag.
    /// The committed registrations and indices must remain unchanged after make.
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration));

    /// The real pinned environment scope used by this instance's host table.
    fn environment(&self) -> Pin<&EnvironmentScope>;

    /// Invoke the validated event callback under RegistrationBatch admission.
    /// The payload remains borrowed only until this call returns.
    ///
    /// # Errors
    /// Returns the owner/admission error without accessing foreign state.
    fn dispatch_event(
        &self,
        index: usize,
        payload: &[u8],
        depth: &DispatchDepth,
    ) -> Result<i32, Error>;

    /// Log a failure against the plugin. Never retry, panic or invoke plugin code.
    /// Lifecycle failures must not prevent the shell from powering off.
    fn event_dropped(&self, tag: u32, why: DropReason);
}

/// Host adapter for events 32–37. Shells return this through CodeLoader.
/// The shared depth must be the same for every flat instance on this device.
#[non_exhaustive]
pub struct FlatPlugin {
    manifest: Manifest,
    instance: Rc<dyn FlatInstance>,
    depth: Rc<DispatchDepth>,
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
    /// Retain a shell-owned instance. Allocations occur only during construction
    /// and subscription registration, never during event encoding or dispatch.
    ///
    /// # Errors
    /// Refuses the entire instance when a committed callback cannot be routed.
    pub fn new(
        manifest: Manifest,
        instance: Box<dyn FlatInstance>,
        depth: Rc<DispatchDepth>,
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
            instance: Rc::from(instance),
            depth,
            host: FlatHost {
                meter: Rc::clone(&host.meter),
            },
        })
    }
}

impl Plugin for FlatPlugin {
    fn manifest(&self) -> Manifest {
        self.manifest.clone()
    }

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
            let instance = Rc::clone(&self.instance);
            let depth = Rc::clone(&self.depth);
            let host = FlatHost {
                meter: Rc::clone(&self.host.meter),
            };
            result = registrar.subscribe(
                event,
                Box::new(move |env, params, _| {
                    deliver(&*instance, &depth, &host, tag, index, env, params)
                }),
            );
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
    let frame = crate::abi7::core_frame::CoreFrame {
        env,
        meter: &host.meter,
        usage: env.usage(),
        plugins: env.plugins(),
        work: env.work(),
    };
    let status = instance
        .environment()
        .scope_core(&frame, || instance.dispatch_event(index, bytes, depth))
        .map_err(admission)?
        .map_err(admission)?;
    if status == paperpaw_native::host_glue::OK {
        Ok(())
    } else {
        Err(DropReason::Plugin(status))
    }
}

#[cfg(test)]
#[path = "flat/routing_tests.rs"]
mod routing_tests;

#[cfg(test)]
#[path = "flat/test_loader.rs"]
pub(crate) mod test_loader;
