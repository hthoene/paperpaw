//! One callback stages up to `CALLBACK_COMMANDS` commands, one per batch; the
//! one after that, and bytes over the budget, are `ERR_NO_SPACE` with nothing
//! extra staged.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use crate::abi7::commands_wire::commands_encode;
use alloc::string::ToString;
use paperpaw_native::host_glue::{ERR_NO_SPACE, OK};
use paperpaw_sdk::builtin::groups;

fn settings() -> Settings {
    let mut settings = Settings::new();
    settings
        .define(SettingSpec::switch(
            "probe.flag",
            "flag",
            false,
            groups::OTHER,
        ))
        .unwrap();
    settings
        .define(SettingSpec::text("probe.note", "note", "", groups::OTHER))
        .unwrap();
    settings
}

/// One command in a batch of its own, through the slot's entry.
fn submit(
    scope: &core::pin::Pin<&mut CommandScope>,
    stage: &mut CommandStage<'_>,
    command: WireCommand<'_>,
) -> i32 {
    let mut bytes = [0; 4096];
    let len = commands_encode(&[command], &mut bytes).unwrap();
    scope
        .as_ref()
        .scope(stage, || {
            // SAFETY: the initialized encoded input stays readable and disjoint
            // from the stage until the synchronous call ends.
            unsafe { scope.submit(bytes.as_ptr(), len) }
        })
        .unwrap()
}

#[test]
fn a_callback_stages_64_single_command_batches_and_refuses_the_65th() {
    let settings = settings();
    let owner = Id::new_static("probe");
    let mut stage = CommandStage::new(
        &owner,
        &settings,
        None,
        CALLBACK_COMMANDS,
        CALLBACK_COMMAND_BYTES,
    );
    let scope = core::pin::pin!(CommandScope::new());
    let flag = WireCommand::SetSetting {
        key: "probe.flag",
        value: SettingValueRef::Bool(true),
    };
    for _ in 0..CALLBACK_COMMANDS {
        assert_eq!(submit(&scope, &mut stage, flag), OK);
    }
    assert_eq!(stage.staged().len(), CALLBACK_COMMANDS);
    for _ in 0..2 {
        assert_eq!(submit(&scope, &mut stage, flag), ERR_NO_SPACE);
        assert_eq!(stage.staged().len(), CALLBACK_COMMANDS);
    }
}

#[test]
fn bytes_over_the_budget_are_no_space_and_stage_nothing_more() {
    let settings = settings();
    let owner = Id::new_static("probe");
    let mut stage = CommandStage::new(
        &owner,
        &settings,
        None,
        CALLBACK_COMMANDS,
        CALLBACK_COMMAND_BYTES,
    );
    let scope = core::pin::pin!(CommandScope::new());
    let text = "x".repeat(1000);
    let note = WireCommand::SetSetting {
        key: "probe.note",
        value: SettingValueRef::Text(&text),
    };
    // 4 x (10 key + 1000 text) = 4040 bytes fit in 4096; the 5th does not.
    for _ in 0..4 {
        assert_eq!(submit(&scope, &mut stage, note), OK);
    }
    assert_eq!(stage.staged().len(), 4);
    assert_eq!(submit(&scope, &mut stage, note), ERR_NO_SPACE);
    assert_eq!(stage.staged().len(), 4);
    // A small command still fits in what is left of the budget.
    let flag = WireCommand::SetSetting {
        key: "probe.flag",
        value: SettingValueRef::Bool(true),
    };
    assert_eq!(submit(&scope, &mut stage, flag), OK);
    assert_eq!(stage.staged().len(), 5);
    let _ = text.to_string();
}
