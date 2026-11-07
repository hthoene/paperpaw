//! Stage records 19/20 (`stage.spec`, `registration.stage`) and the host's own
//! 512/513/1024: syntax, forward compatibility, and refusal until wired.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use super::*;
use crate::native_host::registrations::{
    ServiceEcho, StageCall, StageReply, records_decode, records_encode,
};

const PLUGIN: &str = "reader";

fn spec(name: &str, flags: u32, input: u32, reply: u32, records: u32) -> StageSpec<'_> {
    StageSpec {
        name,
        schema: 1,
        flags,
        max_input: input,
        max_reply: reply,
        max_records: records,
    }
}

fn subscribe(stage: &str, order: i32) -> RegistrationStage<'_> {
    RegistrationStage {
        stage,
        token: 7,
        order,
        signature: 0x1234,
    }
}

#[test]
fn the_readers_insert_stage_and_a_subscription_are_well_formed() {
    let max = STAGE_CALL_PAYLOAD_MAX_BYTES as u32;
    assert_eq!(
        stage_spec_check(PLUGIN, &spec("insert", 0b111, 1, max, 64)),
        Ok(())
    );
    assert_eq!(
        stage_registration_check(&subscribe("reader.insert", -3)),
        Ok(())
    );
}

#[test]
fn bounds_names_and_orders_are_checked() {
    let max = STAGE_CALL_PAYLOAD_MAX_BYTES as u32;
    for bad in [
        spec("", 0, 1, 1, 1),
        spec("Insert", 0, 1, 1, 1),
        spec("insert", 0, 0, 1, 1),
        spec("insert", 0, 1, max + 1, 1),
        spec("insert", 0, 1, 1, 0),
        spec("insert", 0, 1, 1, 65),
    ] {
        assert_eq!(stage_spec_check(PLUGIN, &bad), Err(ERR_INVALID), "{bad:?}");
    }
    let long = "x".repeat(FULL_ID_MAX_BYTES);
    assert_eq!(
        stage_spec_check(PLUGIN, &spec(&long, 0, 1, 1, 1)),
        Err(ERR_INVALID)
    );
    for (stage, order) in [("", 0), ("reader..insert", 0), ("reader.insert", 40_000)] {
        assert_eq!(
            stage_registration_check(&subscribe(stage, order)),
            Err(ERR_INVALID),
            "{stage} {order}"
        );
    }
}

#[test]
fn an_unknown_flag_is_unsupported_not_invalid() {
    assert_eq!(
        stage_spec_check(PLUGIN, &spec("insert", 0b1000, 1, 1, 1)),
        Err(ERR_UNSUPPORTED)
    );
}

#[test]
fn well_formed_stage_declarations_are_refused_until_wired_and_host_records_never_pass() {
    let records = [
        Record::StageSpec(spec("insert", 1, 16, 16, 1)),
        Record::RegistrationStage(subscribe("reader.insert", 0)),
    ];
    for record in records {
        let mut out = [0; 512];
        let len = records_encode(&[record], &mut out).unwrap();
        let batch = records_decode(&out[..len]).unwrap();
        assert_eq!(validate_batch(PLUGIN, &batch), Err(ERR_UNSUPPORTED));
    }
    let host_records = [
        Record::StageCall(StageCall {
            stage: "reader.insert",
            schema: 1,
            payload: &[],
        }),
        Record::StageReply(StageReply {
            subscriber: "notes",
            index: 0,
            status: 0,
            flags: 0,
            signature: 0,
            payload: &[],
        }),
        Record::ServiceEcho(ServiceEcho { text: "hi" }),
    ];
    for record in host_records {
        assert_eq!(
            validate_record(PLUGIN, &record),
            Err(ERR_INVALID),
            "{record:?}"
        );
    }
}
