//! Scoped owner integration preparation. No production loader calls this module.
//!
//! The library is private to the pinned owner. Batches and queues borrow it from
//! outside; no self-reference, lifetime extension or process-owned service leak.

use super::{CheckedLibrary, Make, RuntimeError, checked_library, plugin_output};
use crate::native::HeldLibrary;
use paperpaw_core::native_host::{self, Construction, HostOwner, RegistrationBatch};
use paperpaw_native::host_glue::{self, PpHost, Services, ShellCtx};
use std::{alloc::Layout, cell::Cell, ffi::c_void, path::Path, pin::Pin, ptr};

#[derive(Debug)]
enum Error {
    Runtime(RuntimeError),
    Lifecycle(native_host::Error),
    Allocation,
    Attempt,
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
}

type Owner<S> = HostOwner<Context<S>, HeldLibrary>;
struct Transaction<'a, S, T> {
    context: Pin<&'a Context<S>>,
    staging: Construction<'a, Context<S>, HeldLibrary, T>,
}

impl<S, T> Transaction<'_, S, T> {
    fn stage(&mut self, record: T) -> Result<(), (native_host::Error, T)> {
        self.staging.stage(record)
    }
}
type Batch<'a, S, T> = RegistrationBatch<'a, Context<S>, HeldLibrary, T>;

struct PinnedPlugin<S> {
    owner: Pin<Box<Owner<S>>>,
    make: Make,
}

impl<S: Services> PinnedPlugin<S> {
    /// The caller authorizes native execution and keeps the checked inode
    /// immutable through unload, as required by native_runtime::load.
    unsafe fn open(path: &Path, digest: &[u8; 32], services: S) -> Result<Self, Error> {
        let mut table = host_glue::host_table();
        table.heap_alloc = heap_alloc::<S>;
        table.heap_dealloc = heap_dealloc::<S>;
        table.heap_realloc = heap_realloc::<S>;
        table.clock_micros = clock_micros::<S>;
        // SAFETY: the caller supplies authorization and inode immutability.
        let CheckedLibrary { library, make } = unsafe { checked_library(path, digest, &table) }?;
        let context = Context {
            table,
            services,
            instance: Cell::new(None),
            attempted: Cell::new(false),
        };
        let owner = pin_owner(HostOwner::new(context, library))?;
        Ok(Self { owner, make })
    }

    fn begin<T>(&self) -> Result<Transaction<'_, S, T>, Error> {
        Ok(Transaction {
            context: self.owner.as_ref().context(),
            staging: self.owner.as_ref().begin()?,
        })
    }

    /// Output remains loader-owned until PendingInstance::commit returns Ok.
    /// Failed factories must return no live output. Malformed output never
    /// authorizes reading or calling an unvalidated destructor.
    fn make<'a, T>(
        &'a self,
        transaction: Transaction<'a, S, T>,
    ) -> Result<PendingInstance<'a, S, T>, Error> {
        let context = self.owner.as_ref().context();
        if !ptr::eq(context.get_ref(), transaction.context.get_ref()) {
            return Err(Error::Attempt);
        }
        if context.attempted.replace(true) {
            return Err(Error::Attempt);
        }
        let mut out = ptr::null();
        let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast();
        // SAFETY: owner is pinned, owns services and module, and out is writable.
        let status = unsafe { (self.make)(&context.table, ctx, &mut out) };
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
fn pin_owner<S>(value: Owner<S>) -> Result<Pin<Box<Owner<S>>>, Error> {
    let layout = Layout::new::<Owner<S>>();
    // SAFETY: layout is nonzero and valid for this exact owner type.
    let allocation = unsafe { std::alloc::alloc(layout) }.cast::<Owner<S>>();
    if allocation.is_null() {
        return Err(Error::Allocation);
    }
    // SAFETY: uniquely owned allocation of the exact Box layout, initialized
    // before creating its Box. Nothing has observed the unpinned owner yet.
    unsafe {
        allocation.write(value);
        Ok(Box::into_pin(Box::from_raw(allocation)))
    }
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
        let batch = transaction.staging.commit(destroy_context::<S>)?;
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

#[cfg(test)]
mod tests;
