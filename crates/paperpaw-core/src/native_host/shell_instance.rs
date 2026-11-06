//! One hosted plugin, made and committed, as the kernel's [`FlatInstance`]:
//! what the desktop and the X4 both need around [`Hosted`], once.
//!
//! The instance owns the pinned [`Hosted`] and the batch that borrows it
//! (self-referential, so the borrow is extended with one documented
//! `unsafe`). Release order, in `Drop`: the batch (its lease on the owner),
//! then admission is stopped and the owner finished, then the owner drops:
//! the plugin's destructor, then the services, then the module. If `finish`
//! sees a lease the owner is kept, never freed, and the shell is told.
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem::ManuallyDrop;
use core::pin::Pin;

use paperpaw_native::host_glue::{self, Services};

use super::{Batch, Error, Hosted, Invocation, Make, dispatch_registration, try_box};
use crate::native_host::Declarations;
use crate::native_host::environment::EnvironmentScope;
use crate::native_host::flat::{DropReason, FlatInstance, FlatRegistration};
use crate::native_host::registrations::{self, Registration};
use crate::native_host::{self, DispatchDepth};

/// The wire tag of a draw registration (`abi/`, registration records).
const DRAW_RECORD_TAG: u32 = 2;

/// What the shell does with what the instance cannot do itself (logging is
/// the shell's).
#[derive(Clone, Copy)]
#[allow(missing_debug_implementations, reason = "function pointers")]
pub struct Hooks {
    /// An event for this plugin id was dropped, and why.
    pub event_dropped: fn(id: &str, tag: u32, why: DropReason),
    /// The owner of this plugin id was still in use at the drop and is kept
    /// (leaked on purpose: whatever holds the module must outlive it).
    pub leaked: fn(id: &str),
}

/// One plugin, hosted, made and committed.
#[allow(missing_debug_implementations, reason = "holds the generated table")]
pub struct Instance<S: Services + 'static, M: 'static> {
    id: Box<str>,
    /// What it registered, in batch order.
    records: Vec<Registration>,
    /// Staged by `make`, kept and never applied here (never dropped silently).
    declarations: Declarations,
    batch: Option<Batch<'static, S, M, Registration>>,
    hosted: ManuallyDrop<Box<Hosted<S, M>>>,
    hooks: Hooks,
}

impl<S: Services + 'static, M: 'static> Instance<S, M> {
    /// Hosts `module`'s plugin `id`, runs its `make` inside the registration
    /// scope (with `image` given to staging first), commits, and reads its
    /// records. Everything allocated here is fallible.
    ///
    /// # Errors
    /// Any refusal of [`Hosted`] or the owner; what was made is released.
    ///
    /// # Safety
    ///
    /// As [`Hosted::new`]: `make` is the validated entry of the code `module`
    /// keeps loaded, and `image` is a read-only mapping that stays mapped
    /// while `module` lives, or `(0, 0)`.
    pub unsafe fn load(
        services: S,
        module: M,
        make: Make,
        id: &str,
        image: (usize, usize),
        hooks: Hooks,
    ) -> Result<Self, Error> {
        // SAFETY: the caller's contract.
        let hosted = try_box(unsafe { Hosted::new(services, module, make, id, image) }?)?;
        let mut transaction = hosted.begin::<Registration>()?;
        let image = hosted.image().unwrap_or(&[]);
        transaction.set_image(image)?;
        let mut batch = hosted.make_registered(transaction)?.commit()?;
        let declarations = batch.take_declarations();
        let depth = DispatchDepth::new();
        let mut records = Vec::new();
        records
            .try_reserve_exact(batch.len())
            .map_err(|_| Error::Allocation)?;
        for index in 0..batch.len() {
            records.push(batch.dispatch(index, &depth, |_, record| *record)?);
        }
        let mut owned_id = alloc::string::String::new();
        owned_id
            .try_reserve_exact(id.len())
            .map_err(|_| Error::Allocation)?;
        owned_id.push_str(id);
        // SAFETY: the batch borrows the owner, which lives on the heap inside
        // `hosted` (a `Box` never moved out of nor dropped before the batch:
        // `Drop` releases the batch first and field order does the rest).
        // Nothing else holds the extended lifetime.
        let batch = unsafe {
            core::mem::transmute::<Batch<'_, S, M, Registration>, Batch<'static, S, M, Registration>>(
                batch,
            )
        };
        Ok(Self {
            id: owned_id.into_boxed_str(),
            records,
            declarations,
            batch: Some(batch),
            hosted: ManuallyDrop::new(hosted),
            hooks,
        })
    }

    /// The host-known plugin id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Stops callback admission: from here a dispatch answers the owner's
    /// error without reaching the plugin, and [`image`](Self::image) gives
    /// none. The module stays alive, with its owner, until the drop.
    pub fn stop(&self) {
        self.hosted.stop();
    }

    /// The plugin's read-only data (`DeclRef::Image` offsets index it);
    /// `None` once stopped. Decoders re-validate: a plugin has no sandbox.
    #[must_use]
    pub fn image(&self) -> Option<&[u8]> {
        self.hosted.image()
    }

    /// What it declared and nobody applied: the count of declaration records.
    /// A shell that cannot apply them refuses the plugin (never loads it
    /// without them).
    #[must_use]
    pub fn declared_records(&self) -> usize {
        self.declarations.specs().len()
            + self.declarations.groups().len()
            + self.declarations.extras().len()
            + self.declarations.hyphenations().len()
            + self.declarations.font_faces().len()
            + self.declarations.font_extensions().len()
    }

    /// Dispatches record `index` with `invocation`; a stopped instance
    /// answers the owner's error before anything of the plugin is read.
    ///
    /// # Errors
    /// The owner's error, or `Status` for the callback's absence.
    pub fn dispatch(
        &self,
        index: usize,
        depth: &DispatchDepth,
        invocation: Invocation<'_>,
    ) -> Result<i32, Error> {
        match (&self.batch, self.hosted.is_stopped()) {
            (Some(batch), false) => dispatch_registration(batch, index, depth, invocation),
            _ => Err(Error::Lifecycle(native_host::Error::State)),
        }
    }

    /// The tick registrations, each called with `now_ms`; their statuses.
    pub fn tick(&self, now_ms: u64) -> Vec<i32> {
        let mut bytes = [0u8; 64];
        let Ok(len) = registrations::tick_encode(now_ms, &mut bytes) else {
            return Vec::new();
        };
        let depth = DispatchDepth::new();
        self.records
            .iter()
            .enumerate()
            .filter(|(_, record)| matches!(record, Registration::Event { .. }))
            .map(
                |(index, _)| match self.dispatch(index, &depth, Invocation::Tick(&bytes[..len])) {
                    Ok(status) | Err(Error::Status(status)) => status,
                    Err(_) => host_glue::ERR_STATE,
                },
            )
            .collect()
    }
}

impl<S: Services + 'static, M: 'static> Drop for Instance<S, M> {
    fn drop(&mut self) {
        self.hosted.stop();
        drop(self.batch.take());
        if self.hosted.owner().finish().is_ok() {
            // SAFETY: dropped exactly once, here; nothing borrows the owner
            // any longer (the batch is gone and `finish` saw no lease).
            unsafe { ManuallyDrop::drop(&mut self.hosted) };
        } else {
            // Not reachable while no lease is held. If it ever is, whatever
            // holds the module must outlive it: kept, never freed.
            (self.hooks.leaked)(&self.id);
        }
    }
}

impl<S: Services + 'static, M: 'static> FlatInstance for Instance<S, M> {
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration)) {
        for (index, record) in self.records.iter().enumerate() {
            visit(match *record {
                Registration::Event { event_tag, .. } => {
                    FlatRegistration::Event { event_tag, index }
                }
                Registration::Draw { .. } => FlatRegistration::Other {
                    record_tag: DRAW_RECORD_TAG,
                },
            });
        }
    }

    fn environment(&self) -> Pin<&EnvironmentScope> {
        let context = self.hosted.owner().context();
        // SAFETY: the owner structurally pins its context; this projects one
        // field and never moves it.
        unsafe { context.map_unchecked(|value| &value.environment) }
    }

    fn dispatch_event(
        &self,
        index: usize,
        payload: &[u8],
        depth: &DispatchDepth,
    ) -> Result<i32, native_host::Error> {
        match self.dispatch(index, depth, Invocation::Event(payload)) {
            Ok(status) | Err(Error::Status(status)) => Ok(status),
            Err(Error::Lifecycle(why)) => Err(why),
            Err(_) => Err(native_host::Error::State),
        }
    }

    fn event_dropped(&self, tag: u32, why: DropReason) {
        (self.hooks.event_dropped)(&self.id, tag, why);
    }
}
