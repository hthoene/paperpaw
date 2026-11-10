//! Exercise status normalization through actual ABI callbacks, not Rust errors.
use super::*;
use core::cell::Cell;
use paperpaw_sdk::{Id, ScreenKey, StackEntry};

std::thread_local! {
    static CHAIN_TEST: Cell<bool> = const { Cell::new(false) };
    static RETURN_STATUS: Cell<i32> = const { Cell::new(OK) };
    static SUBMIT_STATUS: Cell<i32> = const { Cell::new(ERR_INVALID) };
    /// A picture request to submit instead of the usual commands: 1 for the
    /// plugin's folder (root 0), 2 for the card (root 1).
    static IMAGE_ROOT: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "C" fn make_commands(
    host: *const PpHost,
    ctx: *mut c_void,
    out: *mut *const PpPlugin,
) -> i32 {
    // SAFETY: forwarded factory arguments satisfy the original fixture's contract.
    let status = unsafe { make(host, ctx, out) };
    if status == OK {
        // SAFETY: make just initialized this uniquely owned fixture table. It is
        // not published to the host until this factory returns.
        unsafe {
            let plugin = (*out).cast_mut();
            (*plugin).input = Some(input_commands);
            (*plugin).dispatch_event = Some(event_commands);
            (*plugin).draw = Some(draw_commands);
        }
    }
    status
}

fn submit(state: *mut c_void) {
    let mut bytes = [0; 128];
    let commands = [
        WireCommand::SetSetting {
            key: "fake.toggle",
            value: crate::abi7::settings_wire::SettingValueRef::Bool(true),
        },
        WireCommand::Redraw { mode: 0 },
        WireCommand::RunAction {
            action: "fake.absent",
        },
    ];
    let count = if CHAIN_TEST.with(Cell::get) { 3 } else { 2 };
    let image = [WireCommand::ImagePrepare {
        root: IMAGE_ROOT.with(Cell::get).saturating_sub(1),
        path: "p.png",
        side: 0,
        cookie: 1,
        flags: 0,
        tone: &[],
    }];
    let len = if IMAGE_ROOT.with(Cell::get) == 0 {
        commands_encode(&commands[..count], &mut bytes)
    } else {
        commands_encode(&image, &mut bytes)
    }
    .unwrap();
    // SAFETY: owner admission keeps the fixture and table alive. Encoded bytes
    // are readable until the synchronous submit returns and do not alias host state.
    let status = unsafe {
        let fake = &*state.cast::<Fake>();
        ((*fake.host).commands_submit)(
            fake.ctx,
            PpBytes {
                ptr: bytes.as_ptr(),
                len,
            },
        )
    };
    SUBMIT_STATUS.with(|value| value.set(status));
}

unsafe extern "C" fn input_commands(state: *mut c_void, _: u32, _: PpBytes) -> i32 {
    submit(state);
    RETURN_STATUS.with(Cell::get)
}
unsafe extern "C" fn event_commands(state: *mut c_void, _: u32, _: PpBytes, _: PpBuffer) -> i32 {
    submit(state);
    RETURN_STATUS.with(Cell::get)
}
unsafe extern "C" fn draw_commands(state: *mut c_void, _: u32, _: PpBytes) -> i32 {
    submit(state);
    OK
}

fn hosted_commands() -> Hosted<Heap, Module> {
    // SAFETY: this fixture's factory is callable for the retained module lifetime.
    unsafe {
        Hosted::new(
            Heap,
            Module::new(),
            make_commands,
            "fake",
            (ptr::null(), 0),
            report,
            super::super::super::super::HostResources::try_new().unwrap(),
            DeclaredServices::none(),
        )
    }
    .unwrap()
}

fn kernel() -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel
        .shared
        .settings
        .define(paperpaw_sdk::SettingSpec::switch(
            "fake.toggle",
            "toggle",
            false,
            paperpaw_sdk::builtin::groups::OTHER,
        ))
        .unwrap();
    kernel.shared.stack.push(StackEntry::new(
        Id::new_static("fake.screen"),
        None,
        false,
        ScreenKey::new(7),
    ));
    kernel
}

fn check_status(input: bool, status: i32, expected: Result<i32, i32>) {
    reset(NORMAL);
    RETURN_STATUS.with(|value| value.set(status));
    let mut kernel = kernel();
    let hosted = hosted_commands();
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    let result = kernel.with_native_commands(
        &batch,
        &DispatchDepth::new(),
        super::super::super::command_dispatch::CommandTarget {
            screen: Some(ScreenKey::new(7)),
            hop: 0,
            result: if input {
                super::super::super::command_dispatch::CommandResult::Input
            } else {
                super::super::super::command_dispatch::CommandResult::Status
            },
        },
        paperpaw_platform::Instant(0),
        |context, frame| {
            super::super::super::tokens::invoke_token(
                context,
                TOKEN,
                frame,
                if input {
                    TokenInvocation::Input(&[])
                } else {
                    TokenInvocation::Event(&[])
                },
            )
        },
    );
    assert_eq!(
        SUBMIT_STATUS.with(Cell::get),
        OK,
        "the ABI callback staged commands"
    );
    match expected {
        Ok(status) => assert_eq!(result, Ok(status)),
        Err(status) => assert!(matches!(result, Err(Error::Status(code)) if code == status)),
    }
    assert_eq!(kernel.shared.settings.bool("fake.toggle"), expected.is_ok());
    assert_eq!(kernel.output.redraw, expected.is_ok());
    let context = hosted.owner().context();
    assert_eq!(
        context.environment.with_frame(|_| ()),
        Err(host_glue::ERR_STATE)
    );
    // SAFETY: a zero-length buffer is valid and the scope has already ended.
    assert_eq!(
        unsafe { context.commands.submit(ptr::null(), 0) },
        host_glue::ERR_STATE
    );
    drop(batch);
    finish(&hosted);
}

#[test]
fn failed_abi_callbacks_discard_staged_commands_and_redraw() {
    for input in [false, true] {
        check_status(input, host_glue::ERR_DENIED, Err(host_glue::ERR_DENIED));
        check_status(input, 2, Err(ERR_INVALID));
    }
}

#[test]
fn valid_input_zero_and_one_keep_accepted_commands() {
    for status in [0, 1] {
        check_status(true, status, Ok(status));
    }
    check_status(false, OK, Ok(OK));
}

#[test]
fn draw_has_no_command_authority_and_cannot_stage_effects() {
    reset(NORMAL);
    let kernel = kernel();
    let hosted = hosted_commands();
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    let mut pixels =
        paperpaw_graphics::Framebuffer::new(8, 8, paperpaw_platform::PixelFormat::Gray8).unwrap();
    assert_eq!(
        kernel.draw_native_for_test(
            &batch,
            1,
            &DispatchDepth::new(),
            paperpaw_platform::Instant(0),
            &mut pixels.canvas(),
        ),
        Ok(OK)
    );
    assert_eq!(SUBMIT_STATUS.with(Cell::get), host_glue::ERR_STATE);
    assert!(!kernel.shared.settings.bool("fake.toggle"));
    assert!(!kernel.output.redraw);
    drop(batch);
    finish(&hosted);
}

#[test]
fn input_forwards_incoming_chain_hop_and_refuses_hop_eight_atomically() {
    for (hop, expected) in [(7, host_glue::ERR_NOT_FOUND), (8, host_glue::ERR_BUSY)] {
        reset(NORMAL);
        CHAIN_TEST.with(|value| value.set(true));
        RETURN_STATUS.with(|value| value.set(OK));
        let mut kernel = kernel();
        let hosted = hosted_commands();
        let batch = hosted
            .make_registered(hosted.begin().unwrap())
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(
            kernel.input_native_for_test(
                &batch,
                TOKEN,
                &DispatchDepth::new(),
                &[],
                paperpaw_platform::Instant(0),
                hop,
            ),
            Ok(OK)
        );
        CHAIN_TEST.with(|value| value.set(false));
        assert_eq!(SUBMIT_STATUS.with(Cell::get), expected);
        assert!(!kernel.shared.settings.bool("fake.toggle"));
        assert!(!kernel.output.redraw);
        drop(batch);
        finish(&hosted);
    }
}

#[test]
fn picture_commands_are_admitted_by_the_plugins_own_features() {
    // Root 0 (its folder) is open to every plugin; root 1 (the card) needs
    // `uses.storage`, which this one did not ask for.
    for (root, expected) in [(1, OK), (2, host_glue::ERR_DENIED)] {
        reset(NORMAL);
        IMAGE_ROOT.with(|value| value.set(root));
        RETURN_STATUS.with(|value| value.set(OK));
        let mut kernel = kernel();
        let hosted = hosted_commands();
        let batch = hosted
            .make_registered(hosted.begin().unwrap())
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(
            kernel.input_native_for_test(
                &batch,
                TOKEN,
                &DispatchDepth::new(),
                &[],
                paperpaw_platform::Instant(0),
                0,
            ),
            Ok(OK)
        );
        IMAGE_ROOT.with(|value| value.set(0));
        assert_eq!(SUBMIT_STATUS.with(Cell::get), expected);
        drop(batch);
        finish(&hosted);
    }
}
