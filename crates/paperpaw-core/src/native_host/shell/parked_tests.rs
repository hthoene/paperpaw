//! A parked plugin's host side (#248): hosted again under its owner key, the
//! record of what its `make` called, and whether its owner holds anything.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::super::tests::{Heap, Module, NORMAL, READS_CLOCK, fake, module, parts, reset};
use super::{DeclaredServices, Error, Hosted};
use crate::native_host::HostResources;
use crate::native_host::resources::{FontKey, FontRole};
use core::ffi::c_void;
use core::ptr;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_STATE};

/// The fake hosted in `pool`: under a new owner key, or again under `owner`.
/// A hosting that is refused takes `()` as its module: the fixture's
/// [`Module`] says "unloaded" when it drops, for every fake of the thread.
fn fake_in<M>(
    pool: &HostResources,
    owner: Option<u64>,
    module: M,
) -> Result<Hosted<Heap, M>, Error> {
    let (make, report) = parts();
    let (pool, image) = (pool.try_clone().unwrap(), (ptr::null(), 0));
    let services = DeclaredServices::none();
    // SAFETY: `make` is the fixture's plugin, always callable; no image.
    unsafe {
        match owner {
            None => Hosted::new(Heap, module, make, "fake", image, report, pool, services),
            Some(key) => Hosted::new_for_owner(
                Heap, module, make, "fake", image, report, pool, key, services,
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
    let _ = context.run_plugin(|| unsafe { (context.table.clock_micros)(ctx, &mut micros) });
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
    let key = first.resource_identity().unwrap().owner_id();
    let batch = first.construct_registered(first.begin().unwrap()).unwrap();
    // While the first holds a handle its key is not given out again.
    let resources = &first.owner().context().get_ref().resources;
    resources.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
    assert_eq!(
        fake_in(&pool, Some(key), ()).err(),
        Some(Error::Status(ERR_BUSY))
    );
    drop(batch);
    drop(first);
    let again = fake_in(&pool, Some(key), module()).unwrap();
    assert_eq!(again.resource_identity().unwrap().owner_id(), key);
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
fn a_key_the_pool_never_gave_out_is_refused() {
    reset(NORMAL);
    let pool = HostResources::try_new().unwrap();
    for key in [0, 1, u64::MAX] {
        assert_eq!(
            fake_in(&pool, Some(key), ()).err(),
            Some(Error::Status(ERR_INVALID))
        );
    }
}

#[test]
fn a_key_whose_owner_still_lives_is_not_hosted_twice() {
    reset(NORMAL);
    let pool = HostResources::try_new().unwrap();
    let first = fake_in(&pool, None, module()).unwrap();
    let key = first.resource_identity().unwrap().owner_id();
    // No handle, nothing queued: only its being alive keeps the key.
    assert_eq!(
        fake_in(&pool, Some(key), ()).err(),
        Some(Error::Status(ERR_BUSY))
    );
    drop(first);
    let again = fake_in(&pool, Some(key), ()).unwrap();
    assert_eq!(
        fake_in(&pool, Some(key), ()).err(),
        Some(Error::Status(ERR_BUSY)),
        "nor while the second lives"
    );
    drop(again);
    assert!(fake_in(&pool, Some(key), ()).is_ok());
}

#[test]
fn no_key_is_hosted_again_once_a_retirement_was_lost() {
    reset(NORMAL);
    let pool = HostResources::try_new().unwrap();
    let key = fake_in(&pool, None, ())
        .unwrap()
        .resource_identity()
        .unwrap()
        .owner_id();
    pool.hosts().lose_a_retirement();
    assert_eq!(
        fake_in(&pool, Some(key), ()).err(),
        Some(Error::Status(ERR_STATE))
    );
}

#[test]
fn a_refused_re_admission_leaves_nothing_behind() {
    reset(NORMAL);
    let pool = HostResources::try_new().unwrap();
    // Four owners: the heap's owner table is full (4 places), so admitting
    // one more asks for memory; the retirement queue has room for four.
    let owners: alloc::vec::Vec<_> = (0..4).map(|_| pool.try_owner().unwrap()).collect();
    let key = owners[0].owner_id();
    let mut owners = owners.into_iter();
    drop(owners.next());
    let alive = pool.hosts().alive_owners();
    super::super::tests::arm_failure(1);
    let refused = pool.try_owner_for(key).err();
    super::super::tests::arm_failure(0);
    assert_eq!(refused, Some(ERR_NO_MEMORY));
    assert_eq!(
        pool.hosts().alive_owners(),
        alive,
        "its retirement room is back"
    );
    // Not marked live: the next try takes it.
    let again = pool.try_owner_for(key).unwrap();
    assert_eq!(again.owner_id(), key);
    drop(owners);
}
