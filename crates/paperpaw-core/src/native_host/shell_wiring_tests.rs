#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The shells' wiring of the host views.
extern crate std;
use super::*;
use paperpaw_sdk::MemoryStorage;

struct Nothing;
impl DeviceHost for Nothing {}

fn storage() -> Rc<dyn Storage> {
    Rc::new(MemoryStorage::new())
}

#[test]
fn nothing_named_wires_nothing() {
    let resources = HostResources::try_new().unwrap();
    let wired = resources.wire(HostWiring::new()).unwrap();
    assert!(wired.hyphen().is_none());
    assert!(resources.hosts().hyphen().is_none());
    assert!(resources.hosts().device().is_none());
}

#[test]
fn the_families_named_are_wired_and_the_hyphen_tables_can_be_declared() {
    let resources = HostResources::try_new().unwrap();
    let wired = resources
        .wire(
            HostWiring::new()
                .hyphenation(storage())
                .device(Rc::new(Nothing)),
        )
        .unwrap();
    assert!(resources.hosts().hyphen().is_some());
    assert!(resources.hosts().device().is_some());
    let tables = wired.hyphen().unwrap();
    tables
        .declare([("de", "plugins/a/de.hyph", 1)].into_iter())
        .unwrap();
    // A bad declaration changes nothing and says so.
    assert_eq!(
        tables.declare([("", "plugins/a/x.hyph", 1)].into_iter()),
        Err(paperpaw_native::host_glue::ERR_INVALID)
    );
    tables.card_changed();
}

#[test]
fn a_family_wired_twice_is_refused_and_nothing_else_is_wired_by_that_call() {
    let resources = HostResources::try_new().unwrap();
    resources
        .wire(HostWiring::new().hyphenation(storage()))
        .unwrap();
    // The hyphen tables are wired: a wiring that names them again and the
    // device is refused whole; the device stays unwired.
    let again = resources.wire(
        HostWiring::new()
            .hyphenation(storage())
            .device(Rc::new(Nothing)),
    );
    assert_eq!(again.err(), Some(ERR_STATE));
    assert!(resources.hosts().device().is_none());
    // The device alone can still follow.
    resources
        .wire(HostWiring::new().device(Rc::new(Nothing)))
        .unwrap();
    assert!(resources.hosts().device().is_some());
}

#[test]
fn core_ui_is_wired_once_with_the_store_sized_for_the_device() {
    let resources = HostResources::try_new().unwrap();
    assert!(resources.hosts().ui().is_none());
    resources.wire(HostWiring::new().ui("xteink-x4")).unwrap();
    let ui = resources.hosts().ui().unwrap();
    // The X4's small limits: three request rows kept (sixteen elsewhere).
    assert_eq!(ui.store.limits.requests_kept, 3);
    // Twice is refused and a family named beside it is not wired either.
    let again = resources.wire(HostWiring::new().ui("desktop").device(Rc::new(Nothing)));
    assert_eq!(again.err(), Some(ERR_STATE));
    assert!(resources.hosts().device().is_none());
    let other = HostResources::try_new().unwrap();
    other.wire(HostWiring::new().ui("desktop")).unwrap();
    assert_eq!(other.hosts().ui().unwrap().store.limits.requests_kept, 16);
}
