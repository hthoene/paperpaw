//! The typed records codec (`WireEncode`, `WireRecord`, `RawRecord`) writes and reads
//! exactly what the `Record` enum's codec does (#1156): a plugin links only the record
//! types it uses, with the same bytes and statuses.
extern crate std;

use super::records_wire::{self as wire, Record, RecordStream, WireEncode};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_UNSUPPORTED};

fn page_turned(path: &str) -> wire::EventPageTurned<'_> {
    wire::EventPageTurned {
        now_ms: 7,
        section: 2,
        offset: 300,
        progress_ppm: 41_000,
        path,
        title: "Title",
        chapter: "One",
        page: Some(12),
        pages: None,
        insert: Some(1),
        chapter_left: None,
    }
}

const END: wire::ServiceEnd = wire::ServiceEnd {
    flags: 1,
    total: 3,
    generation: 9,
    next: 3,
    cursor: Some(1 << 40),
};

const PLAIN_END: wire::ServiceEnd = wire::ServiceEnd {
    flags: 0,
    total: 0,
    generation: 0,
    next: 0,
    cursor: None,
};

fn bare(record: &dyn WireEncode) -> Vec<u8> {
    let mut out = [0; 1024];
    let n = wire::record_encode_of(record, &mut out).unwrap();
    out[..n].to_vec()
}

fn enum_bare(record: &Record<'_>) -> Vec<u8> {
    let mut out = [0; 1024];
    let n = wire::record_encode_one(record, &mut out).unwrap();
    out[..n].to_vec()
}

#[test]
fn typed_and_enum_encoders_write_the_same_bytes() {
    let event = page_turned("/books/a.epub");
    let registration = wire::Registration::Event {
        event_tag: wire::EVENT_PAGE_TURNED_TAG,
        token: 4,
    };
    assert_eq!(bare(&event), enum_bare(&Record::EventPageTurned(event)));
    assert_eq!(bare(&END), enum_bare(&Record::ServiceEnd(END)));
    assert_eq!(bare(&PLAIN_END), enum_bare(&Record::ServiceEnd(PLAIN_END)));
    assert_eq!(
        bare(&registration),
        enum_bare(&Record::Registration(registration))
    );
    assert_eq!(
        wire::record_framed_len_of(&event),
        wire::record_framed_len(&Record::EventPageTurned(event))
    );

    let (mut typed, mut old) = ([0; 512], [0; 512]);
    let records: [&dyn WireEncode; 3] = [&event, &END, &registration];
    let n = wire::records_encode_of(&records, &mut typed).unwrap();
    let enums = [
        Record::EventPageTurned(event),
        Record::ServiceEnd(END),
        Record::Registration(registration),
    ];
    let m = wire::records_encode(&enums, &mut old).unwrap();
    assert_eq!(typed[..n], old[..m]);

    let n =
        wire::records_encode_counted_of([END, PLAIN_END].into_iter(), 2, 64, &mut typed).unwrap();
    let m = wire::records_encode_counted(
        &[Record::ServiceEnd(END), Record::ServiceEnd(PLAIN_END)],
        2,
        64,
        &mut old,
    )
    .unwrap();
    assert_eq!(typed[..n], old[..m]);
}

#[test]
fn typed_encoders_refuse_what_the_enum_encoders_refuse_and_write_nothing() {
    let long = "x".repeat(wire::EVENT_PAGE_TURNED_PATH_MAX_BYTES + 1);
    let over = page_turned(&long);
    let mut out = [0xa5; 64];
    assert_eq!(wire::record_encode_of(&over, &mut out), Err(ERR_NO_SPACE));
    assert_eq!(
        wire::record_encode_of(&END, &mut out[..4]),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(out, [0xa5; 64]);
    let nine: [&dyn WireEncode; 9] = [&END; 9];
    assert_eq!(
        wire::records_encode_of(&nine, &mut [0; 512]),
        Err(ERR_NO_SPACE)
    );
    let three = [END; 3];
    assert_eq!(
        wire::records_encode_counted_of(three.iter(), 2, 512, &mut out),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(
        wire::records_encode_counted_of(three.iter(), 3, 10, &mut out),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(out, [0xa5; 64]);
}

#[test]
fn raw_records_decode_by_type_as_the_enum_decoders_do() {
    let event = page_turned("/b.epub");
    let bytes = bare(&event);
    let raw = wire::RawRecord::one(&bytes).unwrap();
    assert_eq!(raw.tag(), wire::EVENT_PAGE_TURNED_TAG);
    assert_eq!(raw.decode::<wire::EventPageTurned<'_>>(), Ok(event));
    assert_eq!(raw.record(), wire::record_decode_one(&bytes));
    // Another type is an unknown tag to it.
    assert_eq!(raw.decode::<wire::ServiceEnd>(), Err(ERR_UNSUPPORTED));

    let mut batch = [0; 256];
    let n = wire::records_encode_of(&[&END], &mut batch).unwrap();
    let single = wire::RawRecord::single(&batch[..n]).unwrap();
    assert_eq!(single.decode::<wire::ServiceEnd>(), Ok(END));
    assert_eq!(single.record(), wire::records_decode_single(&batch[..n]));

    let mut two = bare(&END);
    two.extend_from_slice(&bytes);
    let (first, len) = wire::RawRecord::prefix(&two).unwrap();
    assert_eq!(len, bare(&END).len());
    assert_eq!(first.decode::<wire::ServiceEnd>(), Ok(END));
    assert_eq!(
        wire::record_decode_prefix(&two),
        first.record().map(|r| (r, len))
    );
}

#[test]
fn raw_framing_errors_are_the_enum_framing_errors() {
    let bytes = bare(&END);
    let cases: [&[u8]; 4] = [&bytes[..bytes.len() - 1], &[], &[0x80], &[3, 0x80, 0]];
    for case in cases {
        assert_eq!(
            wire::RawRecord::one(case).map(|_| ()),
            wire::record_decode_one(case).map(|_| ()),
            "{case:?}"
        );
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert_eq!(wire::RawRecord::one(&trailing), Err(ERR_INVALID));
    let mut batch = [0; 256];
    let n = wire::records_encode_of(&[&END, &END], &mut batch).unwrap();
    assert_eq!(wire::RawRecord::single(&batch[..n]), Err(ERR_INVALID));
    assert_eq!(
        wire::RawRecord::one(&[0; wire::REGISTRATION_MAX_BYTES + 1]),
        Err(ERR_NO_SPACE)
    );
}

#[test]
fn a_raw_stream_visits_what_the_record_stream_visits() {
    let mut bytes = bare(&page_turned("/x.epub"));
    bytes.extend(bare(&END));
    for chunk in [1, 3, 7, bytes.len()] {
        let (mut raw, mut old) = (RecordStream::new(), RecordStream::new());
        let (mut raws, mut olds) = (Vec::new(), Vec::new());
        for part in bytes.chunks(chunk) {
            raw.feed_raw(part, &mut |r| {
                raws.push(enum_bare(&r.record()?));
                Ok(())
            })
            .unwrap();
            old.feed(part, &mut |r| {
                olds.push(enum_bare(&r));
                Ok(())
            })
            .unwrap();
        }
        raw.finish().unwrap();
        assert_eq!(raws, olds);
        assert_eq!(raws.len(), 2);
    }
}

/// The random bytes of `core.secrets` (`service.sec_bytes.data`, a secret field) never
/// reach a Debug string: neither the bytes nor their length (Senior R2 on #367).
#[test]
fn secret_bytes_stay_out_of_debug() {
    let record = wire::ServiceSecBytes {
        flags: 5,
        data: &[0xAB, 0xCD, 0xEF],
    };
    let text = std::format!("{record:?}");
    assert_eq!(text, r#"ServiceSecBytes { flags: 5, data: ".." }"#);
    let text = std::format!("{:?}", Record::ServiceSecBytes(record));
    assert!(!text.contains("171") && !text.contains('3'), "{text}");
}

/// A checked frame and a stream's carry may hold a secret field: their Debug shows the
/// tag and lengths only, never the bytes (Senior M1 on #364).
#[test]
fn raw_records_and_stream_carry_keep_secrets_out_of_debug() {
    let secret = "hunter2-secret";
    let record = wire::ServiceSecValue {
        flags: 0,
        label: "wifi",
        value: secret,
    };
    let mut out = [0_u8; 64];
    let n = wire::record_encode_of(&record, &mut out).unwrap();
    let (raw, used) = wire::RawRecord::prefix(&out[..n]).unwrap();
    assert_eq!(used, n);
    let text = std::format!("{raw:?}");
    assert!(text.starts_with("RawRecord { tag: "), "{text}");
    assert!(text.contains("body_len"), "{text}");
    assert!(!text.contains("104") && !text.contains("hunter"), "{text}");
    // Half the frame stays in the carry until the rest arrives.
    let mut stream = RecordStream::new();
    stream.feed_raw(&out[..n - 4], &mut |_| Ok(())).unwrap();
    let text = std::format!("{stream:?}");
    assert_eq!(
        text,
        std::format!("RecordStream {{ carry_len: {} }}", n - 4)
    );
    let mut seen = 0;
    stream
        .feed_raw(&out[n - 4..n], &mut |raw| {
            assert_eq!(raw.decode::<wire::ServiceSecValue<'_>>(), Ok(record));
            seen += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, 1);
}

/// A bare record of `tag` with an empty body.
fn empty_record(tag: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut value = tag;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out.push(0);
    out
}

#[test]
fn known_tags_are_exactly_the_tags_the_enum_decoder_does_not_refuse_as_unknown() {
    let tags = (0..=4096).chain([65_535, 65_536, 65_536 + 1024, u32::MAX >> 4, u32::MAX]);
    let mut known = 0;
    for tag in tags {
        let bytes = empty_record(tag);
        let raw = wire::RawRecord::one(&bytes).unwrap();
        let unknown = raw.record() == Err(ERR_UNSUPPORTED);
        assert_eq!(wire::record_tag_known(tag), !unknown, "tag {tag}");
        known += usize::from(!unknown);
    }
    assert!(known > 200, "only {known} known tags");
    assert!(wire::record_tag_known(wire::SERVICE_END_TAG));
    assert!(wire::record_tag_known(wire::EVENT_PAGE_TURNED_TAG));
}
