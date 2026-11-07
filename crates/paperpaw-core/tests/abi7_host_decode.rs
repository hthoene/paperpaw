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

fn framed(tag: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for mut value in [tag, u32::try_from(body.len()).unwrap()] {
        while value >= 0x80 {
            out.push((value & 0x7f) as u8 | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }
    out.extend_from_slice(body);
    out
}

/// Kinds only the host sends, each by its tag (`docs/abi/stages-services.md`).
/// Every other known kind (`service.echo`, 1024, a service input, among them)
/// is one a plugin sends.
const HOST_ONLY: [u32; 50] = [
    3,    // event.tick
    32,   // event.started
    33,   // event.resume
    34,   // event.suspend
    35,   // event.stopping
    36,   // event.battery_measured
    37,   // event.page_turned
    38,   // event.screen_shown
    39,   // event.action_run
    40,   // input.key
    41,   // input.pointer
    42,   // input.gesture
    43,   // input.action
    48,   // service.usage
    49,   // service.plugin_usage
    50,   // service.plugin
    51,   // service.work_note
    52,   // service.end
    64,   // event.setting_changed
    65,   // event.book_opened
    66,   // event.book_closed
    67,   // event.book_finished
    68,   // event.book_moved
    69,   // event.book_removed
    70,   // event.plugins_changed
    71,   // event.wifi_changed
    72,   // event.power_changed
    73,   // event.status_changed
    74,   // event.charge_changed
    75,   // event.registry_changed
    76,   // event.preview_changed
    77,   // event.plugins_requested
    78,   // event.plugin_answered
    192,  // draw.frame
    512,  // stage.call
    513,  // stage.reply
    1027, // service.reg_setting
    1028, // service.reg_setting_extra
    1029, // service.reg_choice
    1030, // service.reg_text
    1031, // service.reg_group
    1032, // service.reg_plugin
    1033, // service.reg_need
    1034, // service.reg_item
    1035, // service.reg_slot
    1036, // service.reg_menu_row
    1037, // service.reg_binding
    1038, // service.reg_setup_step
    1039, // service.reg_value
    1040, // service.reg_widget
];

fn host_only(tag: u32) -> bool {
    HOST_ONLY.contains(&tag)
}

#[test]
fn every_kind_only_the_host_sends_is_refused_and_every_other_decodes_as_before() {
    // Sixty-four zero bytes are a valid body of every generated record: an
    // empty bitmap, zero scalars, empty text, trailing bytes skipped.
    let zeros = [0; 64];
    let (mut refused, mut decoded) = (Vec::new(), Vec::new());
    for tag in 0..2048 {
        let bytes = framed(tag, &zeros);
        let full = record_decode_one(&bytes);
        let host = host_decode_one(&bytes);
        if tag != 3 && full == Err(ERR_UNSUPPORTED) {
            assert_eq!(host, Err(ERR_UNSUPPORTED), "unknown tag {tag}");
            continue;
        }
        if host_only(tag) {
            assert_eq!(host, Err(ERR_INVALID), "host-only tag {tag}");
            assert_eq!(host_decode_prefix(&bytes).map(|(r, _)| r), Err(ERR_INVALID));
            let mut two = vec![2];
            two.extend(bare(GROUP));
            two.extend(&bytes);
            assert_eq!(host_records_decode(&two), Err(ERR_INVALID), "tag {tag}");
            refused.push(tag);
        } else {
            assert!(full.is_ok(), "tag {tag}: {full:?}");
            assert_eq!(host, full, "tag {tag}");
            decoded.push(tag);
        }
    }
    // Pinned: a new record kind must be placed on one side on purpose.
    assert_eq!(refused.len(), HOST_ONLY.len(), "{refused:?}");
    assert_eq!(decoded.len(), 32, "{decoded:?}");
}

#[test]
fn a_tick_is_invalid_bare_as_a_prefix_and_in_a_batch() {
    // `event.tick` (tag 3) has its own codec and only the host sends it.
    let tick = [3, 8, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(host_decode_one(&tick), Err(ERR_INVALID));
    assert_eq!(host_decode_prefix(&tick).map(|(r, _)| r), Err(ERR_INVALID));
    let mut one = vec![1];
    one.extend(tick);
    assert_eq!(host_records_decode(&one), Err(ERR_INVALID));
    let mut two = vec![2];
    two.extend(bare(GROUP));
    two.extend(tick);
    let mut held = RecordBatch::EMPTY;
    assert_eq!(host_records_decode_into(&two, &mut held), Err(ERR_INVALID));
    assert!(held.is_empty());
    // A malformed tick body is still refused before it is read.
    assert_eq!(host_decode_one(&[3, 1, 0xff]), Err(ERR_INVALID));
}

#[test]
fn unknown_tags_and_limits_keep_their_statuses() {
    // Tag 79 has no record yet: unsupported, as in `records_decode`.
    assert_eq!(host_decode_one(&[79, 1, 0]), Err(ERR_UNSUPPORTED));
    assert_eq!(record_decode_one(&[79, 1, 0]), Err(ERR_UNSUPPORTED));
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
