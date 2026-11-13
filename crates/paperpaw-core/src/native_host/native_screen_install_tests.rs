//! Real ABI screen declarations are published through the kernel registry.
use super::*;
use crate::native_host::{flat::FlatDepth, registrations as wire};
use paperpaw_sdk::{Command, Handled, Id, Params};

std::thread_local! {
    static POOL: RefCell<Option<crate::native_host::HostResources>> = const { RefCell::new(None) };
    static UNSUPPORTED: Cell<bool> = const { Cell::new(false) };
    /// The host table each `make` was handed, in order.
    static TABLES: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static INSTALL_FAIL_NTH: Cell<u32> = const { Cell::new(0) };
    /// 1: the batch also declares a group, a setting, its extra and an action;
    /// 2: and a secret setting the host does not map (the batch is refused).
    pub(super) static ARMS: Cell<u8> = const { Cell::new(0) };
    /// Another drawable the batch declares: 1 a status item of token 93; 2 a
    /// status line of token 94; 3 a status item of the screen's token 91.
    static DRAWABLE: Cell<u8> = const { Cell::new(0) };
    /// Resource records the batch also declares (see `resources`).
    static RES: Cell<u8> = const { Cell::new(0) };
    static EMPTY: Cell<bool> = const { Cell::new(false) };
    static MENUS: Cell<u8> = const { Cell::new(0) };
    /// 1: also a stage and a subscription to another plugin's; 2: also a
    /// subscription to its own stage.
    static STAGES: Cell<u8> = const { Cell::new(0) };
    /// The callback registrations `make` submits: 1 an event; 2 the same event
    /// twice; 3 a draw of the screen's token; 4 a draw of a token nobody
    /// declared; 5 the same draw twice; 6 an event and a draw; 7 and 8 the same event or
    /// draw in two batches; 9 an event and a draw in a batch each.
    static REGS: Cell<u8> = const { Cell::new(0) };
    static CHECK_TEARDOWN: Cell<bool> = const { Cell::new(false) };
    static TEARDOWN_STATUS: Cell<i32> = const { Cell::new(0) };
    static FLAGS: Cell<u32> = const { Cell::new(0) };
    pub(super) static DESTROYED: Cell<u32> = const { Cell::new(0) };
    static REPORTS: Cell<u32> = const { Cell::new(0) };
    static STAMPS: RefCell<Vec<(u64, u64)>> = const { RefCell::new(Vec::new()) };
    static EVENTS: RefCell<Vec<(u32, u32, u32, u32)>> = const { RefCell::new(Vec::new()) };
    /// What each `screen_opened` said of the hand-over: instance, hand-over,
    /// params, text bytes.
    static OPENED: RefCell<Vec<(u32, u32, u32, u32)>> = const { RefCell::new(Vec::new()) };
}

struct ScreenState {
    table: PpPlugin,
    host: *const PpHost,
    context: *mut c_void,
}

/// A screen, a stage and a subscription (to its own stage when `STAGES` is 2);
/// with `STAGES` 3, a screen and two setup steps (4: one named like the screen).
fn stage_fixture() -> [wire::Record<'static>; 3] {
    let step = |name| {
        wire::Record::SetupStep(wire::SetupStep {
            name,
            title: "factory.step",
            help: "",
            order: 0,
            page_kind: 1,
            page: "factory.screen",
            applies_key: "",
            applies_value: "",
            need: 0,
            capability: 0,
            flags: 1,
        })
    };
    if STAGES.with(Cell::get) >= 3 {
        return [
            wire::Record::ScreenSpec(wire::ScreenSpec {
                name: "screen",
                title: "factory.title",
                token: 91,
                flags: FLAGS.with(Cell::get),
            }),
            step(if STAGES.with(Cell::get) == 4 {
                "screen"
            } else {
                "setup"
            }),
            step("second"),
        ];
    }
    [
        wire::Record::ScreenSpec(wire::ScreenSpec {
            name: "screen",
            title: "factory.title",
            token: 91,
            flags: FLAGS.with(Cell::get),
        }),
        wire::Record::StageSpec(wire::StageSpec {
            name: "insert",
            schema: 1,
            flags: 0,
            max_input: 16,
            max_reply: 16,
            max_records: 1,
        }),
        wire::Record::RegistrationStage(wire::RegistrationStage {
            stage: if STAGES.with(Cell::get) == 2 {
                "factory.insert"
            } else {
                "other.insert"
            },
            token: 91,
            order: 0,
            signature: 0,
        }),
    ]
}

/// The batch `screen_make` submits, by the mode: the screen alone, with the
/// unsupported records, or with the group, setting, extra and action.
fn declaration_batch(bytes: &mut [u8]) -> usize {
    let arms = ARMS.with(Cell::get);
    let records = [
        wire::Record::ScreenSpec(wire::ScreenSpec {
            name: "screen",
            title: "factory.title",
            token: 91,
            flags: FLAGS.with(Cell::get),
        }),
        // A secret setting passes the declaration checks, and the host does
        // not map it yet (`native_values::setting`): the whole batch is refused.
        wire::Record::SettingGroup(wire::SettingGroup {
            name: "g",
            title: "factory.group",
            order: 0,
            parent: "",
            page: "",
        }),
        wire::Record::SettingSpec(wire::SettingSpec {
            name: "secret",
            kind: 0,
            flags: 1 << 3,
            group: "factory.g",
            title: "factory.secret",
            help: "",
            default: &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
            min: 0,
            max: 0,
            choices: "",
        }),
    ];
    let count = if UNSUPPORTED.with(Cell::get) { 3 } else { 1 };
    let flag = wire::Record::SettingSpec(wire::SettingSpec {
        name: "flag",
        kind: 0,
        flags: 0,
        group: "factory.g",
        title: "factory.flag",
        help: "",
        default: &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
        min: 0,
        max: 0,
        choices: "",
    });
    let extra = wire::Record::SettingExtra(wire::SettingExtra {
        name: "flag",
        unit: "",
        section: "",
        follows: "",
        shown_id: "",
        shown_value: "",
        editor_kind: 0,
        editor: "",
        step: 0,
        flags: 0,
        check: 0,
    });
    let action = wire::Record::ActionSpec(wire::ActionSpec {
        name: "action",
        title: "factory.action",
        token: 92,
        flags: 0,
    });
    let all = [records[0], records[1], flag, extra, action, records[2]];
    let item = |token| {
        wire::Record::StatusItem(wire::StatusItem {
            name: "item",
            title: "factory.item",
            tap: "",
            priority: 0,
            min_width: 0,
            token,
            flags: 0,
        })
    };
    let line = wire::Record::StatusLine(wire::StatusLine {
        name: "line",
        title: "factory.line",
        height: 1,
        token: 94,
        flags: 0,
    });
    match arms {
        0 if RES.with(Cell::get) != 0 => resources::batch(RES.with(Cell::get), records[0], bytes),
        0 if DRAWABLE.with(Cell::get) != 0 => {
            let more = match DRAWABLE.with(Cell::get) {
                1 => item(93),
                2 => line,
                _ => item(91),
            };
            wire::records_encode(&[records[0], more], bytes)
        }
        0 => wire::records_encode(&records[..count], bytes),
        // The group, the setting and its extra, the action and the screen;
        // the secret setting last when the batch must be refused.
        1 => wire::records_encode(&all[..5], bytes),
        _ => wire::records_encode(&[all[1], all[2], all[3], all[4], all[0], all[5]], bytes),
    }
    .unwrap()
}

/// Submits the callback registrations of the mode `REGS`; the refusing status
/// when the host refuses them.
fn submit_registrations(host: *const PpHost, ctx: *mut c_void) -> Option<i32> {
    let event = |token| wire::Registration::Event {
        event_tag: wire::EVENT_STARTED_TAG,
        token,
    };
    let draw = |token| wire::Registration::Draw { token };
    let tagged = |event_tag, token| wire::Registration::Event { event_tag, token };
    let batches: &[&[wire::Registration]] = match REGS.with(Cell::get) {
        0 => return None,
        1 => &[&[event(91)]],
        2 => &[&[event(91), event(92)]],
        3 => &[&[draw(91)]],
        // The draws of a status item and of a status line.
        10 => &[&[draw(93)]],
        11 => &[&[draw(94)]],
        // A token that two declarations claim.
        12 => &[&[draw(91)]],
        4 => &[&[draw(77)]],
        5 => &[&[draw(91), draw(91)]],
        6 => &[&[event(91), draw(91)]],
        // The same event in two batches; the same draw in two batches.
        7 => &[&[event(91)], &[event(92)]],
        8 => &[&[draw(91)], &[draw(91)]],
        15 => &[
            &[
                tagged(3, 95),
                tagged(32, 96),
                tagged(33, 97),
                tagged(34, 98),
                tagged(35, 99),
                tagged(36, 100),
                tagged(37, 101),
                tagged(65, 102),
            ],
            &[tagged(66, 103), tagged(67, 104)],
        ],
        // The book events 65-67 and suspend, each its own token (95-98).
        14 => &[&[
            tagged(65, 95),
            tagged(66, 96),
            tagged(67, 97),
            tagged(34, 98),
        ]],
        // `event.widget_changed`, which a maker registers for (`widget_events`).
        13 => &[&[tagged(wire::EVENT_WIDGET_CHANGED_TAG, 91)]],
        // An event and a draw, each in a batch of its own.
        _ => &[&[event(91)], &[draw(91)]],
    };
    for batch in batches {
        let mut bytes = [0; 128];
        let len = wire::registrations_encode(batch, &mut bytes).unwrap();
        // SAFETY: the encoded registrations remain readable for the call.
        let status = unsafe { ((*host).registrations_submit)(ctx, borrowed(&bytes[..len])) };
        if status != OK {
            return Some(status);
        }
    }
    None
}

unsafe extern "C" fn screen_make(
    host: *const PpHost,
    ctx: *mut c_void,
    out: *mut *const PpPlugin,
) -> i32 {
    TABLES.with(|tables| tables.borrow_mut().push(host.addr()));
    let mut bytes = [0; 1024];
    let len = if STAGES.with(Cell::get) == 0 {
        declaration_batch(&mut bytes)
    } else {
        wire::records_encode(&stage_fixture(), &mut bytes).unwrap()
    };
    // SAFETY: the linked factory receives a live host table and context, and
    // its encoded declaration remains readable for the synchronous call.
    if !EMPTY.with(Cell::get) {
        let status = unsafe { ((*host).registrations_submit)(ctx, borrowed(&bytes[..len])) };
        if status != OK {
            return status;
        }
    }
    let menus = MENUS.with(Cell::get);
    if menus != 0 {
        // SAFETY: the same live host table and context as the submit above.
        let status = unsafe {
            if menus >= key_records::FIRST {
                key_records::submit(host, ctx, menus)
            } else {
                menu_records::submit(host, ctx, menus)
            }
        };
        if status != OK {
            return status;
        }
    }
    if let Some(status) = submit_registrations(host, ctx) {
        return status;
    }
    let state = Box::into_raw(Box::new(ScreenState {
        host,
        context: ctx,
        table: PpPlugin {
            size: size_of::<PpPlugin>() as u32,
            state: ptr::null_mut(),
            drop: Some(screen_destroy),
            dispatch_event: Some(screen_event),
            input: Some(input_records::screen_input),
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
        return events_registered::told(token, bytes);
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
    input_records::events_submit(state, bytes);
    let record = match wire::records_decode_single(bytes) {
        Ok(record) => record,
        Err(error) => return error,
    };
    let row = match record {
        wire::Record::EventScreenOpened(event) => {
            OPENED.with(|opened| {
                opened.borrow_mut().push((
                    event.instance,
                    event.handover,
                    event.params,
                    event.text_bytes,
                ));
            });
            (79, event.instance, event.flags, event.params)
        }
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
pub(super) fn host(flags: u32) -> Hosted<Heap, ()> {
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
            DeclaredServices::none().with_features(input_records::FEATURES.with(Cell::get)),
        )
    }
    .unwrap()
}
/// The device's depth counter: the first plugin's, once there is one.
fn device_depth(kernel: &crate::Kernel) -> FlatDepth {
    match kernel.shared.native_depth.as_ref() {
        Some(depth) => depth.try_clone().unwrap(),
        None => FlatDepth::try_new().unwrap(),
    }
}

/// Installs with the resource records of mode `res`, in the plugin's folder.
fn install_resources(
    kernel: &mut crate::Kernel,
    res: u8,
    folder: Option<&str>,
) -> Result<(), Error> {
    RES.with(|value| value.set(res));
    let result = kernel.register_native_in(host(0), &device_depth(kernel), folder, |_, _, _, _| {});
    RES.with(|value| value.set(0));
    result
}

/// The same without the refresh of the font choices, which allocates as the
/// kernel does at start-up: the allocation-failure sweeps stop at the end of
/// the admission.
fn admit_resources(kernel: &mut crate::Kernel, res: u8, folder: Option<&str>) -> Result<(), Error> {
    RES.with(|value| value.set(res));
    let result = kernel.admit_native(host(0), &device_depth(kernel), folder, |_, _, _, _| {});
    RES.with(|value| value.set(0));
    result
}

#[test]
fn a_plugin_with_a_depth_counter_of_its_own_is_refused() {
    let mut kernel = kernel(1 << 1);
    let depth = device_depth(&kernel);
    // The device's counter again is fine; another one would double the budget.
    for name in ["one", "two"] {
        let hosted = host_named(name, 0);
        kernel
            .register_native(hosted, &depth, |_, _, _, _| {})
            .unwrap();
    }
    let other = FlatDepth::try_new().unwrap();
    let result = kernel.register_native(host_named("three", 0), &other, |_, _, _, _| {});
    assert!(matches!(result, Err(Error::Status(host_glue::ERR_STATE))));
    assert!(kernel.shared.native_instances.find("three").is_none());
}

pub(super) fn install(kernel: &mut crate::Kernel, flags: u32) -> Result<(), Error> {
    kernel.register_native(host(flags), &device_depth(kernel), |_, _, _, _| {
        REPORTS.with(|value| value.set(value.get() + 1));
    })
}
pub(super) fn kernel(flags: u32) -> crate::Kernel {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    REPORTS.with(|value| value.set(0));
    EVENTS.with(|events| events.borrow_mut().clear());
    OPENED.with(|opened| opened.borrow_mut().clear());
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    install(&mut kernel, flags).unwrap();
    kernel
}
pub(super) fn open(kernel: &mut crate::Kernel, params: Params) {
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
    assert!(
        matches!(result, Err(Error::Status(host_glue::ERR_UNSUPPORTED))),
        "{result:?}"
    );
    assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn every_post_factory_host_allocation_refuses_without_partial_publication() {
    // Also with the group, setting, extra and action of the batch (ARMS = 1).
    for arms in 0..=1 {
        sweep_post_factory_allocations(arms);
    }
}

fn sweep_post_factory_allocations(arms: u8) {
    let mut reached_success = false;
    for allocation in 1..400 {
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
        ARMS.with(|value| value.set(arms));
        let result = install(&mut kernel, 0);
        ARMS.with(|value| value.set(0));
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
            if arms == 1 {
                let shared = &kernel.shared;
                assert!(shared.actions.contains("factory.action"));
                assert!(shared.settings.spec("factory.flag").is_some());
                assert!(
                    shared
                        .settings
                        .groups()
                        .any(|g| g.id.as_str() == "factory.g")
                );
            }
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
        let shared = &kernel.shared;
        assert!(!shared.actions.contains("factory.action"), "{allocation}");
        assert!(
            shared.settings.spec("factory.flag").is_none(),
            "{allocation}"
        );
        assert!(
            !shared
                .settings
                .groups()
                .any(|g| g.id.as_str() == "factory.g"),
            "{allocation}"
        );
        assert_eq!(DESTROYED.with(Cell::get), 1);
    }
    assert!(
        reached_success,
        "the fault walk reached a fully admitted owner"
    );
}

#[test]
fn an_empty_install_is_refused_without_publication() {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let before = kernel.shared.registry.screens.list().iter().count();
    EMPTY.with(|value| value.set(true));
    let result = install(&mut kernel, 0);
    EMPTY.with(|value| value.set(false));
    assert!(matches!(result, Err(Error::Status(host_glue::ERR_INVALID))));
    assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
    assert!(kernel.shared.native_instances.find("factory").is_none());
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

fn regs_kernel(regs: u8) -> (crate::Kernel, Result<(), Error>) {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    REGS.with(|value| value.set(regs));
    let result = install(&mut kernel, 0);
    REGS.with(|value| value.set(0));
    (kernel, result)
}

#[test]
fn the_draws_of_status_items_and_lines_are_admitted() {
    for (regs, drawable) in [(10, 1), (11, 2)] {
        DRAWABLE.with(|value| value.set(drawable));
        let (mut kernel, result) = regs_kernel(regs);
        DRAWABLE.with(|value| value.set(0));
        result.unwrap();
        assert!(kernel.unload_plugin(&Id::new_static("factory")));
        assert_eq!(DESTROYED.with(Cell::get), 1, "mode {regs}");
    }
}

#[test]
fn a_draw_of_a_token_two_declarations_claim_or_none_refuses_the_install() {
    // 91 is the screen's and the status item's; 93 is nobody's here.
    for (regs, drawable) in [(12, 3), (10, 0)] {
        DRAWABLE.with(|value| value.set(drawable));
        let (kernel, result) = regs_kernel(regs);
        DRAWABLE.with(|value| value.set(0));
        assert!(
            matches!(result, Err(Error::Status(host_glue::ERR_INVALID))),
            "mode {regs}: {result:?}"
        );
        assert!(
            !kernel
                .shared
                .registry
                .screens
                .contains_key("factory.screen")
        );
        assert!(kernel.shared.native_instances.find("factory").is_none());
        assert_eq!(DESTROYED.with(Cell::get), 1, "mode {regs}");
    }
}

#[test]
fn event_and_draw_registrations_are_admitted_and_dropped_with_the_owner() {
    for regs in [1, 3, 6, 9] {
        let (mut kernel, result) = regs_kernel(regs);
        result.unwrap();
        let key = kernel.shared.native_instances.find("factory").unwrap();
        let events = kernel
            .shared
            .native_instances
            .subscribed(key, wire::EVENT_STARTED_TAG);
        assert_eq!(events, regs != 3, "mode {regs}");
        assert!(kernel.unload_plugin(&Id::new_static("factory")));
        assert!(
            !kernel
                .shared
                .native_instances
                .subscribed(key, wire::EVENT_STARTED_TAG),
            "an unloaded owner has no subscriptions"
        );
        assert_eq!(DESTROYED.with(Cell::get), 1, "mode {regs}");
    }
}

#[test]
fn registrations_that_do_not_fit_the_declarations_refuse_the_whole_install() {
    for (regs, status) in [
        // The same event twice; a draw nobody declared; the same draw twice.
        (2, host_glue::ERR_INVALID),
        (4, host_glue::ERR_INVALID),
        (5, host_glue::ERR_INVALID),
        // Across batches the same tag or token is still the same.
        (7, host_glue::ERR_INVALID),
        (8, host_glue::ERR_INVALID),
    ] {
        let (kernel, result) = regs_kernel(regs);
        assert!(
            matches!(result, Err(Error::Status(code)) if code == status),
            "mode {regs}: {result:?}"
        );
        assert!(
            !kernel
                .shared
                .registry
                .screens
                .contains_key("factory.screen")
        );
        assert!(kernel.shared.native_instances.find("factory").is_none());
        assert_eq!(DESTROYED.with(Cell::get), 1, "mode {regs}");
    }
}

#[test]
fn native_screen_below_guard_is_opened_and_closed_without_being_shown() {
    let mut kernel = kernel(1 << 1);
    let depth = device_depth(&kernel);
    for owner in ["base", "other"] {
        kernel
            .register_native(host_named(owner, 0), &depth, |_, _, _, _| {})
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
        UNSUPPORTED.with(|value| value.set(true));
        let result = kernel.register_native(hosted, &depth, |_, _, _, _| {});
        UNSUPPORTED.with(|value| value.set(false));
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
        native_declarations::NativeDeclarations, native_dispatch::NativeDispatch,
        native_screen::Dispatch, retained::Retained, shared::Shared,
    };
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let directory = Shared::try_new(NativeDeclarations::default()).unwrap();
    let mut flat_host = kernel.flat_host();
    flat_host.directory = directory.downgrade();
    let (retained, ()) = Retained::make_native(host(0), |_| Ok(())).unwrap();
    let dispatch = NativeDispatch::new(
        retained,
        FlatDepth::try_new().unwrap(),
        flat_host,
        |_, _, _, _| {},
        crate::native_host::native_instances::test_identity("factory"),
    );
    drop(directory);
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    assert_eq!(dispatch.event(91, &[0], &env), Err(host_glue::ERR_STATE));
    assert_eq!(DESTROYED.with(Cell::get), 0);
    drop(dispatch);
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

fn arms_kernel(arms: u8) -> (crate::Kernel, Result<(), Error>) {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    ARMS.with(|value| value.set(arms));
    let result = install(&mut kernel, 0);
    ARMS.with(|value| value.set(0));
    (kernel, result)
}

#[test]
fn settings_groups_and_actions_install_with_the_screens() {
    let (kernel, result) = arms_kernel(1);
    result.unwrap();
    let shared = &kernel.shared;
    assert!(shared.registry.screens.contains_key("factory.screen"));
    assert!(shared.actions.contains("factory.action"));
    assert!(shared.actions.get("factory.action").unwrap().runs_itself());
    let spec = shared.settings.spec("factory.flag").unwrap();
    assert_eq!(spec.title, "factory.flag");
    assert!(
        shared
            .settings
            .groups()
            .any(|g| g.id.as_str() == "factory.g")
    );
}

#[test]
fn a_batch_with_a_family_the_host_does_not_map_is_refused_whole() {
    let (kernel, result) = arms_kernel(2);
    assert!(
        matches!(result, Err(Error::Status(host_glue::ERR_UNSUPPORTED))),
        "{result:?}"
    );
    let shared = &kernel.shared;
    assert!(!shared.registry.screens.contains_key("factory.screen"));
    assert!(!shared.actions.contains("factory.action"));
    assert!(shared.settings.spec("factory.flag").is_none());
    assert!(
        !shared
            .settings
            .groups()
            .any(|g| g.id.as_str() == "factory.g")
    );
    assert!(shared.native_instances.find("factory").is_none());
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn a_duplicate_setting_or_action_is_refused_before_anything_is_published() {
    // Another plugin already holds the setting and the action ids; the
    // screen of this batch would be published before them otherwise.
    for held in ["factory.flag", "factory.action", "factory.g"] {
        reset(NORMAL);
        DESTROYED.with(|value| value.set(0));
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        match held {
            "factory.flag" => kernel
                .shared
                .settings
                .define(paperpaw_sdk::SettingSpec::new(
                    Id::new_static("factory.flag"),
                    "other",
                    paperpaw_sdk::SettingKind::Bool,
                    paperpaw_sdk::SettingValue::Bool(false),
                    paperpaw_sdk::builtin::groups::OTHER,
                ))
                .unwrap(),
            "factory.g" => kernel
                .shared
                .settings
                .add_group(paperpaw_sdk::SettingGroupSpec::new(
                    Id::new_static("factory.g"),
                    "other",
                ))
                .unwrap(),
            _ => {
                kernel.shared.actions.insert(paperpaw_sdk::ActionSpec::new(
                    Id::new_static("factory.action"),
                    "other",
                ));
            }
        }
        ARMS.with(|value| value.set(1));
        let result = install(&mut kernel, 0);
        ARMS.with(|value| value.set(0));
        assert!(result.is_err(), "{held}");
        let shared = &kernel.shared;
        assert!(
            !shared.registry.screens.contains_key("factory.screen"),
            "{held}"
        );
        assert!(shared.native_instances.find("factory").is_none(), "{held}");
        assert_eq!(
            shared.settings.spec("factory.flag").is_some(),
            held == "factory.flag",
            "{held}"
        );
        assert_eq!(
            shared.actions.contains("factory.action"),
            held == "factory.action"
        );
        assert_eq!(DESTROYED.with(Cell::get), 1, "{held}");
    }
}

#[test]
fn unload_removes_the_actions_settings_and_groups() {
    let (mut kernel, result) = arms_kernel(1);
    result.unwrap();
    let owner = Id::new_static("factory");
    assert!(kernel.unload_plugin(&owner));
    let shared = &kernel.shared;
    assert!(!shared.actions.contains("factory.action"));
    assert!(shared.settings.spec("factory.flag").is_none());
    assert!(
        !shared
            .settings
            .groups()
            .any(|g| g.id.as_str() == "factory.g")
    );
    assert!(!shared.registry.screens.contains_key("factory.screen"));
    // The same plugin installs again under a fresh key.
    ARMS.with(|value| value.set(1));
    install(&mut kernel, 0).unwrap();
    ARMS.with(|value| value.set(0));
    assert!(kernel.shared.settings.spec("factory.flag").is_some());
}

#[test]
fn every_allocation_of_an_install_with_arms_and_registrations_refuses_whole() {
    let mut reached_success = false;
    for allocation in 1..96 {
        reset(NORMAL);
        DESTROYED.with(|value| value.set(0));
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        ARMS.with(|value| value.set(1));
        REGS.with(|value| value.set(6));
        INSTALL_FAIL_NTH.with(|value| value.set(allocation));
        let result = install(&mut kernel, 0);
        arm_failure(0);
        INSTALL_FAIL_NTH.with(|value| value.set(0));
        ARMS.with(|value| value.set(0));
        REGS.with(|value| value.set(0));
        let shared = &kernel.shared;
        if result.is_ok() {
            reached_success = true;
            assert!(shared.actions.contains("factory.action"));
            assert!(shared.settings.spec("factory.flag").is_some());
            assert!(shared.registry.screens.contains_key("factory.screen"));
            break;
        }
        assert!(!shared.actions.contains("factory.action"), "{allocation}");
        assert!(
            shared.settings.spec("factory.flag").is_none(),
            "{allocation}"
        );
        assert!(
            !shared
                .settings
                .groups()
                .any(|g| g.id.as_str() == "factory.g"),
            "{allocation}"
        );
        assert!(!shared.registry.screens.contains_key("factory.screen"));
        assert_eq!(DESTROYED.with(Cell::get), 1, "{allocation}");
    }
    assert!(
        reached_success,
        "the fault walk reached a fully admitted owner"
    );
}

#[path = "native_open_handover_tests.rs"]
mod open_handover;
#[path = "native_registration_cap_tests.rs"]
mod registration_cap;
#[path = "native_resources_tests.rs"]
mod resources;
#[path = "native_slot_offers_tests.rs"]
mod slot_offers;

#[path = "native_menu_install_tests.rs"]
mod menu_records;

#[path = "native_events_install_tests.rs"]
mod events_registered;
#[path = "native_input_install_tests.rs"]
mod input_records;
#[path = "native_key_install_tests.rs"]
mod key_records;
#[path = "native_widget_install_tests.rs"]
mod widget_registered;

#[path = "native_reserved_id_tests.rs"]
mod reserved_id;
#[path = "native_stage_install_tests.rs"]
mod stage_records;
