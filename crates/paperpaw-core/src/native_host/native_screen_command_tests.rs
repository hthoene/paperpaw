//! The command stage of a native screen's callbacks through the real kernel
//! (`docs/abi/screens-input.md` 2.1; Senior's #1125 gates): what a screen's
//! input and event callbacks submit (open, close, back, notify) is applied by
//! the kernel's pump after the callback, and a refused or failed batch leaves
//! the screen stack as it was.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::super::*;
use super::{BOUND, STAGED, STAGED_ANSWER, STAGED_ON_SHOWN, key};
use crate::native_host::commands::{WireCommand, commands_encode};
use paperpaw_sdk::{Id, Params};

/// The notice slot's screen: a plain overlay (the kernel keeps a notice over the stack).
struct NoticeScreen;

impl paperpaw_sdk::Screen for NoticeScreen {
    fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

struct NoticePlugin;

impl paperpaw_sdk::Plugin for NoticePlugin {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("nt"),
            "Notice",
            paperpaw_sdk::Version::new(1, 0, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        r.add_screen(paperpaw_sdk::ScreenSpec::new(
            Id::new_static("nt.notice"),
            "notice",
            alloc::boxed::Box::new(|_, _| alloc::boxed::Box::new(NoticeScreen)),
        ))?;
        r.bind_slot(
            paperpaw_sdk::builtin::slots::NOTICE,
            Id::new_static("nt.notice"),
            paperpaw_sdk::Priority::DEFAULT,
        )?;
        Ok(())
    }
}

/// The screen ids on the stack, bottom to top.
fn stack(kernel: &crate::Kernel) -> Vec<&str> {
    kernel.stack.iter().map(|s| s.id.as_str()).collect()
}

/// A kernel with the notice slot and `factory.screen` (and `other.screen`),
/// whose screen submits `commands` when it hears an action, or when it is
/// shown (`on_shown`), and answers `answer` to the action.
fn staged(policy: u32, commands: &[WireCommand<'_>], on_shown: bool, answer: i32) -> crate::Kernel {
    super::super::reset(NORMAL);
    super::super::ARMS.with(|arms| arms.set(1));
    super::super::MENUS.with(|menus| menus.set(BOUND));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(NoticePlugin)
        .build()
        .unwrap();
    install(&mut kernel, policy << 6).unwrap();
    kernel
        .register_native(host_named("other", 0), &device_depth(&kernel), |_, _, _| {})
        .unwrap();
    super::super::MENUS.with(|menus| menus.set(0));
    super::super::ARMS.with(|arms| arms.set(0));
    let mut bytes = [0; 512];
    let len = commands_encode(commands, &mut bytes).unwrap();
    STAGED.with(|batch| *batch.borrow_mut() = Some(bytes[..len].to_vec()));
    STAGED_ON_SHOWN.with(|shown| shown.set(on_shown));
    STAGED_ANSWER.with(|value| value.set(answer));
    kernel
}

fn unstage() {
    STAGED.with(|batch| *batch.borrow_mut() = None);
    STAGED_ON_SHOWN.with(|shown| shown.set(false));
    STAGED_ANSWER.with(|value| value.set(1));
}

/// Shows `screen` on top and tells it so.
fn show(kernel: &mut crate::Kernel, screen: &'static str) {
    kernel.open_screen(
        paperpaw_platform::Instant(1),
        Id::new_static(screen),
        Params::new(),
    );
    let top = kernel.stack.len() - 1;
    kernel.with_screen(top, |screen, cx| {
        screen.on_show(cx);
        Handled::Yes
    });
}

const OPEN: WireCommand<'static> = WireCommand::Open {
    target: "factory.screen",
    mode: 0,
    slot: "",
    handover: 0,
};
const CLOSE_TOP: WireCommand<'static> = WireCommand::Close { instance: 0 };

#[test]
fn a_screen_that_submits_open_in_an_input_callback_gets_the_screen_opened() {
    // Policy 2: every open makes a new instance.
    let mut kernel = staged(2, &[OPEN], false, 1);
    show(&mut kernel, "factory.screen");
    assert_eq!(stack(&kernel), ["factory.screen"]);
    key(&mut kernel, 100, "confirm");
    assert_eq!(stack(&kernel), ["factory.screen", "factory.screen"]);
    unstage();
}

#[test]
fn a_screen_that_submits_close_in_an_input_callback_gets_itself_closed() {
    let mut kernel = staged(2, &[CLOSE_TOP], false, 1);
    show(&mut kernel, "factory.screen");
    unstage();
    show(&mut kernel, "factory.screen");
    STAGED.with(|batch| {
        let mut bytes = [0; 64];
        let len = commands_encode(&[CLOSE_TOP], &mut bytes).unwrap();
        *batch.borrow_mut() = Some(bytes[..len].to_vec());
    });
    assert_eq!(kernel.stack.len(), 2);
    key(&mut kernel, 100, "confirm");
    assert_eq!(stack(&kernel), ["factory.screen"]);
    unstage();
}

#[test]
fn a_screen_that_submits_back_in_an_input_callback_gets_the_screens_above_the_target_closed() {
    let back = WireCommand::BackTo {
        target: "other.screen",
    };
    let mut kernel = staged(0, &[back], false, 1);
    show(&mut kernel, "other.screen");
    show(&mut kernel, "factory.screen");
    assert_eq!(stack(&kernel), ["other.screen", "factory.screen"]);
    key(&mut kernel, 100, "confirm");
    assert_eq!(stack(&kernel), ["other.screen"]);
    unstage();
}

#[test]
fn a_screen_that_submits_notify_in_an_input_callback_gets_the_notice_shown() {
    let notify = WireCommand::Notify {
        text: "Saved",
        flags: 0,
    };
    let mut kernel = staged(0, &[notify], false, 1);
    show(&mut kernel, "factory.screen");
    assert_eq!(stack(&kernel), ["factory.screen"]);
    // Whatever notice there was (the kernel gives some of its own) is gone.
    kernel.end_notice();
    assert!(kernel.notice.is_none());
    key(&mut kernel, 100, "confirm");
    // The notice is the kernel's, over the stack, not on it.
    assert_eq!(stack(&kernel), ["factory.screen"]);
    assert!(kernel.notice.is_some());
    unstage();
}

#[test]
fn an_event_callback_gets_the_same_stage() {
    let notify = WireCommand::Notify {
        text: "Hello",
        flags: 0,
    };
    let mut kernel = staged(0, &[notify], true, 1);
    show(&mut kernel, "factory.screen");
    // Nothing is applied inside the callback; the pump does, between callbacks.
    assert_eq!(stack(&kernel), ["factory.screen"]);
    let _ = kernel.finish();
    // The notice is the kernel's, over the stack, not on it.
    assert_eq!(stack(&kernel), ["factory.screen"]);
    assert!(kernel.notice.is_some());
    unstage();
}

#[test]
fn a_batch_with_a_refused_command_is_refused_whole_and_the_stack_is_unchanged() {
    // The second command names an instance that is not on the stack: the
    // stage refuses the batch at submit, the first command with it.
    let refused = [CLOSE_TOP, WireCommand::Close { instance: 4000 }];
    let mut kernel = staged(2, &refused, false, 1);
    show(&mut kernel, "factory.screen");
    unstage();
    show(&mut kernel, "factory.screen");
    let mut bytes = [0; 128];
    let len = commands_encode(&refused, &mut bytes).unwrap();
    STAGED.with(|batch| *batch.borrow_mut() = Some(bytes[..len].to_vec()));
    let before = stack(&kernel).len();
    key(&mut kernel, 100, "confirm");
    assert_eq!(kernel.stack.len(), before);
    unstage();
}

#[test]
fn a_callback_that_fails_after_submitting_applies_nothing() {
    // The batch was admitted, then the callback returned an error: the whole
    // batch is dropped with the stage.
    let mut kernel = staged(2, &[OPEN], false, -7);
    show(&mut kernel, "factory.screen");
    key(&mut kernel, 100, "confirm");
    assert_eq!(stack(&kernel), ["factory.screen"]);
    unstage();
}

#[test]
fn an_open_with_a_hand_over_is_refused_where_nothing_stages_hand_overs() {
    let with_handover = WireCommand::Open {
        target: "factory.screen",
        mode: 0,
        slot: "",
        handover: 1,
    };
    let mut kernel = staged(2, &[with_handover], false, 1);
    show(&mut kernel, "factory.screen");
    key(&mut kernel, 100, "confirm");
    assert_eq!(stack(&kernel), ["factory.screen"]);
    unstage();
}

#[test]
fn a_notify_with_an_unknown_flag_is_refused_and_shows_nothing() {
    let notify = WireCommand::Notify {
        text: "Saved",
        flags: 2,
    };
    let mut kernel = staged(0, &[notify], false, 1);
    show(&mut kernel, "factory.screen");
    kernel.end_notice();
    key(&mut kernel, 100, "confirm");
    assert!(kernel.notice.is_none());
    assert_eq!(stack(&kernel), ["factory.screen"]);
    unstage();
}
