//! Real ABI screen declarations are published through the kernel registry.
use super::*;
use crate::native_host::{flat::FlatDepth, registrations as wire};
use paperpaw_sdk::{Command, Handled, Id, Params};

std::thread_local! {
    static POOL: RefCell<Option<crate::native_host::HostResources>> = const { RefCell::new(None) };
    static UNSUPPORTED: Cell<bool> = const { Cell::new(false) };
    static INSTALL_FAIL_NTH: Cell<u32> = const { Cell::new(0) };
    static EMPTY: Cell<bool> = const { Cell::new(false) };
    static EXTRA_CALLBACK: Cell<bool> = const { Cell::new(false) };
    static CHECK_TEARDOWN: Cell<bool> = const { Cell::new(false) };
    static TEARDOWN_STATUS: Cell<i32> = const { Cell::new(0) };
    static FLAGS: Cell<u32> = const { Cell::new(0) };
    static DESTROYED: Cell<u32> = const { Cell::new(0) };
    static REPORTS: Cell<u32> = const { Cell::new(0) };
    static STAMPS: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
    static EVENTS: RefCell<Vec<(u32, u32, u32, u32)>> = const { RefCell::new(Vec::new()) };
}

struct ScreenState {
    table: PpPlugin,
    host: *const PpHost,
    context: *mut c_void,
}

unsafe extern "C" fn screen_make(
    host: *const PpHost,
    ctx: *mut c_void,
    out: *mut *const PpPlugin,
) -> i32 {
    let mut bytes = [0; 128];
    let records = [
        wire::Record::ScreenSpec(wire::ScreenSpec {
            name: "screen",
            title: "factory.title",
            token: 91,
            flags: FLAGS.with(Cell::get),
        }),
        wire::Record::ActionSpec(wire::ActionSpec {
            name: "action",
            title: "factory.action",
            token: 92,
            flags: 0,
        }),
    ];
    let count = if UNSUPPORTED.with(Cell::get) { 2 } else { 1 };
    let len = wire::records_encode(&records[..count], &mut bytes).unwrap();
    // SAFETY: the linked factory receives a live host table and context, and
    // its encoded declaration remains readable for the synchronous call.
    if !EMPTY.with(Cell::get) {
        let status = unsafe { ((*host).registrations_submit)(ctx, borrowed(&bytes[..len])) };
        if status != OK {
            return status;
        }
    }
    if EXTRA_CALLBACK.with(Cell::get) {
        let len = wire::registrations_encode(
            &[wire::Registration::Event {
                event_tag: wire::EVENT_STARTED_TAG,
                token: 91,
            }],
            &mut bytes,
        )
        .unwrap();
        // SAFETY: the encoded callback registration remains readable for submit.
        let status = unsafe { ((*host).registrations_submit)(ctx, borrowed(&bytes[..len])) };
        if status != OK {
            return status;
        }
    }
    let state = Box::into_raw(Box::new(ScreenState {
        host,
        context: ctx,
        table: PpPlugin {
            size: size_of::<PpPlugin>() as u32,
            state: ptr::null_mut(),
            drop: Some(screen_destroy),
            dispatch_event: Some(screen_event),
            input: None,
            draw: None,
        },
    }));
    // SAFETY: this unique initialized allocation is handed to the host once.
    unsafe {
        (*state).table.state = state.cast();
        out.write(&raw const (*state).table);
    }
    // Faults begin after the plugin allocation, in host mapping and publication.
    arm_failure(INSTALL_FAIL_NTH.with(Cell::get));
    OK
}
unsafe extern "C" fn screen_destroy(state: *mut c_void) {
    if CHECK_TEARDOWN.with(Cell::get) {
        // SAFETY: teardown retains the pinned host table through the destructor.
        let status = unsafe {
            let state = &*state.cast::<ScreenState>();
            ((*state.host).commands_submit)(
                state.context,
                PpBytes {
                    ptr: ptr::null(),
                    len: 0,
                },
            )
        };
        TEARDOWN_STATUS.with(|value| value.set(status));
    }
    DESTROYED.with(|value| value.set(value.get() + 1));
    // SAFETY: the host returns exactly the allocation published by screen_make.
    unsafe { drop(Box::from_raw(state.cast::<ScreenState>())) };
}
unsafe extern "C" fn screen_event(
    state: *mut c_void,
    token: u32,
    bytes: PpBytes,
    _: PpBuffer,
) -> i32 {
    if token != 91 {
        return ERR_INVALID;
    }
    // SAFETY: the owner retains the initialized fixture and its pinned context.
    let context = unsafe {
        &*(*state.cast::<ScreenState>())
            .context
            .cast::<Context<Heap>>()
    };
    let stamp = context
        .environment
        .with_frame(|frame| Ok::<_, i32>((frame.caller()?, frame.callback_epoch()?)));
    let (owner, epoch) = match stamp {
        Ok(Ok(stamp)) => stamp,
        _ => return host_glue::ERR_STATE,
    };
    if owner != context.resources.owner_id() {
        return host_glue::ERR_STATE;
    }
    STAMPS.with(|stamps| stamps.borrow_mut().push((owner, epoch)));

    // SAFETY: the callback borrows the host's initialized event for this call.
    let bytes = unsafe { core::slice::from_raw_parts(bytes.ptr, bytes.len) };
    let record = match wire::records_decode_single(bytes) {
        Ok(record) => record,
        Err(error) => return error,
    };
    let row = match record {
        wire::Record::EventScreenOpened(event) => (79, event.instance, event.flags, event.params),
        wire::Record::EventScreenShown(event) => (38, event.instance.unwrap_or(0), 0, 0),
        wire::Record::EventScreenClosed(event) => (80, event.instance, 0, 0),
        _ => return ERR_INVALID,
    };
    let instance = context
        .environment
        .with_frame(|frame| frame.calling_instance());
    if instance != Ok(Ok(row.1)) {
        return host_glue::ERR_STATE;
    }
    EVENTS.with(|events| events.borrow_mut().push(row));
    OK
}
fn host(flags: u32) -> Hosted<Heap, ()> {
    host_named("factory", flags)
}
fn host_named(owner: &str, flags: u32) -> Hosted<Heap, ()> {
    FLAGS.with(|value| value.set(flags));
    // SAFETY: the linked factory and code remain available throughout the owner lifetime.
    unsafe {
        Hosted::new(
            Heap,
            (),
            screen_make,
            owner,
            (ptr::null(), 0),
            |_, _, _, _| {},
            POOL.with(|pool| {
                let mut pool = pool.borrow_mut();
                if pool.is_none() {
                    *pool = Some(crate::native_host::HostResources::try_new().unwrap());
                }
                pool.as_ref().unwrap().try_clone().unwrap()
            }),
            DeclaredServices::none(),
        )
    }
    .unwrap()
}
fn install(kernel: &mut crate::Kernel, flags: u32) -> Result<(), Error> {
    kernel.register_native(host(flags), &FlatDepth::try_new().unwrap(), |_, _, _| {
        REPORTS.with(|value| value.set(value.get() + 1));
    })
}
fn kernel(flags: u32) -> crate::Kernel {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    REPORTS.with(|value| value.set(0));
    EVENTS.with(|events| events.borrow_mut().clear());
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    install(&mut kernel, flags).unwrap();
    kernel
}
fn open(kernel: &mut crate::Kernel, params: Params) {
    kernel.open_screen(
        paperpaw_platform::Instant(1),
        Id::new_static("factory.screen"),
        params,
    );
    let top = kernel.stack.len() - 1;
    kernel.with_screen(top, |screen, cx| {
        screen.on_show(cx);
        Handled::Yes
    });
}

#[test]
fn native_screen_factory_retains_owner_and_applies_instance_policy() {
    for (policy, same_params, expected) in
        [(0, true, 1), (0, false, 2), (1, false, 1), (2, true, 2)]
    {
        let mut kernel = kernel(policy << 6);
        assert_eq!(DESTROYED.with(Cell::get), 0);
        open(&mut kernel, Params::new().with("page", "one"));
        let first = kernel.stack[0].serial;
        open(
            &mut kernel,
            Params::new().with("page", if same_params { "one" } else { "two" }),
        );
        assert_eq!(kernel.stack.len(), expected);
        let events = EVENTS.with(|events| events.borrow().clone());
        let opened: Vec<_> = events.iter().filter(|event| event.0 == 79).collect();
        assert_eq!(opened.len(), 2);
        assert_eq!(opened[0], &(79, first, 0, 1));
        if expected == 1 {
            assert_eq!(kernel.stack[0].serial, first);
            assert_eq!(opened[1], &(79, first, 1, 1));
        } else {
            let second = kernel.stack[1].serial;
            assert_ne!(first, second);
            assert_eq!(opened[1], &(79, second, 0, 1));
            kernel.apply(Command::Close(paperpaw_sdk::ScreenKey::new(second)));
            assert!(EVENTS.with(|events| events.borrow().contains(&(80, second, 0, 0))));
        }
        assert!(
            events
                .iter()
                .filter(|event| event.0 == 38)
                .all(|event| event.1 != 0)
        );
        assert_eq!(DESTROYED.with(Cell::get), 0);
        drop(kernel);
        assert_eq!(DESTROYED.with(Cell::get), 1);
    }
}

#[test]
fn each_native_factory_allocation_failure_preserves_current_screen_and_layout() {
    let mut kernel = kernel(2 << 6);
    open(&mut kernel, Params::new());
    let serial = kernel.stack[0].serial;
    let next = kernel.next_serial;
    let content = kernel.shared.content;
    let dimensions = (
        kernel.shared.device.display.width,
        kernel.shared.device.display.height,
    );
    kernel.take_warnings();
    for allocation in [1, 2] {
        REPORTS.with(|value| value.set(0));
        arm_failure(allocation);
        kernel.open(Id::new_static("factory.screen"), None, &Params::new());
        arm_failure(0);
        assert_eq!(kernel.stack.len(), 1);
        assert_eq!(kernel.stack[0].serial, serial);
        assert_eq!(kernel.next_serial, next);
        assert_eq!(kernel.shared.content, content);
        assert_eq!(
            (
                kernel.shared.device.display.width,
                kernel.shared.device.display.height
            ),
            dimensions
        );
        assert_eq!(REPORTS.with(Cell::get), 1);
        assert_eq!(DESTROYED.with(Cell::get), 0);
        let warnings = kernel.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].message, "Could not open native screen");
    }
    open(&mut kernel, Params::new());
    assert_eq!(kernel.stack.len(), 2);
}

#[test]
fn duplicate_native_screen_install_is_atomic_and_reserved_policy_is_refused() {
    let mut kernel = kernel(0);
    let before = kernel.shared.registry.screens.list().iter().count();
    assert!(install(&mut kernel, 0).is_err());
    assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
    assert_eq!(
        DESTROYED.with(Cell::get),
        1,
        "only the refused owner was destroyed"
    );
    open(&mut kernel, Params::new());
    assert_eq!(kernel.stack.len(), 1);
    assert!(matches!(
        install(&mut kernel, 3 << 6),
        Err(Error::Factory(host_glue::ERR_UNSUPPORTED))
    ));
    assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
}

#[test]
fn unsupported_native_family_discards_every_staged_screen() {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let before = kernel.shared.registry.screens.list().iter().count();
    UNSUPPORTED.with(|value| value.set(true));
    let result = install(&mut kernel, 0);
    UNSUPPORTED.with(|value| value.set(false));
    assert!(matches!(
        result,
        Err(Error::Status(host_glue::ERR_UNSUPPORTED))
    ));
    assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn every_post_factory_host_allocation_refuses_without_partial_publication() {
    let mut reached_success = false;
    for allocation in 1..48 {
        reset(NORMAL);
        DESTROYED.with(|value| value.set(0));
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        kernel.shared.actions.insert(
            paperpaw_sdk::ActionSpec::new(Id::new_static("legacy.action"), "legacy.action")
                .sealed(),
        );
        kernel.shared.actions.add_override(
            Id::new_static("legacy.action"),
            paperpaw_sdk::Priority(0),
            Id::new_static("legacy"),
            Box::new(|_, _, _| {}),
        );
        let before = kernel.shared.registry.screens.list().iter().count();
        INSTALL_FAIL_NTH.with(|value| value.set(allocation));
        let result = install(&mut kernel, 0);
        arm_failure(0);
        INSTALL_FAIL_NTH.with(|value| value.set(0));
        // Public lookup hides overrides of sealed actions. Unseal the fixture
        // after the fault window to observe whether publication removed it.
        kernel.shared.actions.insert(paperpaw_sdk::ActionSpec::new(
            Id::new_static("legacy.action"),
            "legacy.action",
        ));
        assert!(kernel.shared.actions.override_of("legacy.action").is_some());
        if result.is_ok() {
            reached_success = true;
            assert_eq!(
                kernel.shared.registry.screens.list().iter().count(),
                before + 1
            );
            assert_eq!(DESTROYED.with(Cell::get), 0);
            let declarations = kernel.shared.registry.native_declarations.as_ref().unwrap();
            let mut metadata = 0;
            declarations.store.borrow().find(
                "factory",
                crate::native_host::declaration_store::Kind::Screen,
                "screen",
                &mut |_| metadata += 1,
            );
            assert_eq!(metadata, 1);
            drop(kernel);
            assert_eq!(DESTROYED.with(Cell::get), 1);
            break;
        }
        assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
        if let Some(declarations) = &kernel.shared.registry.native_declarations {
            assert_eq!(
                declarations.store.borrow().host_owner("factory"),
                Err(host_glue::ERR_NOT_FOUND)
            );
        }
        assert_eq!(DESTROYED.with(Cell::get), 1);
    }
    assert!(
        reached_success,
        "the fault walk reached a fully admitted owner"
    );
}

#[test]
fn empty_or_unmapped_callback_install_is_refused_without_publication() {
    for (empty, callback) in [(true, false), (false, true)] {
        reset(NORMAL);
        DESTROYED.with(|value| value.set(0));
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        let before = kernel.shared.registry.screens.list().iter().count();
        EMPTY.with(|value| value.set(empty));
        EXTRA_CALLBACK.with(|value| value.set(callback));
        let result = install(&mut kernel, 0);
        EMPTY.with(|value| value.set(false));
        EXTRA_CALLBACK.with(|value| value.set(false));
        assert!(matches!(
            result,
            Err(Error::Status(host_glue::ERR_UNSUPPORTED))
        ));
        assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
        assert_eq!(DESTROYED.with(Cell::get), 1);
    }
}

#[test]
fn native_screen_below_guard_is_opened_and_closed_without_being_shown() {
    let mut kernel = kernel(1 << 1);
    for owner in ["base", "other"] {
        kernel
            .register_native(
                host_named(owner, 0),
                &FlatDepth::try_new().unwrap(),
                |_, _, _| {},
            )
            .unwrap();
    }
    kernel.open_screen(
        paperpaw_platform::Instant(1),
        Id::new_static("base.screen"),
        Params::new(),
    );
    open(&mut kernel, Params::new());
    assert_eq!(kernel.guard(), Some(1));
    kernel.open_screen(
        paperpaw_platform::Instant(2),
        Id::new_static("other.screen"),
        Params::new(),
    );
    let hidden = kernel
        .stack
        .iter()
        .find(|screen| screen.id.as_str() == "other.screen")
        .unwrap()
        .serial;
    assert_eq!(kernel.stack.last().unwrap().id.as_str(), "factory.screen");
    assert!(EVENTS.with(|events| events.borrow().contains(&(79, hidden, 0, 0))));
    assert!(!EVENTS.with(|events| {
        events
            .borrow()
            .iter()
            .any(|event| event.0 == 38 && event.1 == hidden)
    }));
    kernel.apply(Command::Close(paperpaw_sdk::ScreenKey::new(hidden)));
    assert!(EVENTS.with(|events| events.borrow().contains(&(80, hidden, 0, 0))));
}

#[test]
fn duplicate_native_install_allocation_faults_keep_original_owner_registered() {
    let mut kernel = kernel(0);
    let before = kernel.shared.registry.screens.list().iter().count();
    for allocation in 1..32 {
        DESTROYED.with(|value| value.set(0));
        INSTALL_FAIL_NTH.with(|value| value.set(allocation));
        let result = install(&mut kernel, 0);
        arm_failure(0);
        INSTALL_FAIL_NTH.with(|value| value.set(0));
        assert!(matches!(
            result,
            Err(Error::Status(host_glue::ERR_NO_MEMORY | ERR_INVALID))
        ));
        assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
        assert_eq!(DESTROYED.with(Cell::get), 1);
    }
    open(&mut kernel, Params::new());
    assert_eq!(kernel.stack.len(), 1);
}

#[test]
fn retiring_open_registered_owner_refuses_callbacks_and_tears_down_under_kernel_borrow() {
    let mut kernel = kernel(0);
    open(&mut kernel, Params::new());
    let before = EVENTS.with(|events| events.borrow().len());
    let owner = kernel.shared.native_instances.find("factory").unwrap();
    assert!(
        kernel
            .shared
            .native_instances
            .retire(owner)
            .unwrap()
            .is_none()
    );
    kernel.with_screen(0, |screen, cx| {
        screen.on_show(cx);
        Handled::Yes
    });
    assert_eq!(EVENTS.with(|events| events.borrow().len()), before);
    assert_eq!(DESTROYED.with(Cell::get), 0);
    CHECK_TEARDOWN.with(|value| value.set(true));
    release_under_borrow(&mut kernel);
    CHECK_TEARDOWN.with(|value| value.set(false));
    assert_eq!(DESTROYED.with(Cell::get), 1);
    assert_eq!(TEARDOWN_STATUS.with(Cell::get), host_glue::ERR_STATE);
}
fn release_under_borrow(kernel: &mut crate::Kernel) {
    kernel.shared.registry = crate::registry::Registry::default();
    assert_eq!(
        DESTROYED.with(Cell::get),
        0,
        "the open screen retains its owner"
    );
    kernel.stack.clear();
    kernel.shared.stack.clear();
}

#[test]
fn wait_ready_flag_is_refused_until_its_host_policy_is_wired() {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    assert!(matches!(
        install(&mut kernel, 1 << 3),
        Err(Error::Factory(host_glue::ERR_UNSUPPORTED))
    ));
}

#[test]
fn real_callback_stamps_use_registered_owner_and_distinct_live_epochs() {
    let mut kernel = kernel(0);
    STAMPS.with(|stamps| stamps.borrow_mut().clear());
    let owner = kernel.shared.native_instances.find("factory").unwrap();
    open(&mut kernel, Params::new());
    open(&mut kernel, Params::new());
    let stamps = STAMPS.with(|stamps| stamps.borrow().clone());
    assert!(stamps.len() >= 3);
    assert!(stamps.iter().all(|stamp| stamp.0 == owner && stamp.1 != 0));
    assert!(stamps.windows(2).all(|pair| pair[0].1 < pair[1].1));
    assert_eq!(kernel.shared.native_instances.depth(), 0);
}

#[test]
fn repeated_failed_admissions_reclaim_rows_and_recover_in_the_same_kernel() {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let before = kernel.shared.registry.screens.list().iter().count();
    let mut capacity = None;
    let mut recovered = false;
    for allocation in 1..64 {
        INSTALL_FAIL_NTH.with(|value| value.set(allocation));
        let result = install(&mut kernel, 0);
        arm_failure(0);
        INSTALL_FAIL_NTH.with(|value| value.set(0));
        let (rows, reserved) = kernel.shared.native_instances.retained_rows();
        if result.is_ok() {
            assert_eq!(rows, 1);
            let key = kernel.shared.native_instances.find("factory").unwrap();
            assert!(key > 1, "multiple failed admissions consumed unique keys");
            open(&mut kernel, Params::new());
            assert_eq!(kernel.stack.len(), 1);
            recovered = true;
            break;
        }
        assert_eq!(rows, 0, "failed admission must release its row");
        if reserved != 0 {
            assert_eq!(*capacity.get_or_insert(reserved), reserved);
        }
        assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
        assert_eq!(DESTROYED.with(Cell::get), allocation);
    }
    assert!(
        recovered,
        "a later retry must install and open successfully"
    );
}

#[test]
fn native_unload_cleans_up_and_reinstall_uses_a_fresh_key() {
    let mut kernel = kernel(0);
    open(&mut kernel, Params::new());
    let owner = Id::new_static("factory");
    let old = kernel.shared.native_instances.find("factory").unwrap();
    CHECK_TEARDOWN.with(|value| value.set(true));
    kernel.unload_plugin(&owner);
    CHECK_TEARDOWN.with(|value| value.set(false));
    assert!(kernel.shared.native_instances.id(old).is_none());
    assert_eq!(kernel.shared.native_instances.retained_rows().0, 0);
    assert!(
        !kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
    assert!(kernel.stack.is_empty());
    assert_eq!(DESTROYED.with(Cell::get), 1);
    assert_eq!(TEARDOWN_STATUS.with(Cell::get), host_glue::ERR_STATE);
    install(&mut kernel, 0).unwrap();
    let new = kernel.shared.native_instances.find("factory").unwrap();
    assert!(new > old);
    open(&mut kernel, Params::new());
    assert_eq!(kernel.stack.len(), 1);
}

#[test]
fn failed_publication_reclaims_row_then_retry_publishes_under_a_fresh_key() {
    let mut kernel = kernel(0);
    let old = kernel.shared.native_instances.find("factory").unwrap();
    // Keep the old factory to force a conflict at publication, after binding.
    assert!(
        kernel
            .shared
            .native_instances
            .retire(old)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        install(&mut kernel, 0),
        Err(Error::Status(ERR_INVALID))
    ));
    assert_eq!(kernel.shared.native_instances.retained_rows().0, 0);
    kernel
        .shared
        .registry
        .screens
        .remove_native_owner("factory");
    install(&mut kernel, 0).unwrap();
    let new = kernel.shared.native_instances.find("factory").unwrap();
    assert!(
        new > old + 1,
        "the refused publication consumed its own identity"
    );
    open(&mut kernel, Params::new());
    assert_eq!(kernel.stack.len(), 1);
}

#[test]
fn one_unload_request_continues_cleanup_and_retries_busy_retirement_on_tick() {
    let mut kernel = kernel(0);
    open(&mut kernel, Params::new());
    let owner = Id::new_static("factory");
    let key = kernel.shared.native_instances.find("factory").unwrap();
    kernel
        .shared
        .data_folders
        .push((owner.clone(), "retiring-plugin".into()));
    crate::native_host::shell::teardown_fault::arm_persistent(
        crate::native_host::TeardownStep::Release,
        crate::native_host::Error::Busy,
    );
    kernel.unload_plugin(&owner);
    assert!(kernel.shared.native_instances.id(key).is_none());
    assert_eq!(kernel.shared.native_instances.find("factory"), Some(key));
    assert!(
        kernel
            .shared
            .data_folders
            .iter()
            .all(|(id, _)| id != &owner)
    );
    assert!(kernel.stack.is_empty());
    assert!(
        !kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
    assert_eq!(DESTROYED.with(Cell::get), 0);
    let effects = kernel.tick(paperpaw_platform::Instant(10));
    let wake = effects
        .wake_at()
        .expect("pending cleanup schedules a main-loop wake");
    assert!(wake <= paperpaw_platform::Instant(60));
    crate::native_host::shell::teardown_fault::disarm();
    kernel.tick(wake);
    assert_eq!(kernel.shared.native_instances.find("factory"), None);
    assert_eq!(DESTROYED.with(Cell::get), 1);
    install(&mut kernel, 0).unwrap();
    assert!(kernel.shared.native_instances.find("factory").unwrap() > key);
}

#[test]
fn retained_host_stop_enters_the_same_kernel_unload_path() {
    let mut kernel = kernel(0);
    open(&mut kernel, Params::new());
    let key = kernel.shared.native_instances.find("factory").unwrap();
    let crate::native_host::native_instances::InstanceState::Running(lease) =
        kernel.shared.native_instances.state(key).unwrap()
    else {
        panic!("running owner");
    };
    lease.stop();
    kernel.tick(paperpaw_platform::Instant(10));
    assert_eq!(kernel.shared.native_instances.find("factory"), None);
    assert!(
        !kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
    assert!(kernel.stack.is_empty());
    assert_eq!(
        DESTROYED.with(Cell::get),
        0,
        "the test lease still pins the owner"
    );
    drop(lease);
    assert_eq!(DESTROYED.with(Cell::get), 1);
    install(&mut kernel, 0).unwrap();
    assert!(kernel.shared.native_instances.find("factory").unwrap() > key);
}

#[test]
fn replacement_publication_waits_for_one_requested_busy_unload_to_complete() {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let storage = paperpaw_sdk::MemoryStorage::new().with(
        "plugins/.factory.part/plugin.ini",
        "[plugin]\nid = factory\nname = Replacement\nversion = 2.0.0\n",
    );
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(storage)
        .build()
        .unwrap();
    install(&mut kernel, 0).unwrap();
    let old = kernel.shared.native_instances.find("factory").unwrap();
    crate::native_host::shell::teardown_fault::arm_persistent(
        crate::native_host::TeardownStep::Release,
        crate::native_host::Error::Busy,
    );
    kernel.apply(Command::InstallPlugin(
        Id::new_static("factory"),
        "plugins/.factory.part".into(),
    ));
    assert!(
        kernel
            .shared
            .native_instances
            .replacement_pending(&Id::new_static("factory"))
    );
    assert!(
        kernel.plugins().get("factory").is_none(),
        "new declarations must not publish during cleanup"
    );
    let wake = kernel
        .tick(paperpaw_platform::Instant(10))
        .wake_at()
        .unwrap();
    assert!(kernel.shared.native_instances.id(old).is_none());
    assert!(kernel.plugins().get("factory").is_none());
    crate::native_host::shell::teardown_fault::disarm();
    kernel.tick(wake);
    assert_eq!(kernel.shared.native_instances.find("factory"), None);
    assert_eq!(DESTROYED.with(Cell::get), 1);
    let replacement = kernel
        .plugins()
        .get("factory")
        .expect("completion publishes replacement");
    assert_eq!(replacement.manifest.version.major, 2);
    kernel.tick(paperpaw_platform::Instant(wake.millis() + 50));
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn kernel_main_loop_drains_more_than_sixteen_orphan_retirements() {
    use crate::native_host::doc_service::{DocHost, Limits, tests::Fake};
    let mut kernel = kernel(0);
    let pool = POOL.with(|pool| pool.borrow().as_ref().unwrap().try_clone().unwrap());
    let documents = Fake::new(Limits::LARGE);
    pool.hosts().wire_documents(documents.clone()).unwrap();
    let owners: Vec<_> = (0..40).map(|_| pool.try_owner().unwrap()).collect();
    let wake;
    {
        let _busy = documents.table().hold().unwrap();
        drop(owners);
        assert!(pool.hosts().retirements_pending());
        assert!(kernel.native_unload_due().is_some());
        wake = kernel
            .tick(paperpaw_platform::Instant(10))
            .wake_at()
            .unwrap();
        assert!(pool.hosts().retirements_pending());
    }
    kernel.tick(wake);
    assert!(!pool.hosts().retirements_pending());
    assert!(kernel.native_unload_due().is_none());
    assert!(
        kernel.shared.native_instances.find("factory").is_some(),
        "orphan retries preserve a different live owner"
    );
}

#[test]
fn first_failed_admission_retains_pool_for_orphan_cleanup_before_any_row_exists() {
    use crate::native_host::doc_service::{DocHost, Limits, tests::Fake};
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let pool = crate::native_host::HostResources::try_new().unwrap();
    let documents = Fake::new(Limits::LARGE);
    pool.hosts().wire_documents(documents.clone()).unwrap();
    POOL.with(|slot| *slot.borrow_mut() = Some(pool.try_clone().unwrap()));
    let hosted = host(0);
    let depth = FlatDepth::try_new().unwrap();
    assert_eq!(kernel.shared.native_instances.retained_rows().0, 0);
    {
        let _busy = documents.table().hold().unwrap();
        EMPTY.with(|value| value.set(true));
        let result = kernel.register_native(hosted, &depth, |_, _, _| {});
        EMPTY.with(|value| value.set(false));
        assert!(matches!(
            result,
            Err(Error::Status(host_glue::ERR_UNSUPPORTED))
        ));
        assert_eq!(kernel.shared.native_instances.retained_rows().0, 0);
        assert!(pool.hosts().retirements_pending());
        assert!(kernel.native_unload_due().is_some());
        assert!(
            kernel
                .tick(paperpaw_platform::Instant(1))
                .wake_at()
                .is_some()
        );
        assert!(pool.hosts().retirements_pending());
    }
    kernel.tick(paperpaw_platform::Instant(51));
    assert!(!pool.hosts().retirements_pending());
    assert!(kernel.native_unload_due().is_none());
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn expired_directory_refuses_a_native_callback_before_entering_plugin_code() {
    use crate::native_host::{
        native_dispatch::NativeDispatch, native_screen::Dispatch, retained::Retained,
    };
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let directory = crate::native_host::shared::Shared::try_new(
        crate::native_host::native_declarations::NativeDeclarations::default(),
    )
    .unwrap();
    let mut flat_host = kernel.flat_host();
    flat_host.directory = directory.downgrade();
    let (retained, ()) = Retained::make_native(host(0), |_| Ok(())).unwrap();
    let dispatch = NativeDispatch::new(
        retained,
        FlatDepth::try_new().unwrap(),
        flat_host,
        |_, _, _| {},
        crate::native_host::native_instances::test_identity("factory"),
    );
    drop(directory);
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    assert_eq!(dispatch.event(91, &[0], &env), Err(host_glue::ERR_STATE));
    assert_eq!(DESTROYED.with(Cell::get), 0);
    drop(dispatch);
    assert_eq!(DESTROYED.with(Cell::get), 1);
}
