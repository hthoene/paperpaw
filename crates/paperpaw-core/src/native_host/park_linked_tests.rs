//! Linked plugins parked at their admission (#248, #2638): which are parked,
//! what is published for them, and how a first use makes them again.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]

use super::*;
use crate::native_host::{FixedTables, HostResources, registrations as wire};
use core::{cell::Cell, ffi::c_void, ptr};
use paperpaw_native::host_glue::{self, ERR_CANCELLED, PpBuffer, PpBytes, PpHost, PpPlugin};
use paperpaw_native::metadata::{Host, Profile};
use std::{alloc::Layout, sync::OnceLock};

extern crate std;

// As in `linked_install_tests`: target linux-x86_64-sysv64-le, floor 1,
// nothing used or required.
static METADATA: &[u8] = b"PPAM\x02\x00 \x00N\x00\x00\x00\x07\x00\x01\x00\x01\x00 \x00\x00\x00\x00\x00\x16\x00\x00\x00\x00\x00\x01\x00linux-x86_64-sysv64-le\x18\x00\x01\x00 \x00\x00\x00\x0e\x00PAPERPAW_ENTRY";

struct Heap;
impl Services for Heap {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        // SAFETY: the caller passes a valid nonzero layout.
        Layout::from_size_align(size, align).map_or(ptr::null_mut(), |layout| unsafe {
            std::alloc::alloc(layout)
        })
    }
    unsafe fn dealloc(&self, block: *mut u8, size: usize, align: usize) {
        // SAFETY: this allocator's block, freed once.
        unsafe { std::alloc::dealloc(block, Layout::from_size_align(size, align).unwrap()) };
    }
    unsafe fn realloc(&self, block: *mut u8, old: usize, align: usize, new: usize) -> *mut u8 {
        // SAFETY: as `dealloc`.
        unsafe { std::alloc::realloc(block, Layout::from_size_align(old, align).unwrap(), new) }
    }
    fn micros(&self) -> u64 {
        0
    }
}

/// What the fixture's `make` does.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Submits one screen whose bytes lie in the image: parked.
    Parkable,
    /// The same screen, encoded on its stack: copied, not parked.
    Copied,
    /// Reads the clock before submitting: not only registered.
    ReadsClock,
    /// A widget besides the screen: a role another plugin reaches.
    Widget,
    /// Told `event.started`: would be made at once.
    Started,
    /// The screen under another title: another batch than the admission's.
    Retitled,
    /// No room: `make` fails with `ERR_NO_MEMORY` before it submits.
    NoRoom,
}

std::thread_local! {
    static MODE: Cell<Mode> = const { Cell::new(Mode::Parkable) };
    static MAKES: Cell<u32> = const { Cell::new(0) };
    static DROPS: Cell<u32> = const { Cell::new(0) };
}

/// The program's read-only data as far as the fixture goes: the encoded
/// declaration batches, at fixed places, never written after they are made.
struct Image {
    bytes: &'static [u8],
    screen: (usize, usize),
    retitled: (usize, usize),
    widget: (usize, usize),
}

fn image() -> &'static Image {
    static IMAGE: OnceLock<Image> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let mut bytes = std::vec![0; 1024];
        let screen = |title| {
            wire::Record::ScreenSpec(wire::ScreenSpec {
                name: "screen",
                title,
                token: 91,
                flags: 0,
            })
        };
        let first = wire::records_encode(&[screen("linked.title")], &mut bytes).unwrap();
        let second = wire::records_encode(&[screen("linked.other")], &mut bytes[first..]).unwrap();
        let widget = wire::Record::WidgetSpec(wire::WidgetSpec {
            name: "widget",
            title: "linked.widget",
            category: "core.widgets.words",
            previews: "",
            cells: 0x0101_0102,
            token: 92,
            flags: 0,
        });
        let third = wire::records_encode(
            &[screen("linked.title"), widget],
            &mut bytes[first + second..],
        )
        .unwrap();
        Image {
            screen: (0, first),
            retitled: (first, second),
            widget: (first + second, third),
            bytes: std::vec::Vec::leak(bytes),
        }
    })
}

fn submit(host: *const PpHost, ctx: *mut c_void, bytes: &[u8]) -> i32 {
    // SAFETY: the host table and context are live for this `make`; the bytes
    // stay readable until the call returns.
    unsafe {
        ((*host).registrations_submit)(
            ctx,
            PpBytes {
                ptr: bytes.as_ptr(),
                len: bytes.len(),
            },
        )
    }
}

fn submit_registrations(
    host: *const PpHost,
    ctx: *mut c_void,
    batch: &[wire::Registration],
) -> i32 {
    let mut bytes = [0; 64];
    let len = wire::registrations_encode(batch, &mut bytes).unwrap();
    submit(host, ctx, &bytes[..len])
}

unsafe extern "C" fn make(host: *const PpHost, ctx: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    MAKES.with(|count| count.set(count.get() + 1));
    let image = image();
    let slice = |(at, len): (usize, usize)| &image.bytes[at..at + len];
    let mode = MODE.with(Cell::get);
    if mode == Mode::NoRoom {
        return host_glue::ERR_NO_MEMORY;
    }
    let status = match mode {
        Mode::Parkable | Mode::Started => submit(host, ctx, slice(image.screen)),
        Mode::Retitled => submit(host, ctx, slice(image.retitled)),
        Mode::Copied => {
            let copy = slice(image.screen).to_vec();
            submit(host, ctx, &copy)
        }
        Mode::ReadsClock => {
            let mut micros = 0;
            // SAFETY: as `submit`; `micros` is this frame's.
            unsafe { ((*host).clock_micros)(ctx, &mut micros) };
            submit(host, ctx, slice(image.screen))
        }
        Mode::NoRoom => host_glue::ERR_NO_MEMORY,
        Mode::Widget => match submit(host, ctx, slice(image.widget)) {
            host_glue::OK => {
                submit_registrations(host, ctx, &[wire::Registration::Draw { token: 92 }])
            }
            status => status,
        },
    };
    if status != host_glue::OK {
        return status;
    }
    if mode == Mode::Started {
        let started = wire::Registration::Event {
            event_tag: wire::EVENT_STARTED_TAG,
            token: 93,
        };
        let status = submit_registrations(host, ctx, &[started]);
        if status != host_glue::OK {
            return status;
        }
    }
    let table = Box::into_raw(Box::new(PpPlugin {
        size: u32::try_from(size_of::<PpPlugin>()).unwrap(),
        state: ptr::null_mut(),
        drop: Some(destroy),
        dispatch_event: Some(event),
        input: None,
        draw: Some(draw),
    }));
    // SAFETY: `out` is the host's slot; the table is released by `destroy`.
    unsafe {
        (*table).state = table.cast();
        out.write(table);
    }
    host_glue::OK
}

unsafe extern "C" fn destroy(state: *mut c_void) {
    DROPS.with(|count| count.set(count.get() + 1));
    // SAFETY: the table `make` leaked, released once.
    drop(unsafe { Box::from_raw(state.cast::<PpPlugin>()) });
}

unsafe extern "C" fn event(_: *mut c_void, _: u32, _: PpBytes, _: PpBuffer) -> i32 {
    host_glue::OK
}

unsafe extern "C" fn draw(_: *mut c_void, _: u32, _: PpBytes) -> i32 {
    host_glue::OK
}

fn entry() -> &'static PpEntry {
    Box::leak(Box::new(PpEntry {
        size: u32::try_from(size_of::<PpEntry>()).unwrap(),
        abi_major: host_glue::MAJOR,
        needs_minor: 1,
        metadata: PpBytes {
            ptr: METADATA.as_ptr(),
            len: METADATA.len(),
        },
        make: Some(make),
    }))
}

fn shell_host() -> Host<'static> {
    Host {
        abi_major: host_glue::MAJOR,
        minor: host_glue::MINOR,
        profile: Profile {
            target: "linux-x86_64-sysv64-le",
            entry_layout: 1,
            entry_bytes: u16::try_from(size_of::<PpEntry>()).unwrap(),
        },
        services: &[],
        features: &[],
    }
}

fn kernel(mode: Mode) -> crate::Kernel {
    MODE.with(|cell| cell.set(mode));
    MAKES.with(|count| count.set(0));
    DROPS.with(|count| count.set(0));
    crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap()
}

/// Admits the fixture as `linked` through the parking entry.
fn admit(kernel: &mut crate::Kernel) -> u64 {
    admit_with(kernel, HostResources::try_new().unwrap())
}

/// [`admit`] with the shell's resources (one pool for every plugin).
fn admit_with(kernel: &mut crate::Kernel, resources: HostResources) -> u64 {
    admit_as(kernel, "linked", resources)
}

/// [`admit_with`] under the id `id`.
fn admit_as(kernel: &mut crate::Kernel, id: &'static str, resources: HostResources) -> u64 {
    let image = image().bytes;
    // The device's one depth counter (a second is refused).
    let depth = match kernel.shared.native_depth.as_ref() {
        Some(depth) => depth.try_clone().unwrap(),
        None => FlatDepth::try_new().unwrap(),
    };
    // SAFETY: the fixture obeys the factory contract, its code is static and
    // `image` is leaked and never written after it was made.
    unsafe {
        let shell = crate::native_host::ParkShell::try_new(
            &shell_host(),
            || Heap,
            &resources,
            &depth,
            |_, _, _, _| {},
            |_, _, _, _| {},
            (image.as_ptr(), image.len()),
        )
        .unwrap();
        kernel.register_linked_parked(entry(), METADATA, id, &shell, FixedTables::NONE)
    }
    .unwrap();
    kernel.shared.native_instances.find(id).unwrap()
}

fn lease(kernel: &crate::Kernel, key: u64) -> super::super::native_instances::NativeLease {
    kernel.shared.native_instances.lease(key).unwrap()
}

fn makes() -> u32 {
    MAKES.with(Cell::get)
}

fn drops() -> u32 {
    DROPS.with(Cell::get)
}

#[test]
fn a_plugin_that_only_registered_is_parked_with_its_screen_published() {
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    // Made once, its instance dropped, its screen there to be opened.
    assert_eq!((makes(), drops()), (1, 1));
    assert!(lease(&kernel, key).is_parked());
    assert!(lease(&kernel, key).running());
    assert!(kernel.shared.registry.screens.contains_key("linked.screen"));
}

#[test]
fn the_first_use_makes_a_parked_plugin_once() {
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    let lease = lease(&kernel, key);
    assert!(lease.live().is_ok());
    assert_eq!(makes(), 2);
    assert!(!lease.is_parked());
    // Made: a second use takes the same instance.
    assert!(lease.live().is_ok());
    assert_eq!((makes(), drops()), (2, 1));
    drop(lease);
    drop(kernel);
    assert_eq!(drops(), 2);
}

#[test]
fn a_plugin_whose_declarations_are_copied_runs() {
    let mut kernel = kernel(Mode::Copied);
    let key = admit(&mut kernel);
    assert_eq!((makes(), drops()), (1, 0));
    assert!(!lease(&kernel, key).is_parked());
}

#[test]
fn a_plugin_whose_make_read_the_clock_runs() {
    let mut kernel = kernel(Mode::ReadsClock);
    let key = admit(&mut kernel);
    assert_eq!((makes(), drops()), (1, 0));
    assert!(!lease(&kernel, key).is_parked());
}

#[test]
fn a_plugin_with_a_widget_runs() {
    let mut kernel = kernel(Mode::Widget);
    let key = admit(&mut kernel);
    assert_eq!((makes(), drops()), (1, 0));
    assert!(!lease(&kernel, key).is_parked());
}

#[test]
fn a_plugin_told_at_the_start_runs() {
    let mut kernel = kernel(Mode::Started);
    let key = admit(&mut kernel);
    assert_eq!((makes(), drops()), (1, 0));
    assert!(!lease(&kernel, key).is_parked());
}

#[test]
fn another_batch_on_the_first_use_refuses_the_plugin() {
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    MODE.with(|cell| cell.set(Mode::Retitled));
    let lease = lease(&kernel, key);
    assert_eq!(lease.live().err(), Some(ERR_CANCELLED));
    // Made and dropped again; withdrawn at the end of a pass, not made again.
    assert_eq!((makes(), drops()), (2, 2));
    assert!(lease.refused());
    assert_eq!(lease.live().err(), Some(ERR_CANCELLED));
    assert_eq!(makes(), 2);
}

#[test]
fn the_plain_entry_never_parks() {
    let mut kernel = kernel(Mode::Parkable);
    let image = image().bytes;
    // SAFETY: as `admit`.
    unsafe {
        kernel.register_linked(
            entry(),
            METADATA,
            &shell_host(),
            "linked",
            Heap,
            HostResources::try_new().unwrap(),
            &FlatDepth::try_new().unwrap(),
            |_, _, _, _| {},
            |_, _, _, _| {},
            (image.as_ptr(), image.len()),
            FixedTables::NONE,
        )
    }
    .unwrap();
    let key = kernel.shared.native_instances.find("linked").unwrap();
    assert_eq!((makes(), drops()), (1, 0));
    assert!(!lease(&kernel, key).is_parked());
}

#[test]
fn inside_another_admission_a_parked_plugin_is_busy_and_starts_after() {
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    let lease = lease(&kernel, key);
    {
        // What a later plugin's `make` would reach it in: no safe point.
        let _hold = kernel.shared.native_instances.hold_unparking().unwrap();
        assert_eq!(
            lease.live().err(),
            Some(paperpaw_native::host_glue::ERR_BUSY)
        );
        assert_eq!(makes(), 1);
    }
    // Asked to start: made at the next safe point.
    assert!(kernel.native_unpark_pending());
    assert_eq!(makes(), 2);
    assert!(!lease.is_parked());
}

#[test]
fn a_refused_plugin_is_withdrawn_once_and_its_key_is_not_reused() {
    let mut kernel = kernel(Mode::Parkable);
    let resources = HostResources::try_new().unwrap();
    let key = admit_with(&mut kernel, resources.try_clone().unwrap());
    let _ = kernel.take_warnings();
    MODE.with(|cell| cell.set(Mode::Retitled));
    assert_eq!(lease(&kernel, key).live().err(), Some(ERR_CANCELLED));
    kernel.native_unpark_pending();
    kernel.native_unpark_pending();
    assert_eq!(kernel.take_warnings().len(), 1);
    let instances = &kernel.shared.native_instances;
    assert!(instances.id(key).is_none());
    assert!(instances.lease(key).is_none());
    assert!(!kernel.shared.registry.screens.contains_key("linked.screen"));
    // Admitted again, it is another owner: what named the old one misses.
    MODE.with(|cell| cell.set(Mode::Parkable));
    let again = admit_with(&mut kernel, resources);
    assert_ne!(again, key);
}

#[test]
fn only_no_room_or_a_busy_owner_waits_every_other_hosting_error_refuses() {
    use crate::native_host::Error as Lifecycle;
    use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE};
    let waits = [
        shell::Error::Allocation,
        shell::Error::Status(ERR_NO_MEMORY),
        shell::Error::Status(ERR_BUSY),
        // The plugin's own `make` out of room or busy, at a heap peak.
        shell::Error::Factory(ERR_NO_MEMORY),
        shell::Error::Factory(ERR_BUSY),
        shell::Error::Lifecycle(Lifecycle::Memory),
        shell::Error::Lifecycle(Lifecycle::Busy),
        shell::Error::Lifecycle(Lifecycle::Depth),
    ];
    for error in waits {
        let why = unparked(error);
        assert!(
            matches!(why, Unparked::NoMemory | Unparked::Busy),
            "{error:?}"
        );
        // Only no room is memory pressure (Senior, R3 on #1365).
        assert_eq!(
            why == Unparked::NoMemory,
            super::super::native_dispatch::status(error) == ERR_NO_MEMORY,
            "{error:?}"
        );
    }
    for status in [ERR_INVALID, ERR_STATE, ERR_CANCELLED] {
        for error in [shell::Error::Status(status), shell::Error::Factory(status)] {
            assert!(matches!(unparked(error), Unparked::Refused));
        }
    }
    for error in [
        shell::Error::Lifecycle(Lifecycle::State),
        shell::Error::Attempt,
        shell::Error::Plugin,
    ] {
        assert!(matches!(unparked(error), Unparked::Refused));
    }
}

/// Queues `event.action_run` for the parked fixture, as a menu row or a
/// button would (`native_action::queue_run`).
fn run_action(kernel: &crate::Kernel, key: u64) {
    let env = kernel.shared.env(kernel.now);
    assert!(super::super::native_action::queue_run(
        &kernel.shared.native_instances,
        key,
        93,
        "linked.act",
        &env,
    ));
}

fn actions_waiting(kernel: &crate::Kernel) -> usize {
    kernel
        .shared
        .native_instances
        .action_queue()
        .unwrap()
        .waiting()
}

#[test]
fn an_action_of_a_plugin_that_cannot_be_made_yet_waits_and_reaches_it_after() {
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    let _ = kernel.take_warnings();
    // No room at the first use: the plugin stays parked, the action waits.
    MODE.with(|cell| cell.set(Mode::NoRoom));
    run_action(&kernel, key);
    kernel.run_commands();
    assert_eq!(makes(), 2);
    assert!(lease(&kernel, key).is_parked());
    assert_eq!(actions_waiting(&kernel), 1);
    assert!(kernel.take_warnings().is_empty());
    // Within its wait nothing is tried again, and the action stays; the
    // device wakes when the wait is over, not at the next key press.
    kernel.run_commands();
    assert_eq!((makes(), actions_waiting(&kernel)), (2, 1));
    assert_eq!(
        kernel.service_due(),
        Some(paperpaw_platform::Instant(1_000))
    );
    // The wait over and room again: made, and the action delivered.
    MODE.with(|cell| cell.set(Mode::Parkable));
    kernel.now = paperpaw_platform::Instant(60_000);
    kernel.run_commands();
    assert_eq!(makes(), 3);
    assert!(!lease(&kernel, key).is_parked());
    assert_eq!(actions_waiting(&kernel), 0);
    assert_eq!(kernel.shared.native_instances.parked_start_at(), None);
}

fn open_screen(kernel: &mut crate::Kernel) {
    kernel.open(
        paperpaw_sdk::Id::parse("linked.screen").unwrap(),
        None,
        &paperpaw_sdk::Params::new(),
    );
    kernel.run_commands();
}

#[test]
fn a_screen_of_a_plugin_without_room_does_not_open_and_opens_once_it_is_made() {
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    let _ = kernel.take_warnings();
    // No room: no screen its plugin never heard of, the log says so and
    // the user is not told (as for any plugin without memory).
    MODE.with(|cell| cell.set(Mode::NoRoom));
    open_screen(&mut kernel);
    assert_eq!(makes(), 2);
    assert!(lease(&kernel, key).is_parked());
    assert!(kernel.stack.is_empty());
    let warnings = kernel.take_warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].message, "Could not open native screen");
    assert!(kernel.notice.is_none());
    // The wait over and room again (the plugins' clock is set each tick):
    // made as the screen opens.
    MODE.with(|cell| cell.set(Mode::Parkable));
    kernel.now = paperpaw_platform::Instant(60_000);
    kernel.shared.native_instances.set_now(60_000);
    open_screen(&mut kernel);
    assert_eq!(makes(), 3);
    assert!(!lease(&kernel, key).is_parked());
    assert_eq!(
        kernel.stack.last().map(|entry| entry.id.as_str()),
        Some("linked.screen")
    );
}

#[test]
fn a_parked_screen_opened_by_another_plugins_batch_is_made_there_and_the_batch_goes_on_in_order() {
    use paperpaw_sdk::{Command, Id, Params};
    let mut kernel = kernel(Mode::Parkable);
    let resources = HostResources::try_new().unwrap();
    let parked = admit_with(&mut kernel, resources.try_clone().unwrap());
    // A plugin that runs (its declarations copied), whose callback staged
    // an open of the parked plugin's screen, then one of its own.
    MODE.with(|cell| cell.set(Mode::Copied));
    let other = admit_as(&mut kernel, "other", resources);
    assert_eq!(makes(), 2);
    MODE.with(|cell| cell.set(Mode::Parkable));
    let commands = std::vec![
        Command::OpenScreen(Id::parse("linked.screen").unwrap(), Params::new()),
        Command::OpenScreen(Id::parse("other.screen").unwrap(), Params::new()),
    ];
    let staged = super::super::event_call::Staged::new(other, commands, None, std::vec![]);
    kernel.shared.native_instances.outbox.put(staged).unwrap();
    // The outbox is applied at depth 0, a safe point (Senior on #1337).
    // Both opens are applied there, in order, the parked plugin made
    // between them.
    kernel.native_outbox_run();
    assert_eq!(makes(), 3);
    assert!(!lease(&kernel, parked).is_parked());
    let stack: std::vec::Vec<_> = kernel.stack.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(stack, ["linked.screen", "other.screen"]);
}

#[test]
fn a_plugin_that_stays_without_room_gives_up_its_actions_and_a_later_one_reaches_it() {
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    let _ = kernel.take_warnings();
    MODE.with(|cell| cell.set(Mode::NoRoom));
    run_action(&kernel, key);
    run_action(&kernel, key);
    kernel.run_commands();
    // Each wake tries again, the wait doubling, until the longest wait:
    // then both its actions are dropped, each said once, and nothing wakes.
    let mut wakes = 0;
    while let Some(at) = kernel.service_due() {
        wakes += 1;
        assert!(wakes <= 8, "woken without end");
        kernel.now = at;
        kernel.run_commands();
    }
    // 1, 2, 4, 8, 16 and 32 s; the next wait is the longest.
    assert_eq!(wakes, 6);
    assert!(lease(&kernel, key).is_parked());
    assert_eq!(actions_waiting(&kernel), 0);
    let warnings = kernel.take_warnings();
    assert_eq!(warnings.len(), 2);
    assert!(
        warnings
            .iter()
            .all(|w| w.message.ends_with("not run: no memory"))
    );
    // A later action tries at once (the wait starts again): with room it
    // is made and the action delivered, unwoken.
    let tries = makes();
    MODE.with(|cell| cell.set(Mode::Parkable));
    kernel.now = paperpaw_platform::Instant(kernel.now.millis() + 1);
    run_action(&kernel, key);
    kernel.run_commands();
    assert_eq!(makes(), tries + 1);
    assert!(!lease(&kernel, key).is_parked());
    assert_eq!(actions_waiting(&kernel), 0);
}

#[test]
fn a_format_may_be_parked_a_stage_subscriber_may_not() {
    // D2 (#248): formats left condition 4; stage subscribers, lookup
    // providers and widgets stay awake.
    let format = wire::Record::FormatSpec(wire::FormatSpec {
        name: "books",
        title: "format.title",
        extensions: "epub",
        flags: 0,
        priority: 0,
        token: 5,
    });
    assert!(!nested_role(&format));
    let lookup = wire::Record::LookupSpec(wire::LookupSpec {
        name: "words",
        title: "lookup.title",
        languages: "en",
        flags: 0,
        token: 6,
    });
    assert!(nested_role(&lookup));
}

#[test]
fn a_format_that_stays_without_room_is_seen_to_give_up_once() {
    // Senior, R2 on #1365: what the document loop asks of the registry
    // (`fmt_call::Registry::gave_up`) for a pending open's parked format.
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    MODE.with(|cell| cell.set(Mode::NoRoom));
    let instances = &kernel.shared.native_instances;
    assert!(!instances.format_gave_up(key), "never tried yet");
    let mut now = kernel.now.millis();
    let mut tries = 0;
    while !instances.format_gave_up(key) {
        tries += 1;
        assert!(tries <= 10, "it never gave up");
        instances.set_now(now);
        assert!(lease(&kernel, key).live().is_err());
        now += 64_000;
    }
    // 1, 2, 4, 8, 16, 32 and 64 s of waits: tried seven times, taken once.
    assert_eq!(tries, 7);
    assert!(!instances.format_gave_up(key), "taken once");
    assert!(lease(&kernel, key).is_parked());
}

#[test]
fn a_format_without_room_gives_up_at_once_when_nothing_can_be_parked_again() {
    // Manager's decision 2 on #248: no memory to make a parked format, and
    // no idle plugin to park again for room: the pending open fails at the
    // next pass, as with the format awake, not after about two minutes of
    // waits. What a queued action asks (`give_up`) keeps its wait.
    let mut kernel = kernel(Mode::Parkable);
    let key = admit(&mut kernel);
    MODE.with(|cell| cell.set(Mode::NoRoom));
    kernel.shared.native_instances.set_now(kernel.now.millis());
    assert!(lease(&kernel, key).live().is_err());
    assert!(
        !kernel.shared.native_instances.format_gave_up(key),
        "not before parking again was tried"
    );
    assert!(!kernel.native_unpark_pending(), "nothing made");
    assert!(
        !lease(&kernel, key).slot().give_up(),
        "a queued action keeps its wait"
    );
    let instances = &kernel.shared.native_instances;
    assert!(instances.format_gave_up(key), "the open fails at once");
    assert!(!instances.format_gave_up(key), "taken once");
    assert!(lease(&kernel, key).is_parked());
    // Tried again at once by a later open (the wait started over).
    MODE.with(|cell| cell.set(Mode::Parkable));
    assert!(lease(&kernel, key).live().is_ok());
}
