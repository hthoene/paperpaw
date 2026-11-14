//! Scoped owner integration preparation. No production loader calls this module.
//!
//! The library is private to the pinned owner. Batches and queues borrow it from
//! outside; no self-reference, lifetime extension or process-owned service leak.

use super::{CheckedLibrary, Make, RuntimeError, checked_library, plugin_output};
use crate::native::HeldLibrary;
use paperpaw_core::native_host::canvas::CanvasScope;
use paperpaw_core::native_host::environment::EnvironmentScope;
use paperpaw_core::native_host::registration_scope::RegistrationScope;
use paperpaw_core::native_host::registrations::{self, EVENT_TICK_TAG, Registration};
use paperpaw_core::native_host::{self, Construction, DispatchDepth, HostOwner, RegistrationBatch};
use paperpaw_native::host_glue::{self, PpBuffer, PpBytes, PpHost, PpPlugin, Services, ShellCtx};
use std::{cell::Cell, ffi::c_void, path::Path, pin::Pin, ptr};

#[derive(Debug)]
enum Error {
    Runtime(RuntimeError),
    Lifecycle(native_host::Error),
    Allocation,
    Attempt,
    Status(i32),
}

impl From<RuntimeError> for Error {
    fn from(value: RuntimeError) -> Self {
        Self::Runtime(value)
    }
}

impl From<native_host::Error> for Error {
    fn from(value: native_host::Error) -> Self {
        Self::Lifecycle(value)
    }
}

type Instance = plugin_output::PluginOutput;

struct Context<S> {
    table: PpHost,
    services: S,
    instance: Cell<Option<Instance>>,
    attempted: Cell<bool>,
    registrations: RegistrationScope<Registration>,
    environment: EnvironmentScope,
    canvas: CanvasScope,
}

type Owner<S> = HostOwner<Context<S>, HeldLibrary>;
type Staging<'a, S, T> = Construction<'a, Context<S>, HeldLibrary, T>;
type MakeOutput = (i32, *const PpPlugin);
struct Transaction<'a, S, T> {
    context: Pin<&'a Context<S>>,
    staging: Staging<'a, S, T>,
}

impl<'a, S, T> Transaction<'a, S, T> {
    fn stage(&mut self, record: T) -> Result<(), (native_host::Error, T)> {
        self.staging.stage(record)
    }

    /// Gives staging the library's read-only data before `make`: a batch
    /// lying wholly inside it is kept as offsets, not copied.
    fn set_image(&mut self, image: &'a [u8]) -> Result<(), native_host::Error> {
        self.staging.set_image(image)
    }
}
type Batch<'a, S, T> = RegistrationBatch<'a, Context<S>, HeldLibrary, T>;

struct PinnedPlugin<S> {
    owner: native_host::PinnedOwner<Context<S>, HeldLibrary>,
    make: Make,
    /// Host-known plugin id (from the folder, never from the library) that
    /// staged declaration names are checked against.
    id: Box<str>,
    /// The library's read-only data as mapped: address and length. Valid
    /// while the owner holds the library; see [`PinnedPlugin::image`].
    image: (usize, usize),
}

impl<S: Services> PinnedPlugin<S> {
    /// The caller authorizes native execution and keeps the checked inode
    /// immutable through unload, as required by native_runtime::load.
    unsafe fn open(path: &Path, digest: &[u8; 32], id: &str, services: S) -> Result<Self, Error> {
        let mut table = host_glue::host_table();
        table.heap_alloc = heap_alloc::<S>;
        table.heap_dealloc = heap_dealloc::<S>;
        table.heap_realloc = heap_realloc::<S>;
        table.clock_micros = clock_micros::<S>;
        table.registrations_submit = registrations_submit::<S>;
        table.env_snapshot = environment::snapshot::<S>;
        table.settings_read = environment::settings_read::<S>;
        table.canvas_fill_rect = canvas::fill_rect::<S>;
        // SAFETY: the caller supplies authorization and inode immutability.
        let CheckedLibrary {
            library,
            make,
            image,
            ..
        } = unsafe { checked_library(path, digest, &table, &[], super::WIRED_FEATURES) }?;
        let context = Context {
            table,
            services,
            instance: Cell::new(None),
            attempted: Cell::new(false),
            registrations: RegistrationScope::new(),
            environment: EnvironmentScope::new(),
            canvas: CanvasScope::new(),
        };
        let owner = pin_owner(HostOwner::new(context, library))?;
        Ok(Self {
            owner,
            make,
            id: id.into(),
            image,
        })
    }

    /// The library's read-only data, as mapped.
    ///
    /// # Safety
    ///
    /// The returned slice must not outlive the owner's library: the caller
    /// ties it to a borrow of the plugin that holds the owner, and gives no
    /// slice once the plugin is stopped. The bytes are read-only mappings; a
    /// plugin without a sandbox can remap them, so every reader decodes with
    /// checks and never trusts them to stay as they were.
    unsafe fn image(&self) -> &[u8] {
        let (start, len) = self.image;
        if len == 0 {
            return &[];
        }
        // SAFETY: a read-only mapping of `len` bytes at `start`, held by the
        // owner's library for as long as `self` lives (the caller's contract).
        unsafe { std::slice::from_raw_parts(start as *const u8, len) }
    }

    fn begin<T>(&self) -> Result<Transaction<'_, S, T>, Error> {
        Ok(Transaction {
            context: self.owner.as_ref().context(),
            staging: self.owner.as_ref().begin(&self.id)?,
        })
    }

    fn make<'a, T>(
        &'a self,
        transaction: Transaction<'a, S, T>,
    ) -> Result<PendingInstance<'a, S, T>, Error> {
        self.make_with(transaction, |context, _| Ok(self.invoke_make(context)))
    }

    /// The scope borrows staging only while make runs, and resets before output
    /// validation or PendingInstance can transfer its destruction obligation.
    fn make_registered<'a>(
        &'a self,
        transaction: Transaction<'a, S, Registration>,
    ) -> Result<PendingInstance<'a, S, Registration>, Error> {
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
        // SAFETY: owner is pinned, owns services and module, and out is writable.
        let status = unsafe { (self.make)(&context.table, ctx, &mut out) };
        (status, out)
    }

    /// Output stays loader-owned until commit Ok. Failed factories return no
    /// live output. No unvalidated partial-output destructor is invoked here.
    fn make_with<'a, T>(
        &'a self,
        mut transaction: Transaction<'a, S, T>,
        invoke: impl FnOnce(Pin<&Context<S>>, &mut Staging<'a, S, T>) -> Result<MakeOutput, Error>,
    ) -> Result<PendingInstance<'a, S, T>, Error> {
        let context = self.owner.as_ref().context();
        if !ptr::eq(context.get_ref(), transaction.context.get_ref()) {
            return Err(Error::Attempt);
        }
        if context.attempted.replace(true) {
            return Err(Error::Attempt);
        }
        let (status, out) = invoke(context, &mut transaction.staging)?;
        if status != host_glue::OK {
            return Err(RuntimeError::Factory(status).into());
        }
        // SAFETY: successful authorized make supplies a stable declared span;
        // this pinned owner keeps its module and service dependencies alive.
        context
            .instance
            .set(Some(unsafe { plugin_output::read(out) }?));
        Ok(PendingInstance {
            transaction: Some(transaction),
            context,
            armed: true,
        })
    }

    fn construct<'a, T>(
        &'a self,
        transaction: Transaction<'a, S, T>,
    ) -> Result<Batch<'a, S, T>, Error> {
        // On make refusal, transaction drops before returning. No unvalidated
        // partial output is used; failed make has no live output by contract.
        self.make(transaction)?.commit()
    }
}

/// Fallible allocation uses the emulator's global budget allocator. The owner
/// includes a module and context, so its allocation is never zero-sized.
fn pin_owner<S>(
    value: Owner<S>,
) -> Result<native_host::PinnedOwner<Context<S>, HeldLibrary>, Error> {
    native_host::PinnedOwner::try_new(value).ok_or(Error::Allocation)
}

struct PendingInstance<'a, S, T> {
    transaction: Option<Transaction<'a, S, T>>,
    context: Pin<&'a Context<S>>,
    armed: bool,
}

impl<'a, S, T> PendingInstance<'a, S, T> {
    fn commit(mut self) -> Result<Batch<'a, S, T>, Error> {
        // The pending guard owns staging so panic or cancellation also drains
        // records before foreign output destruction. It is present until here.
        let transaction = self.transaction.take().ok_or(Error::Attempt)?;
        let batch = transaction
            .staging
            .commit(paperpaw_core::native_host::Teardown {
                report: report_teardown::<S>,
                cancel: |_| Ok(()),
                release: |_| Ok(()),
                destroy: destroy_context::<S>,
            })?;
        self.armed = false;
        Ok(batch)
    }
}

// A separate local guard survives unwinding from a staged host record's Drop.
// Its foreign destructor must not unwind, including during host cleanup.
struct ForeignCleanup<'a, S> {
    context: Pin<&'a Context<S>>,
    armed: bool,
}

impl<S> Drop for ForeignCleanup<'_, S> {
    fn drop(&mut self) {
        if self.armed {
            destroy_context(self.context);
        }
    }
}

impl<S, T> Drop for PendingInstance<'_, S, T> {
    fn drop(&mut self) {
        let _cleanup = ForeignCleanup {
            context: self.context,
            armed: self.armed,
        };
        self.armed = false;
        // Remaining record fields also drain through Rust's unwind drop glue
        // before the local foreign-cleanup guard runs. A second panic aborts.
        drop(self.transaction.take());
    }
}

fn report_teardown<S>(
    context: Pin<&Context<S>>,
    step: native_host::TeardownStep,
    error: native_host::Error,
) {
    eprintln!(
        "abi7 owner {:p}: fallback {step:?} teardown error: {error:?}",
        context.get_ref()
    );
}

fn destroy_context<S>(context: Pin<&Context<S>>) {
    if let Some(instance) = context.instance.take() {
        // SAFETY: validated successful output has exactly one drop obligation;
        // its pinned context, table, services and module are all still alive.
        unsafe { (instance.destroy)(instance.state) };
    }
}

/// Decode the concrete typed context and forward through a temporary ShellCtx.
/// The plugin retains only Context<S>'s pinned address. No ShellCtx pointer
/// escapes this call, and no CoreCtx prefix layout is assumed.
unsafe fn with_shell<S: Services>(ctx: *mut c_void, call: impl FnOnce(*mut c_void) -> i32) -> i32 {
    // SAFETY: every nonnull ctx is the pinned Context<S> supplied by make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    let mut shell = ShellCtx::new(&context.services);
    call(ptr::from_mut(&mut shell).cast())
}

unsafe extern "C" fn heap_alloc<S: Services>(
    ctx: *mut c_void,
    size: usize,
    align: usize,
    out: *mut *mut c_void,
) -> i32 {
    // SAFETY: typed context and generated heap.alloc pointer contract.
    unsafe { with_shell::<S>(ctx, |shell| host_glue::heap_alloc(shell, size, align, out)) }
}

unsafe extern "C" fn heap_dealloc<S: Services>(
    ctx: *mut c_void,
    block: *mut c_void,
    size: usize,
    align: usize,
) -> i32 {
    // SAFETY: typed context and generated heap.dealloc allocation contract.
    unsafe {
        with_shell::<S>(ctx, |shell| {
            host_glue::heap_dealloc(shell, block, size, align)
        })
    }
}

unsafe extern "C" fn heap_realloc<S: Services>(
    ctx: *mut c_void,
    block: *mut c_void,
    old_size: usize,
    align: usize,
    new_size: usize,
    out: *mut *mut c_void,
) -> i32 {
    // SAFETY: typed context and generated heap.realloc allocation contract.
    unsafe {
        with_shell::<S>(ctx, |shell| {
            host_glue::heap_realloc(shell, block, old_size, align, new_size, out)
        })
    }
}

unsafe extern "C" fn clock_micros<S: Services>(ctx: *mut c_void, out: *mut u64) -> i32 {
    // SAFETY: typed context and generated clock.micros output contract.
    unsafe { with_shell::<S>(ctx, |shell| host_glue::clock_micros(shell, out)) }
}

unsafe extern "C" fn registrations_submit<S: Services>(ctx: *mut c_void, batch: PpBytes) -> i32 {
    // SAFETY: a nonnull context is the pinned Context<S> supplied to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // SAFETY: the foreign caller supplies stable readable batch bytes for this
    // synchronous call. Core retains only decoded scalar records, never input.
    unsafe { registrations::submit(&context.registrations, batch.ptr, batch.len) }
}

enum Invocation<'a> {
    Tick(&'a [u8]),
    /// An encoded event and the caller's exclusive borrowed reply buffer.
    Event(&'a [u8], &'a mut [u8]),
    /// One encoded `draw.frame` batch (N2).
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

/// Called only inside a batch/queue admission closure that retains its owner.
fn invoke_registration<S>(
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
            // SAFETY: admitted owner retains state/context/module. The callback
            // is present, bytes remain live for this call and do not alias host state.
            Ok(status_only(unsafe {
                callback(instance.state, token, borrowed(bytes), no_reply())
            }))
        }
        (Registration::Event { token, .. }, Invocation::Event(bytes, out)) => {
            let callback = instance
                .event
                .ok_or(Error::Status(host_glue::ERR_UNSUPPORTED))?;
            let reply = PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            };
            // SAFETY: admission retains the owner; input/output are disjoint
            // borrowed slices, exclusively writable for the lent output span.
            let status = unsafe { callback(instance.state, token, borrowed(bytes), reply) };
            Ok(
                if status >= 0 && usize::try_from(status).is_ok_and(|len| len > out.len()) {
                    host_glue::ERR_INVALID
                } else {
                    status
                },
            )
        }
        (Registration::Draw { token }, Invocation::Draw(frame)) => {
            let callback = instance
                .draw
                .ok_or(Error::Status(host_glue::ERR_UNSUPPORTED))?;
            // SAFETY: same admitted owner lifetime; the frame is borrowed for this call.
            Ok(status_only(unsafe {
                callback(instance.state, token, borrowed(frame))
            }))
        }
        _ => Err(Error::Status(host_glue::ERR_UNSUPPORTED)),
    }
}

fn dispatch_registration<S>(
    batch: &Batch<'_, S, Registration>,
    index: usize,
    depth: &DispatchDepth,
    invocation: Invocation<'_>,
) -> Result<i32, Error> {
    batch.dispatch(index, depth, |context, record| {
        invoke_registration(context, *record, invocation)
    })?
}

#[cfg(test)]
mod tests;

mod canvas;
pub(super) mod dev;
mod environment;
