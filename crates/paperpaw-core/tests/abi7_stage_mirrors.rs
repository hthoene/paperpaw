//! Mirrored stage records (`core.format`, `core.lookup`): the same record as a
//! service row under a stage-local tag, so the host passes it on by changing
//! only the tag (`docs/abi/document.md` sections 3 and 7).
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::registrations::{
    Record, SERVICE_DOC_STATE_TAG, SERVICE_END_TAG, SERVICE_LOOKUP_ASK_TAG, ServiceDocState,
    ServiceEnd, ServiceLookupAsk, record_encode_one,
};
use paperpaw_core::native_host::stages_wire::{core_format_v1 as format, core_lookup_v1 as lookup};

fn varint(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let byte = u8::try_from(value & 0x7f).unwrap();
        value >>= 7;
        out.push(byte | if value == 0 { 0 } else { 0x80 });
        if value == 0 {
            break;
        }
    }
}

/// The service frame `record` with its tag replaced by `local`, as a stage
/// batch of one (count, tag, then the service frame's length and body).
fn retagged(record: &Record<'_>, service: u32, local: u32) -> Vec<u8> {
    let mut frame = [0u8; 4096];
    let len = record_encode_one(record, &mut frame).unwrap();
    let mut tag = Vec::new();
    varint(service, &mut tag);
    assert_eq!(frame[..tag.len()], tag[..]);
    let mut out = vec![1];
    varint(local, &mut out);
    out.extend_from_slice(&frame[tag.len()..len]);
    out
}

fn local_of(table: &[(u32, u32)], service: u32) -> u32 {
    let mut found = table.iter().filter(|(_, kernel)| *kernel == service);
    let (local, _) = *found.next().unwrap();
    assert!(found.next().is_none());
    local
}

#[test]
fn the_tag_tables_name_each_mirror_once() {
    for (table, tags) in [
        (
            format::SERVICE_TAGS,
            [format::INPUT_TAGS, format::REPLY_TAGS],
        ),
        (
            lookup::SERVICE_TAGS,
            [lookup::INPUT_TAGS, lookup::REPLY_TAGS],
        ),
    ] {
        for (index, (local, service)) in table.iter().enumerate() {
            assert!(tags.iter().any(|side| side.contains(local)), "{local}");
            assert!(
                table[index + 1..]
                    .iter()
                    .all(|(l, s)| l != local && s != service)
            );
        }
    }
    assert_eq!(format::SERVICE_TAGS.len(), 21);
    assert_eq!(lookup::SERVICE_TAGS.len(), 1);
}

#[test]
fn a_service_row_is_a_mirrored_stage_row_with_another_tag() {
    let state = ServiceDocState {
        document: 7,
        state: 1,
        progress_ppm: 500_000,
        status: -8,
        flags: 3,
        conversion: u64::MAX,
        format: "format-epub",
    };
    let local = local_of(format::SERVICE_TAGS, SERVICE_DOC_STATE_TAG);
    let bytes = retagged(
        &Record::ServiceDocState(state),
        SERVICE_DOC_STATE_TAG,
        local,
    );
    let row = format::FmtState::decode(&bytes).unwrap();
    assert_eq!(
        (row.document, row.state, row.progress_ppm, row.status),
        (7, 1, 500_000, -8)
    );
    assert_eq!(
        (row.flags, row.conversion, row.format),
        (3, u64::MAX, "format-epub")
    );
}

#[test]
fn optional_fields_cross_too() {
    let local = local_of(format::SERVICE_TAGS, SERVICE_END_TAG);
    for cursor in [None, Some(u64::MAX)] {
        let end = ServiceEnd {
            flags: 1,
            total: 2,
            generation: 3,
            next: 4,
            cursor,
        };
        let bytes = retagged(&Record::ServiceEnd(end), SERVICE_END_TAG, local);
        let row = format::FmtEnd::decode(&bytes).unwrap();
        assert_eq!(
            (row.flags, row.total, row.next, row.cursor),
            (1, 2, 4, cursor)
        );
    }
}

#[test]
fn the_lookup_question_crosses_as_its_service_row() {
    let ask = ServiceLookupAsk {
        text: "word",
        language: "de",
        context: "a word in context",
        cursor: 2,
    };
    let local = local_of(lookup::SERVICE_TAGS, SERVICE_LOOKUP_ASK_TAG);
    let bytes = retagged(
        &Record::ServiceLookupAsk(ask),
        SERVICE_LOOKUP_ASK_TAG,
        local,
    );
    let row = lookup::LookupAsk::decode(&bytes).unwrap();
    assert_eq!(
        (row.text, row.language, row.context, row.cursor),
        ("word", "de", "a word in context", 2)
    );
}

#[test]
fn retagging_to_the_service_tag_grows_a_row_by_one_byte() {
    // Local tags below 128 take one byte, service tags 1024-1535 two (stages-services.md 2.4).
    let state = ServiceDocState {
        document: 1,
        state: 0,
        progress_ppm: 0,
        status: 0,
        flags: 0,
        conversion: 0,
        format: "",
    };
    let mut frame = [0u8; 256];
    let service = record_encode_one(&Record::ServiceDocState(state), &mut frame).unwrap();
    let local = local_of(format::SERVICE_TAGS, SERVICE_DOC_STATE_TAG);
    assert!(local < 128 && SERVICE_DOC_STATE_TAG >= 128);
    // `retagged` adds the batch's count byte.
    let stage = retagged(
        &Record::ServiceDocState(state),
        SERVICE_DOC_STATE_TAG,
        local,
    )
    .len()
        - 1;
    assert_eq!(service, stage + 1);
    // A full stage reply, every row grown so, still fits the service's 4,096 bytes.
    assert!(format::MAX_REPLY + format::REPLY_ROWS <= 4096);
}
