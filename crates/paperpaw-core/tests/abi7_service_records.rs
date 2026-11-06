//! Service records 48-52 and the bare framing of service blobs
//! (`docs/abi/proposals/interface-7-usage-clock.md`, sections 2.2 and 2.4).
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::{
    declarations::validate_record,
    registrations::{
        Record, SERVICE_END_TAG, SERVICE_PLUGIN_NAME_MAX_BYTES,
        SERVICE_PLUGIN_USAGE_AWAKE_REASON_MAX_BYTES, SERVICE_PLUGIN_USAGE_PLUGIN_MAX_BYTES,
        SERVICE_WORK_NOTE_TEXT_MAX_BYTES, ServiceEnd, ServicePlugin, ServicePluginUsage,
        ServiceUsage, ServiceWorkNote, record_decode_one, record_encode_one, record_framed_len,
    },
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE};

fn usage() -> ServiceUsage {
    ServiceUsage {
        fast: 1,
        partial: 2,
        full: 3,
        pixels: u64::MAX,
        inputs: 5,
        sleeps: 6,
        panel_ms: 7,
        bluetooth_ms: 8,
    }
}

fn row<'a>(plugin: &'a str, awake_reason: &'a str) -> ServicePluginUsage<'a> {
    ServicePluginUsage {
        plugin,
        busy_us: 1,
        calls: 2,
        shown_ms: 3,
        fast: 4,
        partial: 5,
        full: 6,
        pixels: 7,
        read: 8,
        written: 9,
        syncs: 10,
        requests: 11,
        refused: 12,
        awake_ms: u64::from(u32::MAX),
        awake_reason,
    }
}

fn end() -> ServiceEnd {
    ServiceEnd {
        flags: 1,
        total: 70,
        generation: 3,
        next: 4,
    }
}

fn round_trip(record: Record<'_>) -> usize {
    let mut out = [0u8; 512];
    let len = record_encode_one(&record, &mut out).unwrap();
    assert_eq!(len, record_framed_len(&record).unwrap());
    assert_eq!(record_decode_one(&out[..len]), Ok(record));
    len
}

#[test]
fn framed_sizes_are_the_contracts() {
    // Section 2.2: tag 48 is 47 bytes framed, tag 52 is 19, a row at most 210,
    // tag 50 at most 133 and tag 51 at most 270 (section 2.6).
    assert_eq!(round_trip(Record::ServiceUsage(usage())), 47);
    assert_eq!(round_trip(Record::ServiceEnd(end())), 19);
    let id = "p".repeat(SERVICE_PLUGIN_USAGE_PLUGIN_MAX_BYTES);
    let why = "w".repeat(SERVICE_PLUGIN_USAGE_AWAKE_REASON_MAX_BYTES);
    assert_eq!(round_trip(Record::ServicePluginUsage(row(&id, &why))), 210);
    let name = "n".repeat(SERVICE_PLUGIN_NAME_MAX_BYTES);
    assert_eq!(
        round_trip(Record::ServicePlugin(ServicePlugin { name: &name })),
        133
    );
    let text = "t".repeat(SERVICE_WORK_NOTE_TEXT_MAX_BYTES);
    let note = ServiceWorkNote {
        at_ms: 9,
        text: &text,
    };
    assert_eq!(round_trip(Record::ServiceWorkNote(note)), 270);
}

#[test]
fn a_bare_record_has_no_count_and_nothing_after_it() {
    let mut out = [0u8; 64];
    let len = record_encode_one(&Record::ServiceEnd(end()), &mut out).unwrap();
    assert_eq!(out[0], SERVICE_END_TAG as u8);
    assert_eq!(usize::from(out[1]), len - 2);
    let mut longer = out;
    longer[len] = 0;
    assert_eq!(record_decode_one(&longer[..=len]), Err(ERR_INVALID));
    for cut in 0..len {
        assert!(record_decode_one(&out[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn an_appended_field_is_skipped_by_length() {
    // Forward compatibility (`docs/abi/records.md`): a later field inside the
    // record's length is skipped; the known fields still decode.
    let mut out = [0u8; 64];
    let len = record_encode_one(&Record::ServiceEnd(end()), &mut out).unwrap();
    let mut grown = [0u8; 64];
    grown[0] = out[0];
    grown[1] = out[1] + 4;
    grown[2..len].copy_from_slice(&out[2..len]);
    grown[len..len + 4].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(
        record_decode_one(&grown[..len + 4]),
        Ok(Record::ServiceEnd(end()))
    );
}

#[test]
fn over_cap_or_short_output_writes_nothing() {
    let id = "p".repeat(SERVICE_PLUGIN_USAGE_PLUGIN_MAX_BYTES + 1);
    let over = Record::ServicePluginUsage(row(&id, ""));
    assert_eq!(record_framed_len(&over), Err(ERR_NO_SPACE));
    let mut out = [0xAAu8; 512];
    assert_eq!(record_encode_one(&over, &mut out), Err(ERR_NO_SPACE));
    assert!(out.iter().all(|&b| b == 0xAA));
    let fits = Record::ServiceUsage(usage());
    let mut short = [0xAAu8; 46];
    assert_eq!(record_encode_one(&fits, &mut short), Err(ERR_NO_SPACE));
    assert!(short.iter().all(|&b| b == 0xAA));
}

#[test]
fn a_plugin_cannot_submit_a_service_record() {
    let text = "note";
    for record in [
        Record::ServiceUsage(usage()),
        Record::ServicePluginUsage(row("p", "")),
        Record::ServicePlugin(ServicePlugin { name: "n" }),
        Record::ServiceWorkNote(ServiceWorkNote { at_ms: 0, text }),
        Record::ServiceEnd(end()),
    ] {
        assert_eq!(validate_record("p", &record), Err(ERR_INVALID));
    }
}
