//! One hosted plugin, made and committed, as the kernel's [`FlatInstance`](crate::native_host::flat::FlatInstance):
//! what the desktop and the X4 both need around [`Hosted`], once.
//!
//! The instance owns the pinned [`Hosted`] and the batch that leases it
//! (self-referential: the lease is a raw pointer with a lifetime marker,
//! made so by one documented `unsafe`, never a reference into the owner).
//! Release order, in `Drop`: the batch (its lease on the owner),
//! then admission is stopped and the owner finished, then the owner drops:
//! the plugin's destructor, then the services, then the module. If `finish`
//! sees a lease the owner is kept, never freed, and the shell is told.
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem::ManuallyDrop;
use core::pin::Pin;

use paperpaw_native::host_glue::{self, Services};

use super::{
    Batch, DeclaredServices, Error, Hosted, Invocation, Make, Report, dispatch_registration,
    try_box,
};
use crate::native_host::Declarations;
use crate::native_host::FlatHost;
use crate::native_host::environment::EnvironmentScope;
use crate::native_host::flat::{
    DropReason, FlatDepth, FlatError, FlatInstance, FlatPlugin, FlatRegistration,
};
use crate::native_host::registrations::{self, EVENT_TICK_TAG, Registration};
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
    /// A cancel or release step of this plugin id's teardown failed (logged
    /// by the shell; the teardown goes on: a broken backend must not keep the
    /// reader from unloading or powering off).
    pub teardown_failed: fn(id: &str, why: native_host::Error),
    /// Plugin code of this plugin id is about to run (`make`, a callback or
    /// the plugin's destructor). Where it answers `true` the call runs and is
    /// always followed by `call_end`, also when the call failed. The X4 uses
    /// the pair to keep the id in memory a reset survives while the plugin
    /// runs, so a crash is told afterwards.
    ///
    /// `false` refuses the call (the X4 refuses at an interrupt level that
    /// must never run plugin code; `abi/heap-word.md`): the plugin is not run
    /// and no `call_end` follows. A refused `make` is `Error::Refused`, a
    /// refused callback `Error::Refused` (nothing of it changed), and a
    /// refused destructor (the drop) keeps the owner as a leak and tells
    /// [`Hooks::leaked`]; none of them panics. The shell logs the refusal
    /// once.
    pub call_begin: fn(id: &str) -> bool,
    /// The plugin code returned (the shell checks its stack here).
    pub call_end: fn(id: &str),
}

/// Plugin code runs between `new` and the drop, announced to the shell.
struct Call<'a> {
    hooks: &'a Hooks,
    id: &'a str,
}

impl<'a> Call<'a> {
    /// `None` where the shell refused the call (it will not run, no `call_end`).
    fn new(hooks: &'a Hooks, id: &'a str) -> Option<Self> {
        (hooks.call_begin)(id).then(|| Self { hooks, id })
    }
}

impl Drop for Call<'_> {
    fn drop(&mut self) {
        (self.hooks.call_end)(self.id);
    }
}

/// One plugin, hosted, made and committed.
#[allow(missing_debug_implementations, reason = "holds the generated table")]
pub struct Instance<S: Services + 'static, M: 'static> {
    id: Box<str>,
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
    /// As [`Hosted::new`] (`report` is the shell's real, non-allocating sink;
    /// `resources` is the device-wide table, a share of it;
    /// `declared_services` is what the plugin's metadata declares, or
    /// `DeclaredServices::none()`): `make` is the validated entry of the code
    /// `module` keeps loaded, and `image` is a read-only mapping that stays
    /// mapped while `module` lives, or `(core::ptr::null(), 0)`.
    #[allow(
        clippy::too_many_arguments,
        reason = "the shell's one entry: the parts of a hosted plugin, the device's table and the loader's hooks"
    )]
    pub unsafe fn load(
        services: S,
        module: M,
        make: Make,
        id: &str,
        image: (*const u8, usize),
        report: Report<S>,
        hooks: Hooks,
        resources: native_host::HostResources,
        declared_services: DeclaredServices,
    ) -> Result<Self, Error> {
        // One announced call over the whole load, declared first so that it
        // ends last: whatever this function drops on a refusal, the plugin's
        // destructor included, runs inside it (attributed, and the shell's
        // stack check follows it), also when an allocation after `commit`
        // fails.
        let Some(_call) = Call::new(&hooks, id) else {
            return Err(Error::Refused);
        };
        // SAFETY: the caller's contract.
        let hosted = try_box(unsafe {
            Hosted::new(
                services,
                module,
                make,
                id,
                image,
                report,
                resources,
                declared_services,
            )
        }?)?;
        let mut transaction = hosted.begin::<Registration>()?;
        let image = hosted.image().unwrap_or(&[]);
        transaction.set_image(image)?;
        let mut batch = hosted.make_registered(transaction)?.commit()?;
        let declarations = batch.take_declarations();
        let mut owned_id = alloc::string::String::new();
        owned_id
            .try_reserve_exact(id.len())
            .map_err(|_| Error::Allocation)?;
        owned_id.push_str(id);
        // SAFETY: `hosted` keeps the pinned owner on the heap for as long as
        // this instance lives, and `Drop` (and every failure path before it)
        // releases the batch before the owner can be freed. The batch keeps
        // only a raw pointer and a lifetime marker, no reference into the
        // owner (the same design as the retained holder in `Retained`).
        let batch = unsafe { batch.into_retained() };
        Ok(Self {
            id: owned_id.into_boxed_str(),
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

    /// Where the hosted plugin lives, for a test that keeps it on purpose
    /// (a static pointing at it keeps Miri's leak check quiet).
    #[cfg(test)]
    pub(super) fn hosted_address(&self) -> *mut u8 {
        let hosted: &Hosted<S, M> = &self.hosted;
        core::ptr::from_ref(hosted).cast::<u8>().cast_mut()
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
            (Some(batch), false) => {
                let Some(_call) = Call::new(&self.hooks, &self.id) else {
                    return Err(Error::Refused);
                };
                dispatch_registration(batch, index, depth, invocation)
            }
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
        let Some(batch) = &self.batch else {
            return Vec::new();
        };
        (0..batch.len())
            .filter(|&index| {
                matches!(
                    batch.record(index),
                    Some(Registration::Event { event_tag, .. }) if *event_tag == EVENT_TICK_TAG
                )
            })
            .map(
                |index| match self.dispatch(index, &depth, Invocation::Tick(&bytes[..len])) {
                    Ok(status) | Err(Error::Status(status)) => status,
                    Err(_) => host_glue::ERR_STATE,
                },
            )
            .collect()
    }
}

impl<S: Services + 'static, M: 'static> Instance<S, M> {
    /// Hands this instance to the kernel as a flat plugin. Every allocation
    /// is fallible: the instance box, the device's shared dispatch depth
    /// (made with the first plugin and kept for the session), the adapter's
    /// shared node and the adapter's box. On any refusal the instance is
    /// dropped exactly once, whole (stop, lease, cancel, release, destroy,
    /// module), and the depth keeps no extra share.
    ///
    /// # Errors
    /// `FlatError::Allocation` when the heap has no room, or the adapter's
    /// refusal of an unrouted registration.
    pub fn into_plugin(
        self,
        manifest: paperpaw_sdk::Manifest,
        depth: &mut Option<FlatDepth>,
        host: &FlatHost,
    ) -> Result<Box<FlatPlugin>, FlatError> {
        let instance: Box<dyn FlatInstance> = try_box(self).map_err(|_| FlatError::Allocation)?;
        let shared = match depth.as_ref() {
            Some(depth) => depth.try_clone(),
            None => FlatDepth::try_new().ok().and_then(|made| {
                let share = made.try_clone();
                *depth = Some(made);
                share
            }),
        }
        .ok_or(FlatError::Allocation)?;
        let plugin = FlatPlugin::new(manifest, instance, shared, host)?;
        try_box(plugin).map_err(|_| FlatError::Allocation)
    }
}

impl<S: Services + 'static, M: 'static> Drop for Instance<S, M> {
    /// The unload sequence (ADR 0034): admission is stopped at once, the
    /// batch's lease released, the owner's cancel and release steps run
    /// (`prepare_teardown`) and their errors are told, then `finish` destroys
    /// the plugin and the owner drops with the services and the module.
    /// Subscriptions and queued work of this plugin are gone before this
    /// runs: they held the instance, and this is its last reference.
    fn drop(&mut self) {
        self.hosted.stop();
        drop(self.batch.take());
        // The plugin's destructor runs in `finish` or in the owner's drop.
        let Some(_call) = Call::new(&self.hooks, &self.id) else {
            // The shell refuses plugin code now: the destructor does not run,
            // the owner is kept (never freed, as for a teardown that stays
            // busy) and the shell is told. Nothing panics.
            (self.hooks.leaked)(&self.id);
            return;
        };
        let owner = self.hosted.owner();
        // A step answering Busy cannot be deferred from a drop: it is told,
        // and `finish` below tries the same steps once more and refuses the
        // destroy while one is still busy (the owner is then kept).
        if let Err(why) = owner.prepare_teardown() {
            (self.hooks.teardown_failed)(&self.id, why);
        }
        if let Some(why) = owner.take_teardown_error() {
            (self.hooks.teardown_failed)(&self.id, why);
        }
        if owner.finish().is_ok() {
            // SAFETY: dropped exactly once, here; nothing borrows the owner
            // any longer (the batch is gone and `finish` saw no lease).
            unsafe { ManuallyDrop::drop(&mut self.hosted) };
        } else {
            // A teardown step still refuses (Busy) after its retry: the
            // plugin is not destroyed and the owner is kept, never freed, so
            // whatever holds the module must outlive it (tested with a
            // persistent fault). The shell is told.
            (self.hooks.leaked)(&self.id);
        }
    }
}

impl<S: Services + 'static, M: 'static> FlatInstance for Instance<S, M> {
    fn registrations(&self, visit: &mut dyn FnMut(FlatRegistration)) {
        // Read through the batch: no second copy of the records is kept.
        let Some(batch) = &self.batch else {
            return;
        };
        for index in 0..batch.len() {
            let Some(record) = batch.record(index) else {
                continue;
            };
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
        out: &mut [u8],
        depth: &DispatchDepth,
    ) -> Result<i32, native_host::Error> {
        // The events this module hands on have no reply family (an empty,
        // non-null reply buffer goes to the callback): a caller asking for
        // a reply is a host mistake, refused before any plugin code.
        if !out.is_empty() {
            return Err(native_host::Error::Invalid);
        }
        match self.dispatch(index, depth, Invocation::Event(payload)) {
            Ok(status) | Err(Error::Status(status)) => Ok(status),
            Err(Error::Lifecycle(why)) => Err(why),
            // The shell refused to run plugin code now: try again later.
            Err(Error::Refused) => Err(native_host::Error::Busy),
            Err(_) => Err(native_host::Error::State),
        }
    }

    fn event_dropped(&self, tag: u32, why: DropReason) {
        (self.hooks.event_dropped)(&self.id, tag, why);
    }
}
