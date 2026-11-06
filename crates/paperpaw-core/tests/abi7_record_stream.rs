//! Single-record and streaming decoders over the records codec
//! (`docs/abi/proposals/interface-7-usage-clock.md` 2.4, lock format 8).
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::registrations::{
    EventStarted, RECORD_CARRY_MAX_BYTES, Record, RecordStream, ServiceEnd, ServicePluginUsage,
    record_decode_prefix, record_encode_one, records_decode_single, records_encode,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

fn row(plugin: &str) -> Record<'_> {
    Record::ServicePluginUsage(ServicePluginUsage {
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
        awake_ms: 13,
        awake_reason: "wifi",
    })
}

const END: Record<'static> = Record::ServiceEnd(ServiceEnd {
    flags: 0,
    total: 3,
    generation: 7,
    next: 3,
});

/// A table page: three rows (one with a 64-byte id) and the end record.
fn page(out: &mut [u8]) -> usize {
    let long = "p".repeat(64);
    let mut len = 0;
    for record in [row("core"), row(&long), row("battery"), END] {
        len += record_encode_one(&record, &mut out[len..]).unwrap();
    }
    len
}

fn read_in_chunks(bytes: &[u8], size: usize) -> Result<Vec<u32>, i32> {
    let mut stream = RecordStream::new();
    let mut tags = Vec::new();
    for chunk in bytes.chunks(size) {
        stream.feed(chunk, &mut |record| {
            tags.push(match record {
                Record::ServicePluginUsage(row) => row.plugin.len() as u32,
                Record::ServiceEnd(end) => 1000 + end.generation,
                _ => 0,
            });
            Ok(())
        })?;
    }
    stream.finish()?;
    Ok(tags)
}

#[test]
fn a_prefix_reports_the_frame_it_consumed() {
    let mut out = [0u8; 1024];
    let len = page(&mut out);
    let (first, used) = record_decode_prefix(&out[..len]).unwrap();
    assert_eq!(first, row("core"));
    let (second, more) = record_decode_prefix(&out[used..len]).unwrap();
    assert!(matches!(second, Record::ServicePluginUsage(r) if r.plugin.len() == 64));
    assert!(used > 0 && more > used);
    assert_eq!(record_decode_prefix(&out[..used - 1]), Err(ERR_INVALID));
}

#[test]
fn every_chunk_size_reads_the_same_records() {
    let mut out = [0u8; 1024];
    let len = page(&mut out);
    let whole = read_in_chunks(&out[..len], len).unwrap();
    assert_eq!(whole, vec![4, 64, 7, 1007]);
    for size in 1..=len {
        assert_eq!(
            read_in_chunks(&out[..len], size).unwrap(),
            whole,
            "chunk {size}"
        );
    }
}

#[test]
fn an_incomplete_last_record_is_refused_at_finish() {
    let mut out = [0u8; 1024];
    let len = page(&mut out);
    for cut in 1..len {
        let mut stream = RecordStream::new();
        let fed = stream.feed(&out[..cut], &mut |_| Ok(()));
        let ended = fed.and_then(|()| stream.finish());
        let whole_records = [0, 1, 2, 3].iter().any(|&n| {
            let mut at = 0;
            for _ in 0..n {
                at += record_decode_prefix(&out[at..len]).unwrap().1;
            }
            at == cut
        });
        assert_eq!(ended.is_ok(), whole_records, "cut {cut}");
    }
}

#[test]
fn a_record_longer_than_the_carry_must_arrive_whole() {
    // A frame header announcing a body past the carry, split across chunks.
    let mut big = vec![49, 0x82, 0x02];
    big.resize(3 + 258, 0);
    let mut stream = RecordStream::new();
    assert_eq!(stream.feed(&big[..10], &mut |_| Ok(())), Ok(()));
    assert_eq!(
        stream.feed(&big[10..RECORD_CARRY_MAX_BYTES + 5], &mut |_| Ok(())),
        Err(ERR_INVALID)
    );
}

#[test]
fn a_visit_error_stops_the_stream() {
    let mut out = [0u8; 1024];
    let len = page(&mut out);
    let mut stream = RecordStream::new();
    let mut seen = 0;
    let result = stream.feed(&out[..len], &mut |_| {
        seen += 1;
        if seen == 2 { Err(-9) } else { Ok(()) }
    });
    assert_eq!((result, seen), (Err(-9), 2));
}

#[test]
fn unknown_tags_and_non_canonical_headers_are_errors() {
    let mut stream = RecordStream::new();
    assert_eq!(stream.feed(&[60, 0], &mut |_| Ok(())), Err(ERR_UNSUPPORTED));
    let mut stream = RecordStream::new();
    assert_eq!(
        stream.feed(&[0x80, 0x00, 0], &mut |_| Ok(())),
        Err(ERR_INVALID)
    );
}

#[test]
fn a_single_record_batch_decodes_without_a_batch() {
    let started = Record::EventStarted(EventStarted { now_ms: 42 });
    let mut out = [0u8; 64];
    let len = records_encode(&[started], &mut out).unwrap();
    assert_eq!(records_decode_single(&out[..len]), Ok(started));
    let two = records_encode(&[started, started], &mut out).unwrap();
    assert_eq!(records_decode_single(&out[..two]), Err(ERR_INVALID));
    let len = records_encode(&[started], &mut out).unwrap();
    out[len] = 0;
    assert_eq!(records_decode_single(&out[..=len]), Err(ERR_INVALID));
}
