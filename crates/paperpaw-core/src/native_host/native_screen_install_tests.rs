//! Real ABI screen declarations are published through the kernel registry.
use super::*;
use crate::native_host::{flat::FlatDepth, registrations as wire};
use paperpaw_sdk::{Command, Handled, Id, Params};

std::thread_local! {
    static UNSUPPORTED: Cell<bool> = const { Cell::new(false) };
    static FLAGS: Cell<u32> = const { Cell::new(0) };
    static DESTROYED: Cell<u32> = const { Cell::new(0) };
    static REPORTS: Cell<u32> = const { Cell::new(0) };
    static EVENTS: RefCell<Vec<(u32, u32, u32, u32)>> = const { RefCell::new(Vec::new()) };
}

struct ScreenState {
    table: PpPlugin,
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
    let status = unsafe { ((*host).registrations_submit)(ctx, borrowed(&bytes[..len])) };
    if status != OK {
        return status;
    }
    let state = Box::into_raw(Box::new(ScreenState {
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
    OK
}
unsafe extern "C" fn screen_destroy(state: *mut c_void) {
    DESTROYED.with(|value| value.set(value.get() + 1));
    // SAFETY: the host returns exactly the allocation published by screen_make.
    unsafe { drop(Box::from_raw(state.cast::<ScreenState>())) };
}
unsafe extern "C" fn screen_event(_: *mut c_void, token: u32, bytes: PpBytes, _: PpBuffer) -> i32 {
    if token != 91 {
        return ERR_INVALID;
    }
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
    EVENTS.with(|events| events.borrow_mut().push(row));
    OK
}
fn host(flags: u32) -> Hosted<Heap, ()> {
    FLAGS.with(|value| value.set(flags));
    // SAFETY: the linked factory and code remain available throughout the owner lifetime.
    unsafe {
        Hosted::new(
            Heap,
            (),
            screen_make,
            "factory",
            (ptr::null(), 0),
            |_, _, _, _| {},
            super::super::super::HostResources::try_new().unwrap(),
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
        Err(Error::Factory(ERR_INVALID))
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
