#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use wire::{EventPageTurned, Registration, records_encode};

// Same scalar vectors as core's abi7_events::simple_events_round_trip_with_distinct_tags.
#[test]
fn host_scalar_vectors_decode_with_distinct_tags() {
    for now_ms in [0_u64, 1, u64::MAX] {
        for tag in 32..=36 {
            let mut bytes = [0; 12];
            bytes[..4].copy_from_slice(&[1, tag, 9, 0]);
            bytes[4..].copy_from_slice(&now_ms.to_le_bytes());
            let expected = match tag {
                32 => Event::Started { now_ms },
                33 => Event::Resume { now_ms },
                34 => Event::Suspend { now_ms },
                35 => Event::Stopping { now_ms },
                _ => Event::BatteryMeasured { now_ms },
            };
            assert_eq!(decode(&bytes), Ok(expected));
            for end in 0..bytes.len() {
                assert!(decode(&bytes[..end]).is_err());
            }
        }
    }
}
fn page() -> EventPageTurned<'static> {
    EventPageTurned {
        now_ms: 0x0102_0304_0506_0708,
        section: 3,
        offset: 4000,
        progress_ppm: 250_000,
        path: "books/a.epub",
        title: "A Title",
        chapter: "One",
        page: Some(12),
        pages: Some(300),
        // N5: a page that starts in text (absent = 0).
        insert: None,
    }
}
#[test]
fn page_vectors_preserve_counters_and_borrow_input_strings() {
    let mut bytes = [0; 1200];
    for (p, n) in [
        (None, None),
        (Some(1), None),
        (None, Some(9)),
        (Some(12), Some(300)),
    ] {
        let record = EventPageTurned {
            page: p,
            pages: n,
            ..page()
        };
        let len = records_encode(&[Record::EventPageTurned(record)], &mut bytes).unwrap();
        let Event::PageTurned(got) = decode(&bytes[..len]).unwrap() else {
            panic!()
        };
        assert_eq!(
            (got.now_ms, got.section, got.offset, got.progress_ppm),
            (record.now_ms, 3, 4000, 250_000)
        );
        assert_eq!(
            (got.path, got.title, got.chapter),
            (record.path, record.title, record.chapter)
        );
        assert_eq!((got.page, got.pages), (p, n));
        assert!(bytes.as_ptr_range().contains(&got.path.as_ptr()));
        assert!(bytes.as_ptr_range().contains(&got.title.as_ptr()));
        assert!(bytes.as_ptr_range().contains(&got.chapter.as_ptr()));
    }
}
#[test]
fn malformed_and_non_event_batches_are_refused() {
    let mut bytes = [0; 1200];
    for record in [
        EventPageTurned {
            progress_ppm: 1_000_001,
            ..page()
        },
        EventPageTurned {
            page: Some(0),
            ..page()
        },
        EventPageTurned {
            pages: Some(0),
            ..page()
        },
        EventPageTurned {
            page: Some(301),
            ..page()
        },
    ] {
        let n = records_encode(&[Record::EventPageTurned(record)], &mut bytes).unwrap();
        assert_eq!(decode(&bytes[..n]), Err(Error::Invalid));
    }
    let r = Record::Registration(Registration::Event {
        event_tag: 32,
        token: 1,
    });
    let n = records_encode(&[r], &mut bytes).unwrap();
    assert_eq!(decode(&bytes[..n]), Err(Error::Unsupported));
    let n = records_encode(&[r, r], &mut bytes).unwrap();
    assert_eq!(decode(&bytes[..n]), Err(Error::Invalid));
    assert_eq!(decode(&[0]), Err(Error::Invalid));
    assert_eq!(decode(&[1, 63, 1, 0]), Err(Error::Unsupported));
}
#[test]
fn forward_fields_are_accepted_and_trailing_batch_bytes_are_refused() {
    let bytes = [1, 32, 12, 1, 1, 7, 0, 0, 0, 0, 0, 0, 0, 99, 42];
    assert_eq!(decode(&bytes), Ok(Event::Started { now_ms: 7 }));
    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert_eq!(decode(&trailing), Err(Error::Invalid));
    let mut bytes = [0; 1200];
    let len = records_encode(&[Record::EventPageTurned(page())], &mut bytes).unwrap();
    let position = bytes[..len].windows(12).position(|s| s == b"books/a.epub");
    // The path is 12 bytes; corrupt its leading byte without changing framing.
    bytes[position.unwrap()] = 255;
    assert_eq!(decode(&bytes[..len]), Err(Error::Utf8));
}
