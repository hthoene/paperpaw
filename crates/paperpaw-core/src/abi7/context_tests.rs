extern crate std;

use super::*;
use crate::abi7::{
    context::{Error, FrameSlot},
    core_frame::CallbackStamp,
};
use alloc::{borrow::Cow, string::String};
use core::{ops::ControlFlow, pin::pin};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, Version};

fn frame<'a>(kernel: &'a crate::Kernel, env: &'a Env<'a>) -> CoreFrame<'a> {
    CoreFrame {
        callback: None,
        instance: None,
        env,
        meter: &kernel.shared.meter,
        depth: None,
        directory: None,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
    }
}

#[test]
fn core_frames_expose_usage_and_rows_only_during_the_callback() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let frame = frame(&kernel, &env);
    let slot = pin!(FrameSlot::new());
    assert_eq!(slot.with(|_| ()).err(), Some(Error::NoFrame));
    slot.as_ref()
        .scope(&frame, || {
            slot.with(|frame| {
                assert_eq!(frame.device_usage().unwrap(), env.usage().into());
                let mut ids = alloc::vec::Vec::new();
                let end = frame
                    .plugin_rows(0, &mut |row| {
                        ids.push(String::from(row.plugin));
                        ControlFlow::Continue(())
                    })
                    .unwrap();
                assert_eq!(ids, ["core"]);
                assert_eq!((end.total, end.next), (1, 1));
                for from in [1, 2, u32::MAX] {
                    let end = frame
                        .plugin_rows(from, &mut |_| panic!("past end"))
                        .unwrap();
                    assert_eq!((end.total, end.next), (1, 1));
                }
                assert_eq!(
                    frame.plugin_name("core", &mut |name| assert_eq!(name, "PaperPaw")),
                    Ok(())
                );
                assert_eq!(
                    frame.plugin_name("missing", &mut |_| panic!("unknown")),
                    Err(ERR_NOT_FOUND)
                );
                assert_eq!(
                    frame.work_note("missing", &mut |_, _| panic!("unknown")),
                    Err(ERR_NOT_FOUND)
                );
            })
            .unwrap();
            assert_eq!(slot.as_ref().scope(&frame, || ()).err(), Some(Error::Busy));
        })
        .unwrap();
    assert_eq!(slot.with(|_| ()).err(), Some(Error::NoFrame));
    assert!(kernel.shared.meter.table.try_borrow_mut().is_ok());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        slot.as_ref()
            .scope(&frame, || panic!("fixture unwind"))
            .unwrap();
    }));
    assert!(result.is_err());
    assert_eq!(slot.with(|_| ()).err(), Some(Error::NoFrame));
}

#[test]
fn env_only_frames_refuse_core_services() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    assert_eq!(Frame::device_usage(&env), Err(ERR_STATE));
    assert_eq!(
        Frame::plugin_rows(&env, 0, &mut |_| ControlFlow::Continue(())),
        Err(ERR_STATE)
    );
    assert_eq!(
        Frame::plugin_name(&env, "core", &mut |_| panic!("no frame")),
        Err(ERR_STATE)
    );
    assert_eq!(
        Frame::work_note(&env, "core", &mut |_, _| panic!("no frame")),
        Err(ERR_STATE)
    );
}

struct Named;
impl Plugin for Named {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("named"),
            "Manifest name",
            Version::new(1, 0, 0),
        )
    }
    fn register(&self, _: &mut Registrar) -> Result<(), RegisterError> {
        Ok(())
    }
}

#[test]
fn names_follow_ui_language_and_notes_distinguish_empty_from_absent() {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Named)
        .build()
        .unwrap();
    kernel.shared.translator.add(paperpaw_i18n::Catalog::parse(Cow::Borrowed(
        "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\nnamed.plugin.name = Name auf Deutsch\n"
    )).unwrap());
    kernel.shared.translator.set_language("de");
    {
        let env = kernel.shared.env(Instant(0));
        let frame = frame(&kernel, &env);
        frame
            .plugin_name("named", &mut |name| assert_eq!(name, "Name auf Deutsch"))
            .unwrap();
        assert_eq!(
            frame.work_note("named", &mut |_, _| panic!("no note")),
            Err(ERR_NOT_FOUND)
        );
    }
    kernel.shared.translator.set_language("en");
    kernel.shared.set_work_note(
        Id::new_static("named"),
        paperpaw_sdk::work::Note::new(Instant(17), ""),
    );
    let env = kernel.shared.env(Instant(0));
    let frame = frame(&kernel, &env);
    frame
        .plugin_name("named", &mut |name| assert_eq!(name, "named"))
        .unwrap();
    let mut called = false;
    frame
        .work_note("named", &mut |at, text| {
            called = true;
            assert_eq!((at, text), (17, ""));
        })
        .unwrap();
    assert!(called);
}

#[test]
fn pages_keep_source_offsets_and_release_rows_before_plugin_code() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    drop(kernel.shared.meter.enter("first"));
    drop(kernel.shared.meter.enter("second"));
    kernel.shared.storage.sync().unwrap();
    let env = kernel.shared.env(Instant(0));
    let frame = frame(&kernel, &env);
    let mut count = 0;
    let end = frame
        .plugin_rows(0, &mut |row| {
            if count == 1 {
                return ControlFlow::Break(());
            }
            assert_eq!((row.plugin, row.syncs), ("core", 1));
            count += 1;
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!((end.total, end.next), (3, 1));
    let next = frame
        .plugin_rows(end.next, &mut |row| {
            assert_ne!(row.plugin, "core");
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!((next.next, next.generation), (3, end.generation));
    let _table = kernel.shared.meter.table.borrow_mut();
    assert_eq!(
        frame.plugin_rows(0, &mut |_| panic!("borrow conflict")),
        Err(ERR_STATE)
    );
}

#[test]
fn localized_name_caps_preserve_utf8_and_exact_boundary_text() {
    for (text, expected) in [
        ("a".repeat(128), 128),
        (alloc::format!("{}é", "a".repeat(127)), 127),
        (alloc::format!("{}🦊", "a".repeat(126)), 126),
    ] {
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .plugin(Named)
            .build()
            .unwrap();
        let catalog = alloc::format!(
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n[messages]\nnamed.plugin.name = {text}\n"
        );
        kernel
            .shared
            .translator
            .add(paperpaw_i18n::Catalog::parse(catalog).unwrap());
        let env = kernel.shared.env(Instant(0));
        let frame = frame(&kernel, &env);
        frame
            .plugin_name("named", &mut |name| assert_eq!(name, &text[..expected]))
            .unwrap();
    }
}

#[test]
fn caller_and_epoch_require_an_actual_nonzero_callback_stamp() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let mut frame = frame(&kernel, &env);
    assert_eq!(frame.caller(), Err(paperpaw_native::host_glue::ERR_STATE));
    assert_eq!(
        frame.callback_epoch(),
        Err(paperpaw_native::host_glue::ERR_STATE)
    );
    frame.callback = Some(CallbackStamp { owner: 0, epoch: 0 });
    assert_eq!(frame.caller(), Err(paperpaw_native::host_glue::ERR_STATE));
    assert_eq!(
        frame.callback_epoch(),
        Err(paperpaw_native::host_glue::ERR_STATE)
    );
    frame.callback = Some(CallbackStamp { owner: 0, epoch: 9 });
    assert_eq!(frame.caller(), Ok(0));
    assert_eq!(frame.callback_epoch(), Ok(9));
}

#[test]
fn calling_instance_requires_a_live_host_stamped_screen_and_callback() {
    use paperpaw_native::host_glue::ERR_STATE;
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.shared.stack.push(paperpaw_sdk::StackEntry::new(
        Id::new_static("probe.screen"),
        None,
        false,
        paperpaw_sdk::ScreenKey::new(7),
    ));
    kernel.shared.stack.push(paperpaw_sdk::StackEntry::new(
        Id::new_static("probe.overlay"),
        None,
        false,
        paperpaw_sdk::ScreenKey::new(8),
    ));
    kernel.shared.stack_rows.update([7, 8].map(|instance| {
        crate::native_host::stack_rows::StackRow {
            instance,
            owner: 5,
            flags: 0,
        }
    }));
    let env = kernel.shared.env(Instant(0));
    let mut frame = frame(&kernel, &env);
    assert_eq!(frame.calling_instance(), Err(ERR_STATE));
    for (owner, epoch, instance, expected) in [
        (5, 0, Some(7), Err(ERR_STATE)),
        (5, 1, None, Err(ERR_STATE)),
        (5, 1, Some(0), Err(ERR_STATE)),
        (5, 1, Some(6), Err(ERR_STATE)),
        (5, 1, Some(7), Ok(7)),
        (5, 1, Some(8), Ok(8)),
        (6, 1, Some(7), Err(ERR_STATE)),
        (0, 1, Some(7), Err(ERR_STATE)),
    ] {
        frame.callback = Some(CallbackStamp { owner, epoch });
        frame.instance = instance;
        assert_eq!(frame.calling_instance(), expected);
    }
}

#[test]
fn shared_callback_stamp_constructor_refuses_epoch_zero() {
    assert_eq!(
        CallbackStamp::new(7, 0),
        Err(paperpaw_native::host_glue::ERR_STATE)
    );
    assert_eq!(
        CallbackStamp::new(7, 9),
        Ok(CallbackStamp { owner: 7, epoch: 9 })
    );
}

/// The production `CoreFrame` is what `core.ui` ops read the stack through
/// (todo 116): a stale callback stamp or a stale stack generation is
/// `ERR_STATE`, the stack view is the snapshot, and the dialog close queue
/// holds one slot per live instance.
#[test]
fn core_ui_reads_the_stack_through_a_live_frame_only() {
    use crate::native_host::stack_rows::StackRow;
    use paperpaw_native::host_glue::ERR_STATE;
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let rows = |instances: &[u32]| {
        kernel
            .shared
            .stack_rows
            .update(instances.iter().map(|&instance| StackRow {
                instance,
                owner: 5,
                flags: 0,
            }));
    };
    rows(&[7, 8]);
    let env = kernel.shared.env(Instant(0));
    let mut frame = frame(&kernel, &env);
    let view = |frame: &CoreFrame<'_>| {
        let mut seen = alloc::vec::Vec::new();
        frame
            .with_stack(&mut |stack| {
                seen.extend(
                    (0..stack.len())
                        .filter_map(|at| stack.row(at))
                        .map(|row| row.instance),
                );
            })
            .map(|()| seen)
    };
    // No callback (or a zero epoch): nothing is readable or queued.
    assert_eq!(view(&frame), Err(ERR_STATE));
    assert_eq!(frame.close_dialog(7), Err(ERR_STATE));
    frame.callback = Some(CallbackStamp { owner: 5, epoch: 0 });
    assert_eq!(view(&frame), Err(ERR_STATE));
    // A live callback sees the stack, bottom to top.
    frame.callback = Some(CallbackStamp { owner: 5, epoch: 1 });
    assert_eq!(view(&frame), Ok(alloc::vec![7, 8]));
    // One slot per live instance: a repeat is the same slot; an instance
    // that is not on the stack is `ERR_STATE`.
    assert_eq!(frame.close_dialog(8), Ok(()));
    assert_eq!(frame.close_dialog(8), Ok(()));
    assert_eq!(frame.close_dialog(99), Err(ERR_STATE));
    assert_eq!(frame.close_dialog(0), Err(ERR_STATE));
    let queued = kernel
        .shared
        .stack_rows
        .take_pending()
        .iter()
        .filter(|pending| pending.instance != 0)
        .count();
    assert_eq!(queued, 1);
    // The stack changed since this frame was made: its generation is stale.
    rows(&[7]);
    assert_eq!(view(&frame), Err(ERR_STATE));
    assert_eq!(frame.close_dialog(7), Err(ERR_STATE));
    // A frame of the new generation works again; one queue slot is left for
    // every live instance of a full stack and no more.
    let full: alloc::vec::Vec<u32> = (1..=16).collect();
    rows(&full);
    frame.stack_generation = kernel.shared.stack_rows.generation();
    for instance in 1..=16 {
        assert_eq!(frame.close_dialog(instance), Ok(()));
    }
    assert_eq!(frame.close_dialog(17), Err(ERR_STATE));
}

/// The S3 frame scan's budgets (`shells/xteink/frame-budget-s3*.txt`) name the
/// `Frame` trait by its path in an `absent trait` line (todo 252): the day the
/// trait is renamed or moved, this fails, so the line cannot keep passing by
/// matching nothing.
#[test]
fn the_frame_trait_path_is_the_one_the_s3_budgets_name() {
    let path = core::any::type_name::<dyn crate::abi7::context::Frame>();
    assert_eq!(path, "dyn paperpaw_core::abi7::context::Frame");
    let name = path.strip_prefix("dyn ").unwrap_or(path);
    for budget in [
        include_str!("../../../../shells/xteink/frame-budget-s3.txt"),
        include_str!("../../../../shells/xteink/frame-budget-s3-classic.txt"),
    ] {
        let line = alloc::format!("absent trait {name} ");
        let root = alloc::format!("root trait {name}\n");
        assert!(
            budget
                .lines()
                .any(|l| l.starts_with(&line) || l == root.trim_end()),
            "a budget has neither `absent trait {name}` nor `root trait {name}`"
        );
    }
}
