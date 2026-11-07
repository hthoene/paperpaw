//! Deferred application uses the admitted caller and ends every callback borrow.
use super::*;
use crate::native_host::commands::{WireCommand, commands_encode};

#[test]
fn command_stage_applies_only_after_callback_scopes_and_discards_failed_calls() {
    for returned in [None, Some(0), Some(1), Some(2), Some(host_glue::ERR_DENIED)] {
        let success = returned == Some(0);
        reset(NORMAL);
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
        let hosted = fake();
        let batch = hosted
            .make_registered(hosted.begin().unwrap())
            .unwrap()
            .commit()
            .unwrap();
        let mut bytes = [0; 128];
        let len = commands_encode(
            &[WireCommand::SetSetting {
                key: "fake.toggle",
                value: crate::abi7::settings_wire::SettingValueRef::Bool(true),
            }],
            &mut bytes,
        )
        .unwrap();
        let result = kernel.with_native_commands(
            &batch,
            &DispatchDepth::new(),
            super::super::command_dispatch::CommandTarget {
                screen: None,
                hop: 0,
                result: super::super::command_dispatch::CommandResult::Status,
            },
            paperpaw_platform::Instant(0),
            |context, frame| {
                // SAFETY: the retained context structurally pins its environment.
                let scope = unsafe { context.map_unchecked(|value| &value.environment) };
                scope.scope_core(frame, || {
                    // SAFETY: the encoded command is readable and disjoint from host state.
                    assert_eq!(unsafe { context.commands.submit(bytes.as_ptr(), len) }, OK);
                    assert!(
                        !frame.env.settings().bool("fake.toggle"),
                        "application is deferred"
                    );
                    returned.ok_or(Error::Status(ERR_INVALID))
                })?
            },
        );
        if !success {
            assert!(matches!(result, Err(Error::Status(_))));
        } else {
            assert_eq!(result, Ok(OK));
        }
        assert_eq!(kernel.shared.settings.bool("fake.toggle"), success);
        let context = hosted.owner().context();
        assert_eq!(
            context.environment.with_frame(|_| ()),
            Err(host_glue::ERR_STATE)
        );
        // SAFETY: the bytes remain live, but the callback's command scope has ended.
        assert_eq!(
            unsafe { context.commands.submit(bytes.as_ptr(), len) },
            host_glue::ERR_STATE
        );
        drop(batch);
        finish(&hosted);
    }
}

#[path = "shell_command_status_tests.rs"]
mod abi_status;

#[test]
fn image_commands_use_owner_resources_and_wait_for_successful_callback() {
    for returned in [0, host_glue::ERR_DENIED] {
        reset(NORMAL);
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        let hosted = fake();
        let batch = hosted
            .make_registered(hosted.begin().unwrap())
            .unwrap()
            .commit()
            .unwrap();
        let mut bytes = [0; 128];
        let len = commands_encode(
            &[WireCommand::ImagePrepare {
                root: 0,
                path: "missing.png",
                side: 0,
                cookie: 77,
                flags: 0,
            }],
            &mut bytes,
        )
        .unwrap();
        let context = hosted.owner().context();
        let images = context.resources.hosts().images();
        let result = kernel.with_native_commands(
            &batch,
            &DispatchDepth::new(),
            super::super::command_dispatch::CommandTarget {
                screen: None,
                hop: 0,
                result: super::super::command_dispatch::CommandResult::Status,
            },
            paperpaw_platform::Instant(0),
            |context, _| {
                assert!(!images.prepare_pending());
                // SAFETY: encoded bytes are live and disjoint from the host state.
                assert_eq!(unsafe { context.commands.submit(bytes.as_ptr(), len) }, OK);
                assert!(!images.prepare_pending(), "no side effect inside callback");
                Ok(returned)
            },
        );
        assert_eq!(result.is_ok(), returned == 0);
        assert_eq!(images.prepare_pending(), returned == 0);
        if returned == 0 {
            images
                .prepare_step(
                    kernel.storage(),
                    crate::native_host::image_service::prepare::Env::x4(480, false),
                )
                .unwrap();
            images
                .take_ready(&mut |ready| {
                    assert_eq!(ready.owner, context.resources.owner_id());
                    assert_eq!(ready.cookie, 77);
                    Ok(())
                })
                .unwrap();
        }
        drop(batch);
        finish(&hosted);
    }
}

#[test]
fn image_root_admission_and_stop_during_callback_leave_no_work() {
    for stopped in [false, true] {
        reset(NORMAL);
        let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        let hosted = fake();
        let batch = hosted
            .make_registered(hosted.begin().unwrap())
            .unwrap()
            .commit()
            .unwrap();
        let mut bytes = [0; 128];
        let len = commands_encode(
            &[WireCommand::ImagePrepare {
                root: u32::from(!stopped),
                path: "missing.png",
                side: 0,
                cookie: 1,
                flags: 0,
            }],
            &mut bytes,
        )
        .unwrap();
        let result = kernel.with_native_commands(
            &batch,
            &DispatchDepth::new(),
            super::super::command_dispatch::CommandTarget {
                screen: None,
                hop: 0,
                result: super::super::command_dispatch::CommandResult::Status,
            },
            paperpaw_platform::Instant(0),
            |context, _| {
                // SAFETY: the encoded bytes are live and disjoint from host state.
                let submitted = unsafe { context.commands.submit(bytes.as_ptr(), len) };
                if stopped {
                    assert_eq!(submitted, OK);
                    hosted.stop();
                    Ok(0)
                } else {
                    assert_eq!(submitted, host_glue::ERR_DENIED);
                    Err(Error::Status(submitted))
                }
            },
        );
        assert!(result.is_err());
        let context = hosted.owner().context();
        assert!(!context.resources.hosts().image_work.pending());
        assert!(!context.resources.hosts().images().prepare_pending());
        drop(batch);
        finish(&hosted);
    }
}
