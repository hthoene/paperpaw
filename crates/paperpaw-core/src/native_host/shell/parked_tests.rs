//! A parked plugin's host side (#248): hosted again under its owner key, the
//! record of what its `make` called, and whether its owner holds anything.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::super::tests::{Heap, Module, NORMAL, READS_CLOCK, fake, module, parts, reset};
use super::{DeclaredServices, Error, Hosted};
use crate::native_host::HostResources;
use crate::native_host::resources::{FontKey, FontRole, OwnerSeat};
use core::ffi::c_void;
use core::ptr;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};

/// The fake hosted in `pool`: under a new owner key, or again on `seat`.
/// A hosting that is refused takes `()` as its module: the fixture's
/// [`Module`] says "unloaded" when it drops, for every fake of the thread.
fn fake_in<M>(
    pool: &HostResources,
    seat: Option<OwnerSeat>,
    module: M,
) -> Result<Hosted<Heap, M>, Error> {
    let (make, report) = parts();
    let (pool, image) = (pool.try_clone().unwrap(), (ptr::null(), 0));
    let services = DeclaredServices::none();
    // SAFETY: `make` is the fixture's plugin, always callable; no image.
    unsafe {
        match seat {
            None => Hosted::new(Heap, module, make, "fake", image, report, pool, services),
            Some(seat) => Hosted::new_for_owner(
                Heap,
                module,
                make,
                paperpaw_sdk::Id::new_static("fake"),
                image,
                report,
                pool,
                seat,
                services,
            ),
        }
    }
}

/// The plugin's own code calls the host's clock, as a callback would.
fn plugin_reads_the_clock(hosted: &Hosted<Heap, Module>) {
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    let mut micros = 0;
    // SAFETY: the pinned context of a live owner, a writable output.
    let _ = context.run_plugin(|| unsafe { (context.table().clock_micros)(ctx, &mut micros) });
}

#[test]
fn a_make_that_only_registers_and_allocates_is_recorded_as_such() {
    reset(NORMAL);
    let hosted = fake();
    // Before `make` ran nothing is known.
    assert!(!hosted.make_registered_only());
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    // The fake's `make` allocates its state and submits its batch.
    assert!(hosted.make_registered_only());
    drop(batch);
}

#[test]
fn a_make_that_reads_the_clock_did_more_than_register() {
    reset(READS_CLOCK);
    let hosted = fake();
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    assert!(!hosted.make_registered_only());
    assert_eq!(hosted.owner_idle(), Ok(false));
    drop(batch);
}

#[test]
fn a_real_sdk_plugin_s_make_only_registers() {
    // Battery's linked entry: the SDK's own `make` wrapper and the plugin's
    // `make`, which submits its batches from its image.
    let make = paperpaw_plugin_battery::NATIVE_ENTRY.entry().make.unwrap();
    let report = parts().1;
    // SAFETY: statically linked code; no image range.
    let hosted = unsafe {
        Hosted::new(
            Heap,
            (),
            make,
            "battery",
            (ptr::null(), 0),
            report,
            HostResources::try_new().unwrap(),
            DeclaredServices::none(),
        )
    }
    .unwrap();
    let batch = hosted.construct_registered(hosted.begin().unwrap());
    assert!(hosted.make_registered_only());
    drop(batch);
}

#[test]
fn an_owner_is_idle_until_its_code_calls_the_host_after_make() {
    reset(NORMAL);
    let hosted = fake();
    // Not before `make` ran.
    assert_eq!(hosted.owner_idle(), Ok(false));
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    assert_eq!(hosted.owner_idle(), Ok(true));
    plugin_reads_the_clock(&hosted);
    assert_eq!(hosted.owner_idle(), Ok(false));
    // `make` itself is still on record as having only registered.
    assert!(hosted.make_registered_only());
    drop(batch);
}

#[test]
fn an_owner_holding_a_handle_is_not_idle() {
    reset(NORMAL);
    let hosted = fake();
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    let resources = &hosted.owner().context().get_ref().resources;
    let handle = resources.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
    assert_eq!(hosted.owner_idle(), Ok(false));
    resources.release(handle).unwrap();
    assert_eq!(hosted.owner_idle(), Ok(true));
    drop(batch);
}

#[test]
fn a_plugin_is_hosted_again_under_its_owner_key_once_the_first_is_gone() {
    reset(NORMAL);
    let pool = HostResources::try_new().unwrap();
    let first = fake_in(&pool, None, module()).unwrap();
    let seat = first.seat();
    let key = first.resource_identity().unwrap().owner_id();
    let batch = first.construct_registered(first.begin().unwrap()).unwrap();
    // While the first holds a handle its key is not given out again.
    let resources = &first.owner().context().get_ref().resources;
    resources.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
    assert_eq!(
        fake_in(&pool, Some(seat), ()).err(),
        Some(Error::Status(ERR_BUSY))
    );
    drop(batch);
    drop(first);
    let again = fake_in(&pool, Some(seat), module()).unwrap();
    assert_eq!(again.resource_identity().unwrap().owner_id(), key);
    assert_eq!(again.seat(), seat);
    let batch = again.construct_registered(again.begin().unwrap()).unwrap();
    assert!(again.make_registered_only());
    assert_eq!(again.owner_idle(), Ok(true));
    drop(batch);
    drop(again);
    // A fresh plugin still gets a key of its own.
    let other = fake_in(&pool, None, ()).unwrap();
    assert_ne!(other.resource_identity().unwrap().owner_id(), key);
}

#[test]
fn an_unparked_plugin_is_charged_on_its_old_heap_row() {
    reset(NORMAL);
    let pool = HostResources::try_new().unwrap();
    let first = fake_in(&pool, None, module()).unwrap();
    let seat = first.seat();
    let batch = first.construct_registered(first.begin().unwrap()).unwrap();
    // A block the plugin keeps past its instance (the row is never
    // released at park: what it was charged goes on).
    let kept = plugin_allocates(&first, 64);
    let rows = pool.heap_rows();
    drop(batch);
    drop(first);
    let charged = pool.heap_used(seat);
    assert!(charged >= 64, "the kept block is still charged: {charged}");
    let again = fake_in(&pool, Some(seat), module()).unwrap();
    assert_eq!(pool.heap_rows(), rows, "no new row");
    assert_eq!(pool.heap_used(again.seat()), charged, "the same row");
    let batch = again.construct_registered(again.begin().unwrap()).unwrap();
    assert!(pool.heap_used(seat) > charged, "make is charged on it too");
    plugin_frees(&again, kept, 64);
    drop(batch);
    drop(again);
    assert_eq!(pool.heap_rows(), rows);
}

/// The plugin's own code allocates `size` bytes on its heap.
fn plugin_allocates(hosted: &Hosted<Heap, Module>, size: usize) -> *mut c_void {
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    let mut block = ptr::null_mut();
    // SAFETY: the pinned context of a live owner, a writable output.
    let status = context
        .run_plugin(|| unsafe { (context.table().heap_alloc)(ctx, size, 8, &mut block) })
        .expect("entered");
    assert_eq!(status, 0);
    block
}

/// The plugin's own code frees a block of `size` bytes.
fn plugin_frees(hosted: &Hosted<Heap, Module>, block: *mut c_void, size: usize) {
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    // SAFETY: a block of this plugin's heap, with its size and alignment.
    let _ = context
        .run_plugin(|| unsafe { (context.table().heap_dealloc)(ctx, block, size, 8) })
        .expect("entered");
}

#[test]
fn a_seat_of_a_key_the_pool_never_gave_out_is_refused() {
    reset(NORMAL);
    let other = HostResources::try_new().unwrap();
    let seat = fake_in(&other, None, ()).unwrap().seat();
    let pool = HostResources::try_new().unwrap();
    assert_eq!(
        fake_in(&pool, Some(seat), ()).err(),
        Some(Error::Status(ERR_INVALID))
    );
}

#[test]
fn a_key_whose_owner_still_lives_is_not_hosted_twice() {
    reset(NORMAL);
    let pool = HostResources::try_new().unwrap();
    let first = fake_in(&pool, None, module()).unwrap();
    let seat = first.seat();
    // No handle, nothing queued: only its being alive keeps the key.
    assert_eq!(
        fake_in(&pool, Some(seat), ()).err(),
        Some(Error::Status(ERR_BUSY))
    );
    drop(first);
    let again = fake_in(&pool, Some(seat), ()).unwrap();
    assert_eq!(
        fake_in(&pool, Some(seat), ()).err(),
        Some(Error::Status(ERR_BUSY)),
        "nor while the second lives"
    );
    drop(again);
    assert!(fake_in(&pool, Some(seat), ()).is_ok());
}

#[test]
fn no_key_is_hosted_again_once_a_retirement_was_lost() {
    reset(NORMAL);
    let pool = HostResources::try_new().unwrap();
    let seat = fake_in(&pool, None, ()).unwrap().seat();
    pool.hosts().lose_a_retirement();
    assert_eq!(
        fake_in(&pool, Some(seat), ()).err(),
        Some(Error::Status(ERR_STATE))
    );
}

#[test]
fn an_owner_refused_its_heap_place_leaves_nothing_behind() {
    reset(NORMAL);
    let pool = HostResources::try_new().unwrap();
    let rows = pool.heap_rows();
    // The 1st allocation is the owner's retirement room, the 2nd its heap
    // place.
    super::super::tests::arm_failure(2);
    let refused = pool.try_owner().err();
    super::super::tests::arm_failure(0);
    assert_eq!(refused, Some(ERR_NO_MEMORY));
    assert_eq!(
        pool.hosts().alive_owners(),
        0,
        "its retirement room is back"
    );
    assert!(!pool.owner_live(1), "its mark is gone");
    assert_eq!(pool.heap_rows(), rows);
    // The key was not used up: the next owner gets it.
    assert_eq!(pool.try_owner().unwrap().owner_id(), 1);
}

#[test]
fn a_hosted_plugins_blob_expires_after_its_callback() {
    // #248: `Hosted::new` installs the context's own cleanup, so a callback's
    // blob expires through `Context.resources` (no second view of them).
    reset(NORMAL);
    let pool = HostResources::try_new().unwrap();
    let hosted = fake_in(&pool, None, module()).unwrap();
    let context = hosted.owner().context();
    let blob = crate::native_host::resources::Blob {
        bytes: alloc::vec![7; 12],
        callback: 1,
    };
    context.resources.insert_blob(blob).unwrap();
    assert_eq!(context.resources.holds_blobs(), Ok(true));
    // SAFETY: the owner structurally pins the scope; nothing moves it.
    let scope = unsafe { context.map_unchecked(|context| &context.environment) };
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    scope.scope(&env, || ()).unwrap();
    assert_eq!(context.resources.holds_blobs(), Ok(false));
}
