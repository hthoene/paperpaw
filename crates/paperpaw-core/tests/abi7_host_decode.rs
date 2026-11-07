//! The host's decoders (lock format 10, #985): records a plugin sends are
//! decoded as by the full decoder; records only the host sends are refused
//! with `ERR_INVALID` before their body is read.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::registrations::{
    EventStarted, I18nArgStr, Record, RecordBatch, ServiceEnd, SettingGroup, host_decode_one,
    host_decode_prefix, host_records_decode, host_records_decode_into, record_decode_one,
    record_encode_one, records_decode, records_encode,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_UNSUPPORTED, ERR_UTF8};

const GROUP: Record<'static> = Record::SettingGroup(SettingGroup {
    name: "reading",
    title: "Reading",
    order: 3,
    page: "",
    parent: "",
});
const ARGUMENT: Record<'static> = Record::I18nArgStr(I18nArgStr {
    name: "n",
    value: "v",
});
const STARTED: Record<'static> = Record::EventStarted(EventStarted { now_ms: 9 });
const END: Record<'static> = Record::ServiceEnd(ServiceEnd {
    flags: 1,
    total: 0,
    generation: 2,
    next: 0,
});

fn batch(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = vec![0; 4096];
    let len = records_encode(records, &mut out).unwrap();
    out.truncate(len);
    out
}

fn bare(record: Record<'_>) -> Vec<u8> {
    let mut out = vec![0; 4096];
    let len = record_encode_one(&record, &mut out).unwrap();
    out.truncate(len);
    out
}

#[test]
fn what_plugins_send_decodes_as_the_full_decoder_does() {
    let bytes = batch(&[GROUP, ARGUMENT]);
    let host = host_records_decode(&bytes).unwrap();
    assert_eq!(host, records_decode(&bytes).unwrap());
    assert_eq!(host.iter().collect::<Vec<_>>(), [GROUP, ARGUMENT]);
    let one = bare(GROUP);
    assert_eq!(host_decode_one(&one), Ok(GROUP));
    assert_eq!(host_decode_prefix(&one), Ok((GROUP, one.len())));
}

#[test]
fn what_only_the_host_sends_is_invalid_before_its_body_is_read() {
    for record in [STARTED, END] {
        let one = bare(record);
        assert_eq!(record_decode_one(&one), Ok(record));
        assert_eq!(host_decode_one(&one), Err(ERR_INVALID));
        assert_eq!(host_decode_prefix(&one).map(|(r, _)| r), Err(ERR_INVALID));
        assert_eq!(
            host_records_decode(&batch(&[GROUP, record])),
            Err(ERR_INVALID)
        );
    }
    // A malformed body of such a record: the full decoder reads it and finds
    // bad UTF-8; the host never reads it (#985, Manager's rule).
    let mut end = bare(Record::ServiceWorkNote(
        paperpaw_core::native_host::registrations::ServiceWorkNote {
            at_ms: 1,
            text: "ab",
        },
    ));
    let last = end.len() - 1;
    end[last] = 0xff;
    assert_eq!(record_decode_one(&end), Err(ERR_UTF8));
    assert_eq!(host_decode_one(&end), Err(ERR_INVALID));
}

#[test]
fn unknown_tags_and_limits_keep_their_statuses() {
    // Tag 75 has no record yet: unsupported, as in `records_decode`.
    assert_eq!(host_decode_one(&[75, 1, 0]), Err(ERR_UNSUPPORTED));
    assert_eq!(record_decode_one(&[75, 1, 0]), Err(ERR_UNSUPPORTED));
    let mut nine = vec![9];
    for _ in 0..9 {
        nine.extend(bare(GROUP));
    }
    assert_eq!(host_records_decode(&nine), Err(ERR_NO_SPACE));
    assert_eq!(records_decode(&nine), Err(ERR_NO_SPACE));
}

#[test]
fn a_held_batch_is_empty_after_an_error_and_reusable() {
    let first = batch(&[GROUP, ARGUMENT]);
    let bad = batch(&[GROUP, STARTED]);
    let good = batch(&[ARGUMENT]);
    let mut held = RecordBatch::EMPTY;
    host_records_decode_into(&first, &mut held).unwrap();
    assert_eq!(held.len(), 2);
    assert_eq!(host_records_decode_into(&bad, &mut held), Err(ERR_INVALID));
    assert!(held.is_empty());
    assert_eq!(held, RecordBatch::EMPTY);
    host_records_decode_into(&good, &mut held).unwrap();
    assert_eq!(held, records_decode(&good).unwrap());
}
