//! The shell's side of one interface-7 plugin, shared by the desktop emulator and the X4 firmware:
//! the owner with its host table, the transactional `make`, the plugin's output, and the scoped
//! dispatch of its callbacks.
//!
//! What differs between shells is only small and stays in the shell: the
//! [`Services`](paperpaw_native::host_glue::Services) (heap, clock), the *module* `M` the owner
//! keeps alive (a loaded library on the desktop, the placed image on the X4), the `make` function,
//! and the read-only range of the image. Everything that touches the plugin's table and the
//! lifetime rules is here once, so it is reviewed and tested once.
//!
//! Rules (ADR 0034, `abi/`): the owner is pinned on the heap and outlives every batch and lease;
//! `make` runs inside the registration scope and its records stay non-dispatchable until the output
//! validates and commits; the plugin's destructor runs exactly once, while the context, services
//! and module are alive; Env and Canvas are lent for one synchronous callback.
use alloc::alloc::alloc;
use alloc::boxed::Box;
use core::{alloc::Layout, cell::Cell, ffi::c_void, pin::Pin, ptr};

use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{
    self, PpBuffer, PpBytes, PpHost, PpPlugin, Services, ShellCtx, read_plugin_dispatch_event,
    read_plugin_draw, read_plugin_drop, read_plugin_input, read_plugin_size, read_plugin_state,
};
use paperpaw_platform::Rect;

pub use super::admitted_features::AdmittedFeatures;
use super::canvas::CanvasScope;
pub use super::data_services::DATA_SERVICES;
pub use super::declared_services::{DeclaredServices, Error as DeclaredServicesError};
pub use super::device_service::DEVICE_SERVICES;
use super::environment::EnvironmentScope;
use super::registration_scope::RegistrationScope;
use super::registrations::{self, EVENT_TICK_TAG, Registration};
use super::{Construction, DispatchDepth, HostOwner, PublishedRecords, RegistrationBatch};
use crate::sdk::Env;

/// Why a step of the plugin's life was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The owner's lifecycle said so.
    Lifecycle(super::Error),
    /// No room for the owner.
    Allocation,
    /// A second `make`, a transaction of another owner, or no instance.
    Attempt,
    /// A status the plugin or the registration decode returned.
    Status(i32),
    /// `make` returned this status.
    Factory(i32),
    /// The plugin's output table is not one this host accepts.
    Plugin,
    /// The shell refused the call (`Hooks::call_begin`): the plugin was not run.
    Refused,
}

impl From<super::Error> for Error {
    fn from(value: super::Error) -> Self {
        Self::Lifecycle(value)
    }
}

/// The plugin's factory: `make(host, ctx, out)`.
pub type Make = unsafe extern "C" fn(*const PpHost, *mut c_void, *mut *const PpPlugin) -> i32;

type Event = unsafe extern "C" fn(*mut c_void, u32, PpBytes, PpBuffer) -> i32;
type Draw = unsafe extern "C" fn(*mut c_void, u32, PpBytes) -> i32;
type Input = unsafe extern "C" fn(*mut c_void, u32, PpBytes) -> i32;

/// What `make` returned, copied from its table: only generated, bounded
/// fields, and never a reference to the whole table.
#[derive(Clone, Copy)]
struct PluginOutput {
    state: *mut c_void,
    destroy: unsafe extern "C" fn(*mut c_void),
    event: Option<Event>,
    draw: Option<Draw>,
    input: Option<Input>,
}

fn optional<T>(callback: Result<T, i32>) -> Result<Option<T>, Error> {
    match callback {
        Ok(callback) => Ok(Some(callback)),
        Err(host_glue::ERR_UNSUPPORTED) => Ok(None),
        Err(_) => Err(Error::Plugin),
    }
}

/// Reads the output of a successful `make`.
///
/// # Safety
///
/// The factory promises an aligned, readable size word and a stable readable
/// allocation covering its declared size. Numeric checks cannot establish
/// those memory guarantees.
unsafe fn read_output(out: *const PpPlugin) -> Result<PluginOutput, Error> {
    if out.is_null() || !(out as usize).is_multiple_of(align_of::<PpPlugin>()) {
        return Err(Error::Plugin);
    }
    // SAFETY: the successful factory promises a readable size prefix.
    let accessible = unsafe { out.cast::<u32>().read() } as usize;
    let table = out.cast::<u8>();
    // SAFETY: the factory guarantees this live, initialized span; the
    // generated readers check numeric bounds before copying single fields.
    unsafe {
        read_plugin_size(table, accessible).map_err(|_| Error::Plugin)?;
        Ok(PluginOutput {
            state: read_plugin_state(table, accessible).map_err(|_| Error::Plugin)?,
            destroy: read_plugin_drop(table, accessible).map_err(|_| Error::Plugin)?,
            event: optional(read_plugin_dispatch_event(table, accessible))?,
            draw: optional(read_plugin_draw(table, accessible))?,
            input: optional(read_plugin_input(table, accessible))?,
        })
    }
}

/// Shell diagnostic sink. It must log owner, step and error without allocating,
/// panicking or reentering the owner. No default or silent sink is supplied.
/// The same rules apply during callbacks, including no conflicting RefCell
/// borrows. Release also reports failed callback blob cleanup; it does not
/// mean the callback was refused or teardown has begun.
pub type Report<S> = fn(&S, &str, super::TeardownStep, super::Error);

/// The host table's `ctx`: what the shell's entries reach. Pinned inside the
/// owner; the plugin keeps only its address.
#[allow(missing_debug_implementations, reason = "holds the generated table")]
pub struct Context<S: 'static> {
    /// The one table every plugin hosted with `S` shares ([`SharedTable`]):
    /// a static, never a copy per plugin (152 bytes on the X4).
    table: &'static PpHost,
    report: Report<S>,
    resources: super::resources::OwnerResources,
    declared_services: DeclaredServices,
    /// The shell's heap and clock.
    pub services: S,
    instance: Cell<Option<PluginOutput>>,
    attempted: Cell<bool>,
    /// The scope in which `make`'s registrations are staged.
    pub registrations: RegistrationScope<Registration>,
    pub(crate) commands: super::commands::CommandScope,
    /// The scope in which a callback may query the environment.
    pub environment: EnvironmentScope,
    /// The scope in which a draw callback may paint.
    pub canvas: CanvasScope,
}

impl<S: 'static> Context<S> {
    /// The host-known id of the plugin this context belongs to: the caller
    /// its resources were bound to before the context was made, kept once.
    pub(crate) fn plugin_id(&self) -> &str {
        // `Hosted::new` binds the caller before the context exists and a
        // bound caller is never replaced; "" is never a plugin's id.
        self.resources
            .bound_caller()
            .map_or("", paperpaw_sdk::Id::as_str)
    }
}

/// The pinned owner of one plugin.
pub type Owner<S, M> = HostOwner<Context<S>, M>;
/// Staging of one `make`.
pub type Staging<'a, S, M, T> = Construction<'a, Context<S>, M, T>;
/// The records a `make` registered, dispatchable.
pub type Batch<'a, S, M, T> = RegistrationBatch<'a, Context<S>, M, T>;
/// The records a `make` registered, as an instance retains them after its
/// declarations were moved out.
pub type Published<'a, S, M, T> = PublishedRecords<'a, Context<S>, M, T>;
type MakeOutput = (i32, *const PpPlugin);

/// A started `make`: the context it belongs to and its staging.
#[allow(missing_debug_implementations, reason = "holds the generated table")]
pub struct Transaction<'a, S: 'static, M, T> {
    context: Pin<&'a Context<S>>,
    staging: Staging<'a, S, M, T>,
}

impl<'a, S: 'static, M, T> Transaction<'a, S, M, T> {
    /// Stages one record.
    ///
    /// # Errors
    /// The owner's refusal, with the record given back.
    pub fn stage(&mut self, record: T) -> Result<(), (super::Error, T)> {
        self.staging.stage(record)
    }

    /// Gives staging the read-only data of the plugin's image before `make`:
    /// a batch lying wholly inside it is kept as offsets, not copied.
    ///
    /// # Errors
    /// `State` on a second or late call.
    pub fn set_image(&mut self, image: &'a [u8]) -> Result<(), super::Error> {
        self.staging.set_image(image)
    }

    /// The staging itself, for what the shell reads before `make`.
    pub const fn staging(&self) -> &Staging<'a, S, M, T> {
        &self.staging
    }
}

/// One plugin, hosted: its pinned owner, its factory and the host-known id.
///
/// The owner is private: replacing or swapping it would unload the module
/// while `make` and the image still point into it.
///
/// ```compile_fail,E0616
/// use paperpaw_core::native_host::shell::Hosted;
/// fn swap<S: 'static, M>(a: &mut Hosted<S, M>, b: &mut Hosted<S, M>) {
///     core::mem::swap(&mut a.owner, &mut b.owner);
/// }
/// ```
#[allow(missing_debug_implementations, reason = "holds the generated table")]
pub struct Hosted<S: 'static, M> {
    owner: super::PinnedOwner<Context<S>, M>,
    make: Make,
    /// The plugin's read-only data as mapped: pointer with provenance and length.
    image: (*const u8, usize),
    /// Admission was stopped ([`Hosted::stop`]): no image is given any more.
    stopped: Cell<bool>,
}

/// The host table with this shell's entries in it, as a plugin of `S` sees it.
/// The shell's preflight may read it before anything is hosted.
#[must_use]
pub const fn host_table<S: Services + 'static>() -> PpHost {
    let mut table = host_glue::host_table();
    table.heap_alloc = heap_alloc::<S>;
    table.heap_dealloc = heap_dealloc::<S>;
    table.heap_realloc = heap_realloc::<S>;
    table.clock_micros = clock_micros::<S>;
    table.stage_run = stage_slot::run::<S>;
    table.text_covers = text_covers::covers::<S>;
    table.ui_compose = super::ui_compose::compose::<S>;
    table.registrations_submit = registrations_submit::<S>;
    table.commands_submit = commands_submit::<S>;
    table.env_snapshot = env_snapshot::<S>;
    table.settings_read = settings_read::<S>;
    table.canvas_fill_rect = canvas_fill_rect::<S>;
    table.i18n_copy = i18n_copy::<S>;
    table.i18n_format = i18n_format::<S>;
    table.i18n_digits = i18n_digits::<S>;
    table.canvas_draw_line = canvas_draw_line::<S>;
    table.canvas_fill_round = canvas_fill_round::<S>;
    table.canvas_stroke_round = canvas_stroke_round::<S>;
    table.canvas_clip = canvas_clip::<S>;
    table.canvas_describe = canvas_describe::<S>;
    table.storage_size = storage_externs::storage_size::<S>;
    table.storage_read_at = storage_externs::storage_read_at::<S>;
    table.storage_write = storage_externs::storage_write::<S>;
    table.storage_append = storage_externs::storage_append::<S>;
    table.storage_rename = storage_externs::storage_rename::<S>;
    table.storage_remove = storage_externs::storage_remove::<S>;
    table.storage_sync = storage_externs::storage_sync::<S>;
    table.service_open = service_open::<S>;
    table.service_call = service_call::<S>;
    table.services_query = services_query::<S>;
    table.blob_read = blob_read::<S>;
    table.blob_release = blob_release::<S>;
    table.font_acquire = font_acquire::<S>;
    table.handle_release = handle_release::<S>;
    table.text_measure = text_measure::<S>;
    table.text_metrics = text_metrics::<S>;
    table.text_draw = text_draw::<S>;
    doc_slots::install::<S>(&mut table);
    // The X4 ink probe is enabled only after device timing qualifies it.
    #[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
    {
        table.text_ink = text_ink::<S>;
    }
    table
}

impl<S: Services + 'static, M> Hosted<S, M> {
    /// Hosts the plugin `id` whose factory is `make`, kept alive by `module`.
    /// `report` is the shell's real diagnostic sink, including the X4 sink.
    /// `image` is the pointer and length of the plugin's read-only data
    /// (null and zero for none).
    ///
    /// # Errors
    /// `Allocation` when the id or the owner cannot be placed; `services` and
    /// `module` are then dropped, `make` was not called.
    ///
    /// # Safety
    ///
    /// - `make` is the validated entry of the code that `module` keeps loaded,
    ///   and stays callable while `module` lives.
    /// - `image` is a read-only mapping of that many bytes which stays mapped
    ///   while `module` lives, or a null pointer with zero length.
    #[allow(
        clippy::too_many_arguments,
        reason = "admission supplies the owner resources and checked requirements"
    )]
    pub unsafe fn new(
        services: S,
        module: M,
        make: Make,
        id: &str,
        image: (*const u8, usize),
        report: Report<S>,
        resources: super::HostResources,
        declared_services: DeclaredServices,
    ) -> Result<Self, Error> {
        let mut resources = resources.try_owner().map_err(Error::Status)?;
        resources.bind_caller(id).map_err(|status| {
            if status == host_glue::ERR_NO_MEMORY {
                Error::Allocation
            } else {
                Error::Status(status)
            }
        })?;
        let scoped_resources = resources
            .try_view()
            .ok_or(Error::Status(host_glue::ERR_NO_MEMORY))?;
        let context = Context {
            table: SharedTable::<S>::TABLE,
            report,
            resources,
            declared_services,
            services,
            instance: Cell::new(None),
            attempted: Cell::new(false),
            registrations: RegistrationScope::new(),
            commands: super::commands::CommandScope::new(),
            environment: EnvironmentScope::with_resources(scoped_resources),
            canvas: CanvasScope::new(),
        };
        let owner = pin_owner(HostOwner::new(context, module))?;
        let context = owner.as_ref().context();
        // SAFETY: the owner pins Context<S> and keeps it alive through every
        // callback and cleanup. Report<S> promises no panic or reentry.
        unsafe {
            context.environment.set_cleanup_reporter(
                (context.get_ref() as *const Context<S>).cast(),
                report_blob_cleanup::<S>,
            );
        }
        Ok(Self {
            owner,
            make,
            image,
            stopped: Cell::new(false),
        })
    }

    /// Per-instance available features retained before make, without metadata.
    #[must_use]
    pub fn features(&self) -> AdmittedFeatures {
        self.owner.as_ref().context().declared_services.features()
    }

    /// Retain only the pool identity and this load's authoritative owner key.
    pub(crate) fn resource_identity(&self) -> Option<super::resources::OwnerResourceView> {
        self.owner.as_ref().context().resources.try_view()
    }

    /// The pinned owner; callers lease and finish through it. To stop the
    /// plugin call [`stop`](Self::stop), not `revoke` here: only `stop` also
    /// withdraws [`image`](Self::image).
    #[must_use]
    pub fn owner(&self) -> Pin<&Owner<S, M>> {
        self.owner.as_ref()
    }

    /// Stops callback admission: from here a dispatch answers the owner's
    /// error and [`image`](Self::image) gives none. The module stays alive
    /// until the drop. The kernel observes registered owners and completes
    /// the same ordered unload through its main loop, retrying busy cleanup.
    pub fn stop(&self) {
        self.stopped.set(true);
        self.owner.as_ref().revoke();
    }

    /// Whether [`stop`](Self::stop) was called.
    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.stopped.get()
    }

    /// The plugin's read-only data, as mapped; `None` once stopped. The bytes
    /// are read-only mappings; a plugin without a sandbox can remap them, so
    /// every reader decodes with checks and never trusts them to stay as
    /// they were.
    #[must_use]
    pub fn image(&self) -> Option<&[u8]> {
        if self.stopped.get() {
            return None;
        }
        let (start, len) = self.image;
        if len == 0 {
            return Some(&[]);
        }
        // SAFETY: `new`'s contract: a read-only mapping of `len` bytes at
        // `start`, held by the owner's module for as long as `self` lives.
        Some(unsafe { core::slice::from_raw_parts(start, len) })
    }

    /// Starts the transaction `make` registers in.
    ///
    /// # Errors
    /// The owner's refusal.
    pub fn begin<T>(&self) -> Result<Transaction<'_, S, M, T>, Error> {
        Ok(Transaction {
            context: self.owner.as_ref().context(),
            staging: self
                .owner
                .as_ref()
                .begin(self.owner.as_ref().context().get_ref().plugin_id())?,
        })
    }

    /// Runs `make` with its `registrations.submit` staging into the
    /// transaction. The scope borrows staging only while `make` runs, and
    /// resets before the output is validated or the pending instance can take
    /// over its destruction.
    ///
    /// # Errors
    /// `Attempt`, `Factory`, `Plugin`, or the scope's refusal.
    pub fn make_registered<'a>(
        &'a self,
        transaction: Transaction<'a, S, M, Registration>,
    ) -> Result<PendingInstance<'a, S, M, Registration>, Error> {
        self.make_with(transaction, |context, staging| {
            // SAFETY: the owner structurally pins this field; no API moves it.
            let scope = unsafe { context.map_unchecked(|value| &value.registrations) };
            scope
                .scope(staging, || self.invoke_make(context))
                .map_err(Into::into)
        })
    }

    fn invoke_make(&self, context: Pin<&Context<S>>) -> MakeOutput {
        let mut out = ptr::null();
        let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast();
        // SAFETY: the owner is pinned, owns services and module, and `out` is writable.
        let status = context.run_plugin(|| unsafe { (self.make)(context.table, ctx, &mut out) });
        (status, out)
    }

    /// The output stays loader-owned until commit succeeds. A failed factory
    /// returns no live output; no unvalidated partial-output destructor runs.
    /// A table that does not validate (`Error::Plugin`) leaves the state the
    /// plugin made behind: nothing may call a destructor it did not prove.
    /// The shell logs that refusal. A `make` whose own submit was refused
    /// returns a valid table, and the latched refusal is reported at commit,
    /// where the output is destroyed once.
    fn make_with<'a, T>(
        &'a self,
        mut transaction: Transaction<'a, S, M, T>,
        invoke: impl FnOnce(Pin<&Context<S>>, &mut Staging<'a, S, M, T>) -> Result<MakeOutput, Error>,
    ) -> Result<PendingInstance<'a, S, M, T>, Error> {
        let context = self.owner.as_ref().context();
        if !ptr::eq(context.get_ref(), transaction.context.get_ref()) {
            return Err(Error::Attempt);
        }
        if context.attempted.replace(true) {
            return Err(Error::Attempt);
        }
        let (status, out) = invoke(context, &mut transaction.staging)?;
        if status != host_glue::OK {
            return Err(Error::Factory(status));
        }
        // SAFETY: a successful authorized `make` supplies a stable declared
        // span; this pinned owner keeps its module and services alive.
        context.instance.set(Some(unsafe { read_output(out) }?));
        Ok(PendingInstance {
            transaction: Some(transaction),
            context,
            armed: true,
        })
    }

    /// `make_registered` and commit in one step. The only way to run `make`
    /// for registrations: without the scope a plugin's submits are refused
    /// without poisoning construction, and it could activate with none.
    ///
    /// # Errors
    /// As [`make_registered`](Self::make_registered) and
    /// [`PendingInstance::commit`].
    pub fn construct_registered<'a>(
        &'a self,
        transaction: Transaction<'a, S, M, Registration>,
    ) -> Result<Batch<'a, S, M, Registration>, Error> {
        // On a refusal the transaction drops before returning. No unvalidated
        // partial output is used; a failed `make` has no live output.
        self.make_registered(transaction)?.commit()
    }
}

/// Boxes `value` or says `Allocation`; the host is never aborted by a plugin's
/// memory use (the X4 has no room to abort).
///
/// # Errors
/// `Allocation` when the heap has no room.
pub fn try_box<T>(value: T) -> Result<Box<T>, Error> {
    let layout = Layout::new::<T>();
    if layout.size() == 0 {
        return Ok(Box::new(value));
    }
    // SAFETY: the layout is nonzero and valid for this exact type.
    let allocation = unsafe { alloc(layout) }.cast::<T>();
    if allocation.is_null() {
        return Err(Error::Allocation);
    }
    // SAFETY: a uniquely owned allocation of the exact `Box` layout,
    // initialized before the `Box` exists.
    unsafe {
        allocation.write(value);
        Ok(Box::from_raw(allocation))
    }
}

/// Fallible placement of the owner: it holds a context and a module, so its
/// allocation is never zero-sized.
fn pin_owner<S: 'static, M>(
    value: Owner<S, M>,
) -> Result<super::PinnedOwner<Context<S>, M>, Error> {
    super::PinnedOwner::try_new(value).ok_or(Error::Allocation)
}

/// A made plugin whose records are not yet dispatchable. Dropped before
/// [`commit`](Self::commit), it drains the staged records and then destroys
/// the plugin's output, exactly once.
#[allow(missing_debug_implementations, reason = "holds the generated table")]
pub struct PendingInstance<'a, S: 'static, M, T> {
    transaction: Option<Transaction<'a, S, M, T>>,
    context: Pin<&'a Context<S>>,
    armed: bool,
}

impl<'a, S: 'static, M, T> PendingInstance<'a, S, M, T> {
    /// Makes the records dispatchable; the owner then holds the destruction.
    ///
    /// # Errors
    /// `Attempt` after a first commit, or the owner's refusal (the plugin is
    /// then destroyed by the drop).
    pub fn commit(mut self) -> Result<Batch<'a, S, M, T>, Error> {
        // The pending guard owns staging so a panic or cancellation also
        // drains the records before the foreign output is destroyed.
        let transaction = self.transaction.take().ok_or(Error::Attempt)?;
        let batch = transaction.staging.commit(crate::native_host::Teardown {
            report: report_context::<S>,
            cancel: cancel_context::<S>,
            release: release_context::<S>,
            destroy: destroy_context::<S>,
        })?;
        self.armed = false;
        Ok(batch)
    }
}

// A separate local guard survives unwinding from a staged host record's Drop.
// The foreign destructor must not unwind, including during host cleanup.
struct ForeignCleanup<'a, S: 'static> {
    context: Pin<&'a Context<S>>,
    armed: bool,
}

impl<S: 'static> Drop for ForeignCleanup<'_, S> {
    fn drop(&mut self) {
        if self.armed {
            destroy_context(self.context);
        }
    }
}

impl<S: 'static, M, T> Drop for PendingInstance<'_, S, M, T> {
    fn drop(&mut self) {
        let _cleanup = ForeignCleanup {
            context: self.context,
            armed: self.armed,
        };
        self.armed = false;
        // Remaining record fields drain through the drop glue before the
        // local foreign-cleanup guard runs. A second panic aborts.
        drop(self.transaction.take());
    }
}

fn cancel_context<S: 'static>(context: Pin<&Context<S>>) -> Result<(), super::Error> {
    host_cleanup(super::TeardownStep::Cancel)?;
    context
        .resources
        .cancel_all()
        .map_err(resource_cleanup_error)
}

fn release_context<S: 'static>(context: Pin<&Context<S>>) -> Result<(), super::Error> {
    host_cleanup(super::TeardownStep::Release)?;
    context
        .resources
        .release_all()
        .map_err(resource_cleanup_error)
}

fn resource_cleanup_error(error: i32) -> super::Error {
    match error {
        host_glue::ERR_BUSY => super::Error::Busy,
        _ => super::Error::State,
    }
}

fn host_cleanup(_step: super::TeardownStep) -> Result<(), super::Error> {
    #[cfg(test)]
    return teardown_fault::run(_step);
    #[cfg(not(test))]
    Ok(())
}

/// Test-only host failures. Never present in device builds.
#[cfg(test)]
pub(crate) mod teardown_fault {
    extern crate std as test_std;
    use crate::native_host::{Error, TeardownStep};
    use core::cell::Cell;

    test_std::thread_local! {
        static FAILURE: Cell<Option<(TeardownStep, Error, bool)>> = const { Cell::new(None) };
    }

    pub(crate) fn arm(step: TeardownStep, error: Error) {
        FAILURE.with(|failure| failure.set(Some((step, error, false))));
    }

    pub(crate) fn arm_persistent(step: TeardownStep, error: Error) {
        FAILURE.with(|failure| failure.set(Some((step, error, true))));
    }

    pub(crate) fn disarm() {
        FAILURE.with(|failure| failure.set(None));
    }

    pub(super) fn run(step: TeardownStep) -> Result<(), Error> {
        FAILURE.with(|failure| match failure.get() {
            Some((wanted, error, persistent)) if wanted == step => {
                if !persistent {
                    failure.set(None);
                }
                Err(error)
            }
            _ => Ok(()),
        })
    }
}

unsafe fn report_blob_cleanup<S: 'static>(raw: *const c_void, status: i32) {
    // SAFETY: Hosted::new installed the exact pinned Context<S> pointer.
    let context = unsafe { &*raw.cast::<Context<S>>() };
    let error = if status == host_glue::ERR_BUSY {
        super::Error::Busy
    } else {
        super::Error::State
    };
    (context.report)(
        &context.services,
        context.plugin_id(),
        super::TeardownStep::Release,
        error,
    );
}

fn report_context<S: 'static>(
    context: Pin<&Context<S>>,
    step: super::TeardownStep,
    error: super::Error,
) {
    (context.report)(&context.services, context.plugin_id(), step, error);
}

fn destroy_context<S: 'static>(context: Pin<&Context<S>>) {
    if let Some(instance) = context.instance.take() {
        // SAFETY: validated successful output has exactly one drop obligation;
        // its pinned context, table, services and module are all still alive.
        context.run_plugin(|| unsafe { (instance.destroy)(instance.state) });
    }
}

/// Decodes the typed context and forwards through a temporary `ShellCtx`.
/// The plugin keeps only the pinned `Context<S>` address; no `ShellCtx`
/// pointer escapes this call.
unsafe fn with_shell<S: Services + 'static>(
    ctx: *mut c_void,
    call: impl FnOnce(*mut c_void) -> i32,
) -> i32 {
    // SAFETY: every nonnull ctx is the pinned Context<S> supplied by make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    let mut shell = ShellCtx::new(&context.services);
    call(ptr::from_mut(&mut shell).cast())
}

unsafe extern "C" fn heap_alloc<S: Services + 'static>(
    ctx: *mut c_void,
    size: usize,
    align: usize,
    out: *mut *mut c_void,
) -> i32 {
    // SAFETY: typed context and the generated heap.alloc pointer contract.
    unsafe {
        heap_table::with_heap::<S>(ctx, |shell| host_glue::heap_alloc(shell, size, align, out))
    }
}

unsafe extern "C" fn heap_dealloc<S: Services + 'static>(
    ctx: *mut c_void,
    block: *mut c_void,
    size: usize,
    align: usize,
) -> i32 {
    // SAFETY: typed context and the generated heap.dealloc allocation contract.
    unsafe {
        heap_table::with_heap::<S>(ctx, |shell| {
            host_glue::heap_dealloc(shell, block, size, align)
        })
    }
}

unsafe extern "C" fn heap_realloc<S: Services + 'static>(
    ctx: *mut c_void,
    block: *mut c_void,
    old_size: usize,
    align: usize,
    new_size: usize,
    out: *mut *mut c_void,
) -> i32 {
    // SAFETY: typed context and the generated heap.realloc allocation contract.
    unsafe {
        heap_table::with_heap::<S>(ctx, |shell| {
            host_glue::heap_realloc(shell, block, old_size, align, new_size, out)
        })
    }
}

unsafe extern "C" fn clock_micros<S: Services + 'static>(ctx: *mut c_void, out: *mut u64) -> i32 {
    // SAFETY: typed context and the generated clock.micros output contract.
    unsafe { with_shell::<S>(ctx, |shell| host_glue::clock_micros(shell, out)) }
}

unsafe extern "C" fn registrations_submit<S: Services + 'static>(
    ctx: *mut c_void,
    batch: PpBytes,
) -> i32 {
    // SAFETY: a nonnull context is the pinned Context<S> supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: the foreign caller supplies stable readable batch bytes for this
    // synchronous call. Core copies callback records, owns its snapshots of
    // declarations, and borrows image storage only through the image range
    // given to `set_image`; it never keeps the input pointer itself.
    unsafe { registrations::submit(&context.registrations, batch.ptr, batch.len) }
}

unsafe extern "C" fn env_snapshot<S: 'static>(ctx: *mut c_void, out: PpBuffer) -> i32 {
    // SAFETY: a nonnull context is exactly the pinned Context<S> given to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: generated slot 4 requires exclusive writable output, disjoint
    // from host state and borrowed inputs. The scope keeps no output pointer.
    unsafe { context.environment.copy(out.ptr, out.cap) }
}

unsafe extern "C" fn settings_read<S: 'static>(
    ctx: *mut c_void,
    key: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: a nonnull context is exactly the pinned Context<S> given to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    let caller = match context.resources.bound_caller() {
        Ok(caller) => caller,
        Err(error) => return error,
    };
    // SAFETY: generated slot 5 requires initialized immutable readable key
    // bytes and exclusive output, disjoint from key and host state. No pointer
    // is kept. The backend's required-length and prefix semantics are kept.
    unsafe { context.environment.settings_read_for(caller, key, out) }
}

/// Slot 9 (`canvas.fill_rect`): the foreign callback reaches [`CanvasScope`]
/// only inside one synchronous draw dispatch. Colour is generated
/// `0x00RRGGBB`; the scalars are forwarded unchanged.
unsafe extern "C" fn canvas_fill_rect<S: 'static>(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: u32,
) -> i32 {
    // SAFETY: a nonnull context is exactly the pinned Context<S> given to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    context.canvas.fill_rect_raw(x, y, width, height, color)
}

/// What a dispatch hands to the callback.
#[derive(Debug, Clone, Copy)]
pub enum Invocation<'a> {
    /// The tick event, its payload encoded by core.
    Tick(&'a [u8]),
    /// An event of interface 7's list, its payload encoded by core
    /// (`native_host::events`): handed to the callback as it is.
    Event(&'a [u8]),
    /// The draw callback with its encoded `draw.frame` record (N2).
    Draw(&'a [u8]),
}

fn borrowed(bytes: &[u8]) -> PpBytes {
    PpBytes {
        ptr: bytes.as_ptr(),
        len: bytes.len(),
    }
}

/// Events 32-39 and tick have no reply family: an empty, non-null reply
/// buffer (cap 0, `docs/abi/screens-input.md` section 6).
fn no_reply() -> PpBuffer {
    PpBuffer {
        ptr: ptr::NonNull::dangling().as_ptr(),
        cap: 0,
    }
}

/// Callbacks without a reply family return a status: a positive result is a
/// plugin fault, never a length (`docs/abi/screens-input.md` section 6).
fn status_only(result: i32) -> i32 {
    if result > 0 {
        host_glue::ERR_INVALID
    } else {
        result
    }
}

/// Called only inside a batch admission closure that retains its owner.
fn invoke_registration<S: 'static>(
    context: Pin<&Context<S>>,
    record: Registration,
    invocation: Invocation<'_>,
) -> Result<i32, Error> {
    let instance = context.instance.get().ok_or(Error::Attempt)?;
    match (record, invocation) {
        (
            Registration::Event {
                event_tag: EVENT_TICK_TAG,
                token,
            },
            Invocation::Tick(bytes),
        ) => {
            registrations::tick_decode(bytes).map_err(Error::Status)?;
            let callback = instance
                .event
                .ok_or(Error::Status(host_glue::ERR_UNSUPPORTED))?;
            // SAFETY: the admitted owner retains state, context and module. The
            // callback is present, the bytes live for this call and do not
            // alias host state.
            Ok(status_only(context.run_plugin(|| unsafe {
                callback(instance.state, token, borrowed(bytes), no_reply())
            })))
        }
        (Registration::Event { token, .. }, Invocation::Event(bytes)) => {
            let callback = instance
                .event
                .ok_or(Error::Status(host_glue::ERR_UNSUPPORTED))?;
            // SAFETY: the same admitted owner lifetime; the payload is
            // borrowed for this call only and does not alias host state.
            Ok(status_only(context.run_plugin(|| unsafe {
                callback(instance.state, token, borrowed(bytes), no_reply())
            })))
        }
        (Registration::Draw { token }, Invocation::Draw(frame)) => {
            let callback = instance
                .draw
                .ok_or(Error::Status(host_glue::ERR_UNSUPPORTED))?;
            // SAFETY: the same admitted owner lifetime; the frame is borrowed
            // for this call only and does not alias host state.
            Ok(status_only(context.run_plugin(|| unsafe {
                callback(instance.state, token, borrowed(frame))
            })))
        }
        _ => Err(Error::Status(host_glue::ERR_UNSUPPORTED)),
    }
}

#[path = "shell_doc_slots.rs"]
mod doc_slots;
#[path = "shell_storage.rs"]
mod storage_externs;
#[path = "shell_tokens.rs"]
mod tokens;
pub(crate) use tokens::dispatch_token_staged;
#[allow(
    unused_imports,
    reason = "native built-in adapter follows this prerequisite"
)]
pub(crate) use tokens::{TokenInvocation, dispatch_token, dispatch_token_draw};

/// Dispatches record `index` without a frame.
///
/// # Errors
/// The owner's refusal, or the callback's absence.
pub fn dispatch_registration<S: 'static, M>(
    batch: &Published<'_, S, M, Registration>,
    index: usize,
    depth: &DispatchDepth,
    invocation: Invocation<'_>,
) -> Result<i32, Error> {
    batch.dispatch(index, depth, |context, record| {
        invoke_registration(context, *record, invocation)
    })?
}

/// Dispatches record `index` with `env` lent for this one call; cleared on
/// every exit, including unwind.
///
/// # Errors
/// The owner's or the scope's refusal, or the callback's absence.
pub fn dispatch_with_env<S: 'static, M>(
    batch: &Published<'_, S, M, Registration>,
    index: usize,
    depth: &DispatchDepth,
    invocation: Invocation<'_>,
    env: &Env<'_>,
) -> Result<i32, Error> {
    batch.dispatch(index, depth, |context, record| {
        // SAFETY: the owner structurally pins this field; no API moves it.
        let scope = unsafe { context.map_unchecked(|value| &value.environment) };
        scope.scope(env, || invoke_registration(context, *record, invocation))?
    })?
}

/// The screen frame (N2) of a canvas's visible region, encoded into `out`;
/// `None` when the region is empty, which is never sent.
fn screen_frame(clip: Rect, out: &mut [u8; 64]) -> Result<Option<usize>, Error> {
    if clip.width == 0 || clip.height == 0 {
        return Ok(None);
    }
    let (top, height) = (i32::from(clip.y), i32::from(clip.height));
    let frame = super::frames::screen(
        i32::from(clip.x),
        top,
        u32::from(clip.width),
        u32::from(clip.height),
        (top, top + height),
    );
    super::frames::encode(&frame, out)
        .map(Some)
        .map_err(Error::Status)
}

/// Draw dispatch with a frame: the Env (slots 4/5) and the Canvas (slot 9)
/// are both lent for this one synchronous callback and cleared on every exit,
/// including unwind. Nested installation is refused by the scopes.
///
/// # Errors
/// The owner's or the scopes' refusal, or the callback's absence.
pub fn dispatch_draw<S: 'static, M>(
    batch: &Published<'_, S, M, Registration>,
    index: usize,
    depth: &DispatchDepth,
    env: &Env<'_>,
    canvas: &mut Canvas<'_>,
) -> Result<i32, Error> {
    let mut bytes = [0; 64];
    // An empty clip is never sent (section 6): nothing visible, no call.
    let Some(len) = screen_frame(canvas.clip(), &mut bytes)? else {
        return Ok(host_glue::OK);
    };
    let frame = &bytes[..len];
    batch.dispatch(index, depth, |context, record| {
        // SAFETY: the owner structurally pins these fields; no API moves them.
        let env_scope = unsafe { context.map_unchecked(|value| &value.environment) };
        let canvas_scope = unsafe { context.map_unchecked(|value| &value.canvas) };
        env_scope.scope(env, || {
            canvas_scope.scope(canvas, || {
                invoke_registration(context, *record, Invocation::Draw(frame))
            })
        })??
    })?
}

/// One hosted, made and committed plugin as the kernel's `FlatInstance`.
#[path = "shell_instance.rs"]
pub mod instance;

#[cfg(test)]
#[path = "shell_frame_tests.rs"]
mod frame_tests;
#[cfg(test)]
#[path = "shell_tests.rs"]
pub(in crate::native_host) mod tests;

/// The services a plugin declares, checked against the host's catalog before
/// its image is placed.
#[path = "shell_admission.rs"]
pub mod admission;

/// Which plugin's code runs now, kept where a reset leaves it.
#[path = "shell_inflight.rs"]
pub mod inflight;

unsafe extern "C" fn i18n_copy<S: 'static>(ctx: *mut c_void, key: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe { super::formatting::copy(&context.environment, key, out) }
}

unsafe extern "C" fn i18n_format<S: 'static>(
    ctx: *mut c_void,
    key: PpBytes,
    args: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe { super::formatting::format(&context.environment, key, args, out) }
}

unsafe extern "C" fn i18n_digits<S: 'static>(
    ctx: *mut c_void,
    text: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: generated slot contracts supply live, disjoint input/output pairs.
    unsafe { super::formatting::digits(&context.environment, text, out) }
}

unsafe extern "C" fn canvas_draw_line<S: 'static>(
    ctx: *mut c_void,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    width: u32,
    color: u32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    context.canvas.draw_line(x0, y0, x1, y1, width, color)
}

unsafe extern "C" fn canvas_fill_round<S: 'static>(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    radius: u32,
    color: u32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    context
        .canvas
        .fill_round(x, y, width, height, radius, color)
}

unsafe extern "C" fn canvas_stroke_round<S: 'static>(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    radius: u32,
    thickness: u32,
    color: u32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    context
        .canvas
        .stroke_round(x, y, width, height, radius, thickness, color)
}

unsafe extern "C" fn canvas_clip<S: 'static>(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    context.canvas.clip(x, y, width, height)
}

/// Slot 42: the label is at most 256 bytes of UTF-8 (`screens-input.md` 6.1).
const DESCRIBE_LABEL_MAX: usize = 256;

#[allow(clippy::too_many_arguments, reason = "matches the ABI scalar slot")]
unsafe extern "C" fn canvas_describe<S: 'static>(
    ctx: *mut c_void,
    role: u32,
    flags: u32,
    label: PpBytes,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: the slot contract supplies live readable input for the call.
    match unsafe { host_glue::buffers::text_in(label, DESCRIBE_LABEL_MAX) } {
        Ok(label) => context
            .canvas
            .describe(role, flags, label, x, y, width, height),
        Err(error) => error,
    }
}

unsafe extern "C" fn font_acquire<S: 'static>(
    ctx: *mut c_void,
    key: PpBytes,
    out: *mut u32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: slot contracts supply live, disjoint input and output storage.
    unsafe { super::fonts::acquire(&context.environment, &context.resources, key, out) }
}

unsafe extern "C" fn handle_release<S: 'static>(ctx: *mut c_void, handle: u32) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    super::fonts::release(&context.resources, handle)
}

unsafe extern "C" fn text_measure<S: 'static>(
    ctx: *mut c_void,
    handle: u32,
    size: f32,
    text: PpBytes,
    out: *mut f32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: slot contracts supply live, disjoint input and output storage.
    unsafe {
        super::fonts::measure(
            &context.environment,
            &context.resources,
            handle,
            size,
            text,
            out,
        )
    }
}

unsafe extern "C" fn text_metrics<S: 'static>(
    ctx: *mut c_void,
    handle: u32,
    size: f32,
    ascent: *mut f32,
    descent: *mut f32,
    height: *mut f32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: slot contracts supply live, disjoint input and output storage.
    unsafe {
        super::fonts::metrics(
            &context.environment,
            &context.resources,
            handle,
            size,
            ascent,
            descent,
            height,
        )
    }
}

#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
#[allow(clippy::too_many_arguments, reason = "matches the ABI scalar slot")]
unsafe extern "C" fn text_ink<S: 'static>(
    ctx: *mut c_void,
    handle: u32,
    size: f32,
    text: PpBytes,
    top: *mut i32,
    bottom: *mut i32,
    left: *mut i32,
    right: *mut i32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: slot contracts supply live, disjoint input and output storage.
    unsafe {
        super::fonts::ink(
            &context.environment,
            &context.resources,
            handle,
            size,
            text,
            top,
            bottom,
            left,
            right,
        )
    }
}

#[allow(clippy::too_many_arguments, reason = "matches the ABI scalar slot")]
unsafe extern "C" fn text_draw<S: 'static>(
    ctx: *mut c_void,
    handle: u32,
    size: f32,
    x: f32,
    baseline: i32,
    color: u32,
    text: PpBytes,
    out: *mut f32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> provided to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: slot contracts supply live, disjoint input and output storage.
    unsafe {
        super::fonts::draw(
            &context.environment,
            &context.resources,
            &context.canvas,
            handle,
            size,
            x,
            baseline,
            color,
            text,
            out,
        )
    }
}

#[cfg(test)]
#[path = "shell_token_tests.rs"]
mod token_tests;
unsafe extern "C" fn services_query<S: 'static>(
    ctx: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: slot contracts supply live, disjoint input and output storage.
    unsafe {
        super::service_slots::query(
            &context.environment,
            &context.resources,
            &context.declared_services,
            key,
            major,
            minor,
            out,
        )
    }
}
unsafe extern "C" fn blob_read<S: 'static>(
    ctx: *mut c_void,
    blob: u32,
    offset: u64,
    out: PpBuffer,
) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: slot contracts supply live, disjoint output storage.
    unsafe {
        super::service_slots::read(&context.environment, &context.resources, blob, offset, out)
    }
}
unsafe extern "C" fn blob_release<S: 'static>(ctx: *mut c_void, blob: u32) -> i32 {
    // SAFETY: nonnull ctx is the pinned Context<S> supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    super::service_slots::release(&context.environment, &context.resources, blob)
}

#[cfg(test)]
#[path = "shell_commands.rs"]
mod command_dispatch;

#[cfg(test)]
#[path = "shell_core.rs"]
mod core_dispatch;

unsafe extern "C" fn commands_submit<S: 'static>(ctx: *mut c_void, batch: PpBytes) -> i32 {
    // SAFETY: nonnull ctx is the pinned host context supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: the slot contract supplies stable, readable batch bytes.
    unsafe { context.commands.submit(batch.ptr, batch.len) }
}

#[path = "shell_stage_slot.rs"]
mod stage_slot;
#[path = "shell_text_covers.rs"]
mod text_covers;
unsafe extern "C" fn service_open<S: 'static>(
    ctx: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: a nonnull context is the pinned Context<S> supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: generated slot contracts supply live and disjoint pairs.
    unsafe {
        super::call_services::open(
            &context.environment,
            &context.resources,
            &context.declared_services,
            key,
            major,
            minor,
            out,
        )
    }
}

unsafe extern "C" fn service_call<S: 'static>(
    ctx: *mut c_void,
    service: u32,
    op: u32,
    input: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: a nonnull context is the pinned Context<S> supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // The plugin waits for the host from here to the return.
    let _slot = super::host_slot::enter();
    // SAFETY: generated slot contracts supply live and disjoint pairs.
    unsafe {
        super::call_services::call(
            &context.environment,
            &context.resources,
            context.plugin_id(),
            service,
            op,
            input,
            out,
        )
    }
}

#[path = "shell_stage.rs"]
mod stage;
#[allow(
    unused_imports,
    reason = "stage broker consumes this checked reply hook next"
)]
pub(crate) use stage::dispatch_stage;

#[path = "shell/heap_table.rs"]
mod heap_table;
#[path = "shell/native_commit.rs"]
mod native_commit;
#[path = "shell/shared_table.rs"]
mod shared_table;
pub use heap_table::{handed_heap, heap_table};
use shared_table::SharedTable;
pub use shared_table::{handed_table, same_table};
