//! Typed environment bridges for the inactive owner integration.
use super::*;
use paperpaw_core::sdk::Env;
use paperpaw_native::host_glue::PpBuffer;

pub(super) unsafe extern "C" fn snapshot<S>(ctx: *mut c_void, out: PpBuffer) -> i32 {
    // SAFETY: a nonnull context is exactly the pinned Context<S> given to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // SAFETY: generated slot 4 requires exclusive writable output, disjoint
    // from host state and borrowed inputs. The scope retains no output pointer.
    unsafe { context.environment.copy(out.ptr, out.cap) }
}

pub(super) unsafe extern "C" fn settings_read<S>(
    ctx: *mut c_void,
    key: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: a nonnull context is exactly the pinned Context<S> given to make.
    let Some(context) = (unsafe { ctx.cast::<Context<S>>().as_ref() }) else {
        return host_glue::ERR_INVALID;
    };
    // SAFETY: generated slot 5 requires initialized immutable readable key
    // bytes and exclusive output, disjoint from key and host state. No pointer
    // is retained. Preserve the backend's required-length and prefix semantics.
    unsafe {
        context
            .environment
            .settings_read(key.ptr, key.len, out.ptr, out.cap)
    }
}

pub(super) fn invoke<S>(
    context: Pin<&Context<S>>,
    record: Registration,
    invocation: Invocation<'_>,
    env: &Env<'_>,
) -> Result<i32, Error> {
    // SAFETY: the owner structurally pins this field; no API moves it.
    let scope = unsafe { context.map_unchecked(|value| &value.environment) };
    scope.scope(env, || invoke_registration(context, record, invocation))?
}

/// Admission retains the owner/module before installing borrowed services.
/// Each synchronous callback clears its Env on every exit, including unwind.
pub(super) fn dispatch<S>(
    batch: &Batch<'_, S, Registration>,
    index: usize,
    depth: &DispatchDepth,
    invocation: Invocation<'_>,
    env: &Env<'_>,
) -> Result<i32, Error> {
    batch.dispatch(index, depth, |context, record| {
        invoke(context, *record, invocation, env)
    })?
}
