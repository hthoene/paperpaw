#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::native_host::{
    HostResources,
    environment::EnvironmentScope,
    resources::{FontKey, FontRole},
};
use alloc::vec;
use core::pin::pin;
use paperpaw_native::host_glue::ERR_FULL;
use paperpaw_platform::Instant;

fn blob(bytes: usize, callback: u64) -> Blob {
    Blob {
        bytes: vec![0x91; bytes],
        callback,
    }
}

#[test]
fn shared_device_budget_and_kind_ownership_are_enforced() {
    let pool = HostResources::try_new().unwrap();
    let first = pool.try_owner().unwrap();
    assert_eq!(first.blob_page_bytes(), 384);
    let second = pool.try_owner().unwrap();
    let font = first.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
    let handle = first.insert_blob(blob(47, 1)).unwrap();
    assert_eq!(first.with_blob(handle, 1, |bytes| Ok(bytes.len())), Ok(47));
    assert_eq!(first.with_blob(handle, 2, |_| Ok(())), Err(ERR_STATE));
    assert_eq!(second.with_blob(handle, 1, |_| Ok(())), Err(ERR_STATE));
    assert_eq!(first.with_blob(font, 1, |_| Ok(())), Err(ERR_STATE));
    assert_eq!(first.release(handle), Err(ERR_STATE));
    assert_eq!(second.insert_blob(blob(1, 1)), Err(ERR_BUSY));
    first.release_blob(handle, 1).unwrap();
    assert_eq!(first.release_blob(handle, 1), Err(ERR_STATE));
    let fresh = second.insert_blob(blob(384, 1)).unwrap();
    assert_ne!(fresh, handle);
    first.with_font(font, |_| Ok(())).unwrap();
    second.release_all().unwrap();
    assert_eq!(second.with_blob(fresh, 1, |_| Ok(())), Err(ERR_STATE));
    assert!(first.insert_blob(blob(385, 2)).is_err());
}

#[test]
fn owner_family_limit_precedes_larger_device_byte_budget() {
    let pool = HostResources::with_blob_limits(Limits::LARGE).unwrap();
    let owners: alloc::vec::Vec<_> = (0..3).map(|_| pool.try_owner().unwrap()).collect();
    for owner in &owners[..2] {
        owner.insert_blob(blob(4096, 1)).unwrap();
        owner.insert_blob(blob(4096, 1)).unwrap();
    }
    assert_eq!(owners[0].insert_blob(blob(1, 1)), Err(ERR_FULL));
    assert_eq!(owners[2].insert_blob(blob(1, 1)), Err(ERR_BUSY));
    owners[0].expire_blobs().unwrap();
    assert!(owners[2].insert_blob(blob(1, 1)).is_ok());
}

#[test]
fn scope_end_expires_blobs_but_keeps_fonts_and_nested_busy_keeps_epoch() {
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let view = owner.try_view().unwrap();
    let scope = pin!(EnvironmentScope::new());
    // SAFETY: the view outlives the scope's callbacks.
    unsafe { scope.expire_view(&view) };
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let font = owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
    let handle = scope
        .as_ref()
        .scope(&env, || {
            let callback = scope.callback_id();
            let handle = owner.insert_blob(blob(12, callback)).unwrap();
            assert!(scope.as_ref().scope(&env, || ()).is_err());
            assert_eq!(scope.callback_id(), callback);
            owner.with_blob(handle, callback, |_| Ok(())).unwrap();
            handle
        })
        .unwrap();
    assert_eq!(
        owner.with_blob(handle, scope.callback_id(), |_| Ok(())),
        Err(ERR_STATE)
    );
    owner.with_font(font, |_| Ok(())).unwrap();
    scope
        .as_ref()
        .scope(&env, || {
            let fresh = owner.insert_blob(blob(12, scope.callback_id())).unwrap();
            assert_ne!(fresh, handle);
        })
        .unwrap();
}

#[test]
fn unwind_reclaims_callback_blobs_without_allocation() {
    extern crate std;
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let view = owner.try_view().unwrap();
    let scope = pin!(EnvironmentScope::new());
    // SAFETY: the view outlives the scope's callbacks.
    unsafe { scope.expire_view(&view) };
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scope
            .as_ref()
            .scope(&env, || {
                owner.insert_blob(blob(12, scope.callback_id())).unwrap();
                panic!("plugin unwind");
            })
            .unwrap();
    }));
    assert!(result.is_err());
    assert!(owner.insert_blob(blob(12, 99)).is_ok());
}

#[test]
fn dropping_a_callback_resource_view_never_releases_owner_fonts() {
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let font = owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
    {
        let _view = owner.try_view().unwrap();
    }
    owner.with_font(font, |_| Ok(())).unwrap();
}

#[test]
fn cleanup_failure_reports_once_and_preserves_the_delivered_callback_result() {
    use core::{cell::Cell, ffi::c_void};
    type Probe = (OwnerResourceView, Cell<(u32, i32)>);
    unsafe fn expire(raw: *const c_void) {
        // SAFETY: the test installs the live probe with this exact type.
        let (view, log) = unsafe { &*raw.cast::<Probe>() };
        if let Err(status) = view.expire_blobs() {
            log.set((log.get().0 + 1, status));
        }
    }
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let probe: Probe = (owner.try_view().unwrap(), Cell::new((0, 0)));
    let log = &probe.1;
    let scope = pin!(EnvironmentScope::new());
    // SAFETY: the probe stays in place until after every callback; the hook
    // never reenters.
    unsafe {
        scope.set_cleanup((&raw const probe).cast(), expire);
    }
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let handle = owner.insert_blob(blob(12, 1)).unwrap();
    let held = owner.table.handles.borrow();
    assert_eq!(
        scope.as_ref().scope(&env, || Err::<(), _>(-42)),
        Ok(Err(-42))
    );
    assert_eq!(log.get(), (1, ERR_BUSY));
    drop(held);
    scope
        .as_ref()
        .scope(&env, || {
            assert_eq!(
                owner.with_blob(handle, scope.callback_id(), |_| Ok(())),
                Err(ERR_STATE)
            );
        })
        .unwrap();
    assert_eq!(log.get(), (1, ERR_BUSY));
    assert!(owner.insert_blob(blob(12, 99)).is_ok());
}

#[test]
fn only_a_blob_keeps_a_plugin_from_sleeping_again() {
    // #248, D3: fonts and service handles are released with the instance
    // and opened again by the remade plugin; a blob is not.
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let other = pool.try_owner().unwrap();
    owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
    assert_eq!(owner.holds_blobs(), Ok(false));
    let handle = owner.insert_blob(blob(12, 1)).unwrap();
    assert_eq!(owner.holds_blobs(), Ok(true));
    assert_eq!(other.holds_blobs(), Ok(false));
    owner.release_blob(handle, 1).unwrap();
    assert_eq!(owner.holds_blobs(), Ok(false));
}
