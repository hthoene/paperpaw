#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The shells' wiring of the host views.
extern crate std;
use super::*;
use paperpaw_platform::MemoryClass;
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
fn core_lookup_is_wired_once_over_the_declarations() {
    let resources = HostResources::try_new().unwrap();
    assert!(resources.hosts().lookup().is_none());
    resources.wire(HostWiring::new().lookup()).unwrap();
    assert!(resources.hosts().lookup().is_some());
    // Wired once: a second wiring is refused.
    assert_eq!(
        resources.wire(HostWiring::new().lookup()).err(),
        Some(paperpaw_native::host_glue::ERR_STATE)
    );
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
    resources
        .wire(HostWiring::new().ui(MemoryClass::Small))
        .unwrap();
    let ui = resources.hosts().ui().unwrap();
    // The X4's small limits: three request rows kept (sixteen elsewhere).
    assert_eq!(ui.store.limits.requests_kept, 3);
    // Twice is refused and a family named beside it is not wired either.
    let again = resources.wire(
        HostWiring::new()
            .ui(MemoryClass::Large)
            .device(Rc::new(Nothing)),
    );
    assert_eq!(again.err(), Some(ERR_STATE));
    assert!(resources.hosts().device().is_none());
    let other = HostResources::try_new().unwrap();
    other
        .wire(HostWiring::new().ui(MemoryClass::Large))
        .unwrap();
    assert_eq!(other.hosts().ui().unwrap().store.limits.requests_kept, 16);
}

#[test]
fn the_plugin_with_the_greater_id_wins_a_tag_without_regard_to_case() {
    use super::super::hyphen_service::HyphenHost;
    let tables = HyphenTables::over(Rc::new(CardTables::new(storage())));
    tables
        .declare_plugins(
            [
                ("de", "plugins/a-pack/de.hyph", "a-pack"),
                ("DE", "plugins/B-pack/de.hyph", "B-pack"),
                ("en", "plugins/a-pack/en.hyph", "a-pack"),
            ]
            .into_iter(),
        )
        .unwrap();
    // The entries are numbered in the order given: B-pack's `de` is the second.
    assert_eq!(tables.card().find("de"), Some(1));
    assert_eq!(tables.card().find("en"), Some(2));
    assert_eq!(tables.card().find("fr"), None);
    // The same two plugins in another order, with ids that differ only by case.
    tables
        .declare_plugins(
            [
                ("de", "plugins/z/de.hyph", "z"),
                ("de", "plugins/Z/de.hyph", "Z"),
            ]
            .into_iter(),
        )
        .unwrap();
    assert!(tables.card().find("de").is_some());
}

/// A sealer that seals nothing (the shells' test double; not a real cipher).
struct Keyed;
impl Sealer for Keyed {
    fn seals(&self) -> bool {
        true
    }
    fn seal(&self, _: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        out.get_mut(..plain.len())
            .ok_or(ERR_STATE)?
            .copy_from_slice(plain);
        Ok(plain.len())
    }
    fn open(&self, _: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        out.get_mut(..sealed.len())
            .ok_or(ERR_STATE)?
            .copy_from_slice(sealed);
        Ok(sealed.len())
    }
}

struct Clock;
impl WorkDevice for Clock {
    fn now_ms(&self) -> u64 {
        5
    }
    fn network_up(&self) -> bool {
        true
    }
    fn radio_on(&self) -> bool {
        true
    }
}

fn no_names() -> core::iter::Empty<&'static str> {
    core::iter::empty()
}

#[test]
fn core_work_is_wired_once_and_its_view_is_made_of_the_shell_the_kernel_and_the_directory() {
    use super::super::work_sched::Power;
    let resources = HostResources::try_new().unwrap();
    resources
        .wire(HostWiring::new().work(WorkInputs::new(Rc::new(Clock), MemoryClass::Small)))
        .unwrap();
    let work = resources.hosts().work().unwrap();
    let env = &*work.env;
    // The shell's own answers.
    assert_eq!(env.now_ms(), 5);
    assert!(env.network_up());
    // What the kernel tells before a pass.
    assert_eq!(env.power().level, 0);
    env.kernel_state(2, 4, true);
    assert_eq!(
        env.power(),
        Power {
            level: 2,
            stretch: 4,
            radio_on: true
        }
    );
    assert!(env.screen_hold());
    // A plugin the directory does not know, or a policy not filled yet: its
    // network is off, never allowed by a guess.
    assert!(!env.network_allowed(1));
    resources
        .hosts()
        .owners()
        .unwrap()
        .admit(1, &paperpaw_sdk::Id::new_static("news"))
        .unwrap();
    assert!(!env.network_allowed(1));
    let policy = resources.hosts().policy();
    policy.update(1, false, false, no_names(), no_names());
    assert!(env.network_allowed(1));
    assert!(!env.saving_manual(1));
    policy.update(2, false, false, no_names(), ["news"].into_iter());
    assert!(env.network_allowed(1));
    assert!(env.saving_manual(1));
    policy.update(3, false, false, ["news"].into_iter(), no_names());
    assert!(!env.network_allowed(1));
    assert!(!env.subscribed(1, 103));
    // Twice is refused, and the family named beside it is not wired either.
    let again = resources.wire(
        HostWiring::new()
            .work(WorkInputs::new(Rc::new(Clock), MemoryClass::Large))
            .device(Rc::new(Nothing)),
    );
    assert_eq!(again.err(), Some(ERR_STATE));
    assert!(resources.hosts().device().is_none());
}

#[test]
fn core_work_is_made_at_its_first_call_and_a_call_without_room_tries_again() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let resources = HostResources::try_new().unwrap();
    resources
        .wire(HostWiring::new().work(WorkInputs::new(Rc::new(Clock), MemoryClass::Small)))
        .unwrap();
    let hosts = resources.hosts();
    assert!(hosts.work_wired());
    assert!(
        hosts.work_made().is_none(),
        "nothing made at the wiring (#248)"
    );
    // The first call without room: no memory, still wired, nothing made.
    FAIL_NTH.with(|n| n.set(1));
    let refused = hosts.work_for_call().err();
    FAIL_NTH.with(|n| n.set(0));
    assert_eq!(refused, Some(ERR_NO_MEMORY));
    assert!(hosts.work_made().is_none() && hosts.work_wired());
    // The next call makes it, over the shell's view as before.
    let work = hosts.work_for_call().unwrap();
    assert_eq!(work.env.now_ms(), 5);
    assert!(work.env.network_up());
    assert!(core::ptr::eq(hosts.work_made().unwrap(), work));
    // Wired once: a second wiring is refused.
    let again =
        resources.wire(HostWiring::new().work(WorkInputs::new(Rc::new(Clock), MemoryClass::Small)));
    assert_eq!(again.err(), Some(ERR_STATE));
}

#[test]
fn core_secrets_is_wired_once_and_the_shell_keeps_its_lifecycle_calls() {
    let resources = HostResources::try_new().unwrap();
    let mut wired = resources
        .wire(HostWiring::new().secrets(SecretsInputs::new(storage(), Rc::new(Keyed))))
        .unwrap();
    assert!(resources.hosts().secrets().is_some());
    // No file yet: nothing to upgrade.
    assert_eq!(wired.secrets().unwrap().upgrade(), Ok(false));
    // A shell may keep it past the wiring (the X4 upgrades again once its
    // random numbers are strong); it is taken once.
    let kept = wired.take_secrets().unwrap();
    assert_eq!(kept.upgrade(), Ok(false));
    assert!(wired.take_secrets().is_none() && wired.secrets().is_none());
    let again = resources.wire(
        HostWiring::new()
            .secrets(SecretsInputs::new(storage(), Rc::new(Keyed)))
            .device(Rc::new(Nothing)),
    );
    assert_eq!(again.err(), Some(ERR_STATE));
    assert!(resources.hosts().device().is_none());
}

#[test]
fn the_plugin_heap_budget_is_named_or_follows_the_classes_named() {
    use super::super::plugin_heap::{PLUGIN_HEAP_LARGE, PLUGIN_HEAP_SMALL};
    let class = |wiring: HostWiring| wiring.class_plugin_heap();
    // None named: small. One named: its class. Two that differ: the smaller.
    assert_eq!(class(HostWiring::new()), PLUGIN_HEAP_SMALL);
    assert_eq!(
        class(HostWiring::new().ui(MemoryClass::Small)),
        PLUGIN_HEAP_SMALL
    );
    assert_eq!(
        class(HostWiring::new().ui(MemoryClass::Large)),
        PLUGIN_HEAP_LARGE
    );
    assert_eq!(
        class(HostWiring::new().documents(MemoryClass::Large, 800)),
        PLUGIN_HEAP_LARGE
    );
    assert_eq!(
        class(
            HostWiring::new()
                .ui(MemoryClass::Large)
                .documents(MemoryClass::Small, 800)
        ),
        PLUGIN_HEAP_SMALL
    );
    assert_eq!(
        class(
            HostWiring::new()
                .ui(MemoryClass::Small)
                .documents(MemoryClass::Large, 800)
        ),
        PLUGIN_HEAP_SMALL
    );
    // Wired: the class default, then the shell's own number, which wins.
    let resources = HostResources::try_new().unwrap();
    resources
        .wire(HostWiring::new().ui(MemoryClass::Large))
        .unwrap();
    assert_eq!(resources.plugin_heap_limit(), PLUGIN_HEAP_LARGE);
    let resources = HostResources::try_new().unwrap();
    resources
        .wire(HostWiring::new().ui(MemoryClass::Large).plugin_heap(12_345))
        .unwrap();
    assert_eq!(resources.plugin_heap_limit(), 12_345);
    let resources = HostResources::try_new().unwrap();
    resources.wire(HostWiring::new().plugin_heap(0)).unwrap();
    assert_eq!(resources.plugin_heap_limit(), 0);
}

#[test]
fn the_library_is_wired_over_the_catalog_reads_and_once() {
    let card = crate::native_host::lib_reads::tests::card();
    let storage: Rc<dyn Storage> = card.storage.clone();
    let resources = HostResources::try_new().unwrap();
    assert!(resources.hosts().library().is_none());
    resources
        .wire(HostWiring::new().library(storage.clone(), card.env.clone()))
        .unwrap();
    assert!(resources.hosts().library().is_some());
    // Wired once: a second naming is refused whole.
    let again = resources.wire(HostWiring::new().library(storage, card.env.clone()));
    assert_eq!(again.err(), Some(ERR_STATE));
}

#[test]
fn a_library_wired_after_files_sees_the_move_job_of_files() {
    use crate::native_host::lib_service::BookRef;
    use crate::native_host::registrations as wire;
    use paperpaw_native::host_glue::ERR_BUSY;
    let card = crate::native_host::lib_reads::tests::card();
    let storage: Rc<dyn Storage> = card.storage.clone();
    let resources = HostResources::try_new().unwrap();
    resources
        .wire(
            HostWiring::new()
                .documents(MemoryClass::Large, 800)
                .files(storage.clone()),
        )
        .unwrap();
    // A later wiring names the library alone: it shares the job all the same.
    resources
        .wire(HostWiring::new().library(storage, card.env.clone()))
        .unwrap();
    let moves = resources.hosts().library_moves().unwrap().clone();
    moves.begin("books/c.epub", "moved/c.epub", false).unwrap();
    let provided = wire::ServiceLibProvided {
        title: "Provided",
        author: "",
        series: "",
        number: "",
        tags: "",
        flags: 0b1,
    };
    let library = resources.hosts().library().unwrap();
    assert_eq!(
        library.0.provide(BookRef::Path("books/c.epub"), &provided),
        Err(ERR_BUSY)
    );
}

#[test]
fn the_library_transactions_learn_the_memory_class_and_clock_from_wirings_of_their_own() {
    // The X4 shell wires `work`, `library` and `ui` in calls of their own, in
    // an order the library does not decide.
    let card = crate::native_host::lib_reads::tests::card();
    let resources = HostResources::try_new().unwrap();
    let policy = resources.hosts().edit_policy().clone();
    assert!(!policy.is_small() && policy.clock_ms().is_none());
    resources
        .wire(HostWiring::new().work(WorkInputs::new(Rc::new(Clock), MemoryClass::Large)))
        .unwrap();
    resources
        .wire(HostWiring::new().library(card.storage.clone(), card.env.clone()))
        .unwrap();
    // The device's clock is there, the memory class is not yet said.
    assert_eq!((policy.is_small(), policy.clock_ms()), (false, Some(5)));
    resources
        .wire(HostWiring::new().ui(MemoryClass::Small))
        .unwrap();
    assert!(policy.is_small());
}

#[test]
fn the_library_learns_the_screens_long_side_from_a_documents_wiring_of_its_own() {
    // Op 7 bounds a cover by the screen's long side: unsupported until the
    // shell wires `documents`, in whichever call that comes.
    use paperpaw_native::host_glue::ERR_UNSUPPORTED;
    let card = crate::native_host::lib_reads::tests::card();
    let resources = HostResources::try_new().unwrap();
    resources
        .wire(HostWiring::new().library(card.storage.clone(), card.env.clone()))
        .unwrap();
    let library = resources.hosts().library().unwrap();
    assert_eq!(library.0.screen_long_side(), Err(ERR_UNSUPPORTED));
    resources
        .wire(HostWiring::new().documents(MemoryClass::Small, 800))
        .unwrap();
    assert_eq!(library.0.screen_long_side(), Ok(800));
}

#[test]
fn a_value_that_does_not_open_is_told_once_per_start() {
    let host = SecretsInputs::new(storage(), Rc::new(Keyed)).host();
    assert!(!host.take_unopened(), "nothing to tell");
    host.unopened();
    host.unopened();
    assert!(host.take_unopened());
    assert!(!host.take_unopened(), "once");
    host.unopened();
    assert!(!host.take_unopened(), "once per start, not once per line");
}
