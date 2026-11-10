//! The whole-plugin rules of stages (`stages-services.md` section 1.1):
//! one name, one token per stage, eight subscriptions, none to its own stage.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use super::*;
use crate::native_host::{
    raw_declarations::RawDeclarations,
    registrations::{REGISTRATION_MAX_BYTES, StageSpec, records_decode, records_encode},
    staged::Declarations,
};
use alloc::{vec, vec::Vec};
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID};

fn spec(name: &str) -> Record<'_> {
    Record::StageSpec(StageSpec {
        name,
        schema: 1,
        flags: 0,
        max_input: 8,
        max_reply: 8,
        max_records: 1,
    })
}

fn subscribe(stage: &str, token: u32) -> Record<'_> {
    Record::RegistrationStage(RegistrationStage {
        stage,
        token,
        order: 0,
        signature: 0,
    })
}

/// The plugin `reader`'s batches, committed and checked at the end.
fn finish(batches: &[&[Record<'_>]]) -> Result<(), i32> {
    let mut raw = RawDeclarations::default();
    for records in batches {
        let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
        let len = records_encode(records, &mut bytes).unwrap();
        bytes.truncate(len);
        let pending = raw
            .prepare(None, Some(&bytes), &records_decode(&bytes).unwrap())
            .unwrap();
        raw.commit(pending);
    }
    raw.sort_index();
    let owned = Declarations::default();
    View {
        raw: &raw,
        owned: &owned,
        image: None,
        plugin: "reader",
        refused: Default::default(),
    }
    .finish()
}

#[test]
fn a_stage_name_is_declared_once() {
    assert_eq!(finish(&[&[spec("insert"), spec("after")]]), Ok(()));
    assert_eq!(
        finish(&[&[spec("insert")], &[spec("insert")]]),
        Err(ERR_INVALID)
    );
}

#[test]
fn a_token_subscribes_to_a_stage_once_but_two_tokens_may() {
    assert_eq!(
        finish(&[&[subscribe("notes.insert", 1), subscribe("notes.insert", 2)]]),
        Ok(())
    );
    assert_eq!(
        finish(&[&[subscribe("notes.insert", 1), subscribe("notes.after", 1)]]),
        Ok(())
    );
    assert_eq!(
        finish(&[
            &[subscribe("notes.insert", 1)],
            &[subscribe("notes.insert", 1)]
        ]),
        Err(ERR_INVALID)
    );
}

#[test]
fn eight_subscriptions_are_the_most_and_the_ninth_is_full() {
    let rows: Vec<Record<'_>> = (0..9)
        .map(|token| subscribe("notes.insert", token))
        .collect();
    assert_eq!(finish(&[&rows[..8]]), Ok(()));
    assert_eq!(finish(&[&rows[..5], &rows[5..8]]), Ok(()));
    assert_eq!(finish(&[&rows[..8], &rows[8..]]), Err(ERR_FULL));
    assert_eq!(finish(&[&rows[..5], &rows[5..]]), Err(ERR_FULL));
}

#[test]
fn a_subscription_to_the_plugins_own_stage_refuses_the_batch() {
    // Same batch or another; the stage declared before or after.
    assert_eq!(
        finish(&[&[spec("insert"), subscribe("reader.insert", 1)]]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        finish(&[&[subscribe("reader.insert", 1)], &[spec("insert")]]),
        Err(ERR_INVALID)
    );
    // Another plugin's stage, or one that the plugin does not declare, is fine.
    assert_eq!(
        finish(&[&[spec("insert"), subscribe("notes.insert", 1)]]),
        Ok(())
    );
    assert_eq!(
        finish(&[&[spec("insert"), subscribe("reader.other", 1)]]),
        Ok(())
    );
}
