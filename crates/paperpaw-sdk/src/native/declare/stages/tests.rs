use super::*;
use alloc::{string::ToString, vec, vec::Vec};
use records_wire::{Record, records_decode};

fn spec(name: &str, input: u32, reply: u32, records: u32) -> StageDecl<'_> {
    StageDecl::new(name, 42, input, reply, records)
}

#[test]
fn generated_records_preserve_stage_flags_limits_and_signed_order() {
    let mut batches = Vec::new();
    let mut declarations = Declarations::new(|bytes: &[u8]| {
        batches.push(bytes.to_vec());
        Ok(())
    });
    declarations
        .stage(
            spec("insert.words", 3840, 3840, 64)
                .changes_layout()
                .draws()
                .takes_taps(),
        )
        .unwrap();
    declarations
        .subscribe(
            SubscribeDecl::new("reader.insert", 99)
                .order(i16::MIN)
                .signature(u64::MAX),
        )
        .unwrap();
    declarations
        .subscribe(SubscribeDecl::new("reader.insert", 100).order(i16::MAX))
        .unwrap();
    declarations.submit().unwrap();
    drop(declarations);
    let batch = records_decode(&batches[0]).unwrap();
    let records: Vec<_> = batch.iter().collect();
    match records.as_slice() {
        [
            Record::StageSpec(stage),
            Record::RegistrationStage(first),
            Record::RegistrationStage(second),
        ] => {
            assert_eq!(
                (stage.name, stage.schema, stage.flags),
                ("insert.words", 42, 7)
            );
            assert_eq!(
                (stage.max_input, stage.max_reply, stage.max_records),
                (3840, 3840, 64)
            );
            assert_eq!(
                (first.stage, first.token, first.order, first.signature),
                ("reader.insert", 99, i32::from(i16::MIN), u64::MAX)
            );
            assert_eq!((second.order, second.signature), (i32::from(i16::MAX), 0));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn stage_refusals_latch_and_send_only_one_abort() {
    let long = "x".repeat(records_wire::STAGE_SPEC_NAME_MAX_BYTES + 1);
    let cases = [
        (spec("", 1, 1, 1), DeclareError::Empty("stage name")),
        (spec(&long, 1, 1, 1), DeclareError::TooLong("stage name")),
        (spec("Bad", 1, 1, 1), DeclareError::Encode(ERR_INVALID)),
        (spec(".bad", 1, 1, 1), DeclareError::Encode(ERR_INVALID)),
        (spec("bad.", 1, 1, 1), DeclareError::Encode(ERR_INVALID)),
        (
            spec("bad..name", 1, 1, 1),
            DeclareError::Encode(ERR_INVALID),
        ),
        (spec("bad/name", 1, 1, 1), DeclareError::Encode(ERR_INVALID)),
        (spec("good", 0, 1, 1), DeclareError::Encode(ERR_INVALID)),
        (spec("good", 3841, 1, 1), DeclareError::Encode(ERR_INVALID)),
        (spec("good", 1, 0, 1), DeclareError::Encode(ERR_INVALID)),
        (spec("good", 1, 3841, 1), DeclareError::Encode(ERR_INVALID)),
        (spec("good", 1, 1, 0), DeclareError::Encode(ERR_INVALID)),
        (spec("good", 1, 1, 65), DeclareError::Encode(ERR_INVALID)),
    ];
    for (stage, expected) in cases {
        let mut batches = Vec::new();
        let mut d = Declarations::new(|bytes: &[u8]| {
            batches.push(bytes.to_vec());
            Ok(())
        });
        d.stage(spec("pending", 1, 1, 1)).unwrap();
        assert_eq!(d.stage(stage).map(|_| ()), Err(expected));
        assert_eq!(
            d.subscribe(SubscribeDecl::new("reader.insert", 1))
                .map(|_| ()),
            Err(expected)
        );
        assert_eq!(d.submit(), Err(expected));
        assert_eq!(d.submit(), Err(expected));
        drop(d);
        assert_eq!(batches, [Vec::<u8>::new()]);
    }
}

#[test]
fn subscription_name_refusals() {
    for (stage, expected) in [
        (String::new(), DeclareError::Empty("subscription stage")),
        ("x".repeat(129), DeclareError::TooLong("subscription stage")),
        ("Bad.stage".to_string(), DeclareError::Encode(ERR_INVALID)),
        (
            "reader..stage".to_string(),
            DeclareError::Encode(ERR_INVALID),
        ),
        (
            "reader.stage_".to_string(),
            DeclareError::Encode(ERR_INVALID),
        ),
    ] {
        let mut batches = Vec::new();
        let mut d = Declarations::new(|bytes: &[u8]| {
            batches.push(bytes.to_vec());
            Ok(())
        });
        assert_eq!(
            d.subscribe(SubscribeDecl::new(&stage, 1)).map(|_| ()),
            Err(expected)
        );
        assert_eq!(d.submit(), Err(expected));
        drop(d);
        assert_eq!(batches, [Vec::<u8>::new()]);
    }
}

#[test]
fn duplicate_pair_is_refused_even_after_submit() {
    let mut batches = Vec::new();
    let mut d = Declarations::new(|bytes: &[u8]| {
        batches.push(bytes.to_vec());
        Ok(())
    });
    d.subscribe(SubscribeDecl::new("reader.insert", 7)).unwrap();
    d.submit().unwrap();
    assert_eq!(
        d.subscribe(SubscribeDecl::new("reader.insert", 7).order(99))
            .map(|_| ()),
        Err(DeclareError::Encode(ERR_INVALID))
    );
    assert_eq!(d.submit(), Err(DeclareError::Encode(ERR_INVALID)));
    drop(d);
    assert_eq!(batches.len(), 2);
    assert!(batches[1].is_empty());
}

#[test]
fn eight_subscriptions_are_allowed_across_batches_ninth_aborts() {
    let mut batches = Vec::new();
    let mut d = Declarations::new(|bytes: &[u8]| {
        batches.push(bytes.to_vec());
        Ok(())
    });
    let long = "a".repeat(128);
    for token in 0..8 {
        d.subscribe(SubscribeDecl::new(&long, token)).unwrap();
        d.submit().unwrap();
    }
    assert_eq!(
        d.subscribe(SubscribeDecl::new("reader.insert", 8))
            .map(|_| ()),
        Err(DeclareError::Encode(ERR_FULL))
    );
    assert_eq!(d.submit(), Err(DeclareError::Encode(ERR_FULL)));
    drop(d);
    assert_eq!(batches.len(), 9);
    assert_eq!(batches[8], vec![]);
}

#[test]
fn smallest_limits_and_default_flags_are_accepted() {
    let mut encoded = Vec::new();
    let mut d = Declarations::new(|bytes: &[u8]| {
        encoded = bytes.to_vec();
        Ok(())
    });
    d.stage(spec(&"x".repeat(96), 1, 1, 1)).unwrap();
    d.submit().unwrap();
    drop(d);
    let batch = records_decode(&encoded).unwrap();
    let Record::StageSpec(stage) = batch.iter().next().unwrap() else {
        panic!("wrong tag")
    };
    assert_eq!(stage.flags, 0);
    assert_eq!(
        (stage.max_input, stage.max_reply, stage.max_records),
        (1, 1, 1)
    );
}
