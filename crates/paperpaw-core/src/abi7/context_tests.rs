extern crate std;

use super::*;
use alloc::{borrow::Cow, string::String};
use core::{ops::ControlFlow, pin::pin};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, Version};

fn frame<'a>(kernel: &'a crate::Kernel, env: &'a Env<'a>) -> CoreFrame<'a> {
    CoreFrame {
        env,
        meter: &kernel.shared.meter,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
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
