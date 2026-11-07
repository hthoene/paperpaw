//! Deferred application uses the admitted caller and ends every callback borrow.
use super::*;
use crate::native_host::commands::{WireCommand, commands_encode};

#[test]
fn command_stage_applies_only_after_callback_scopes_and_discards_failed_calls() {
    for fail in [false, true] {
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
                    if fail {
                        Err(Error::Status(ERR_INVALID))
                    } else {
                        Ok(OK)
                    }
                })?
            },
        );
        if fail {
            assert!(matches!(result, Err(Error::Status(ERR_INVALID))));
        } else {
            assert_eq!(result, Ok(OK));
        }
        assert_eq!(kernel.shared.settings.bool("fake.toggle"), !fail);
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
