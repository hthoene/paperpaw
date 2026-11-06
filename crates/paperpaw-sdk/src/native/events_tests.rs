#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use wire::{
    EventActionRun, EventPageTurned, EventScreenShown, InputAction, Registration, records_encode,
};

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
        insert: Some(2),
        progress_ppm: 250_000,
        path: "books/a.epub",
        title: "A Title",
        chapter: "One",
        page: Some(12),
        pages: Some(300),
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
        assert_eq!((got.page, got.pages, got.insert), (p, n, 2));
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

#[test]
fn page_insert_ordinals_preserve_source_position_and_progress() {
    let mut bytes = [0; 1200];
    for insert in [None, Some(0), Some(1), Some(u32::MAX)] {
        let record = EventPageTurned { insert, ..page() };
        let len = records_encode(&[Record::EventPageTurned(record)], &mut bytes).unwrap();
        let Event::PageTurned(got) = decode(&bytes[..len]).unwrap() else {
            panic!()
        };
        assert_eq!(got.insert, insert.unwrap_or(0));
        assert_eq!(
            (got.section, got.offset, got.progress_ppm),
            (record.section, record.offset, record.progress_ppm)
        );
        assert_eq!((got.page, got.pages), (record.page, record.pages));
    }
}

#[test]
fn screen_and_action_events_borrow_the_exact_input_fields() {
    let mut bytes = [0; 256];
    let now_ms = 0x1234_5678_9abc_def0;
    let screen = Record::EventScreenShown(EventScreenShown {
        now_ms,
        screen: "demo.main",
        width: 0x1234_5678,
        height: 0x9abc_def0,
    });
    let len = records_encode(&[screen], &mut bytes).unwrap();
    assert_eq!(bytes[1], 38);
    let Event::ScreenShown {
        now_ms: got_ms,
        screen,
        width,
        height,
    } = decode(&bytes[..len]).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (got_ms, screen, width, height),
        (now_ms, "demo.main", 0x1234_5678, 0x9abc_def0)
    );
    assert!(bytes[..len].as_ptr_range().contains(&screen.as_ptr()));
    let action = Record::EventActionRun(EventActionRun {
        now_ms,
        action: "demo.refresh",
    });
    let len = records_encode(&[action], &mut bytes).unwrap();
    assert_eq!(bytes[1], 39);
    let Event::ActionRun {
        now_ms: got_ms,
        action,
    } = decode(&bytes[..len]).unwrap()
    else {
        panic!()
    };
    assert_eq!((got_ms, action), (now_ms, "demo.refresh"));
    assert!(bytes[..len].as_ptr_range().contains(&action.as_ptr()));
}

#[test]
fn screen_and_action_events_refuse_bad_framing_and_utf8() {
    for record in [
        Record::EventScreenShown(EventScreenShown {
            now_ms: 9,
            screen: "demo.main",
            width: 800,
            height: 600,
        }),
        Record::EventActionRun(EventActionRun {
            now_ms: 13,
            action: "demo.refresh",
        }),
    ] {
        let mut bytes = [0; 256];
        let len = records_encode(&[record], &mut bytes).unwrap();
        for end in 0..len {
            assert!(decode(&bytes[..end]).is_err());
        }
        let mut trailing = bytes[..len].to_vec();
        trailing.push(0);
        assert_eq!(decode(&trailing), Err(Error::Invalid));
        let position = bytes[..len].windows(5).position(|s| s == b"demo.").unwrap();
        bytes[position] = 255;
        assert_eq!(decode(&bytes[..len]), Err(Error::Utf8));
        let len = records_encode(&[record, record], &mut bytes).unwrap();
        assert_eq!(decode(&bytes[..len]), Err(Error::Invalid));
    }
}

#[test]
fn event_decoding_uses_exact_tags_for_growth_and_input_records() {
    let mut bytes = [0; 64];
    let len = records_encode(
        &[Record::InputAction(InputAction {
            now_ms: 17,
            action: "demo.refresh",
        })],
        &mut bytes,
    )
    .unwrap();
    assert_eq!(decode(&bytes[..len]), Err(Error::Unsupported));
    for tag in [63, 64, 127, 191] {
        // A reserved event-family tag remains unsupported even with valid framing.
        let unknown = if tag < 128 {
            alloc::vec![1, tag, 1, 0]
        } else {
            alloc::vec![1, tag | 0x80, 1, 1, 0]
        };
        assert_eq!(decode(&unknown), Err(Error::Unsupported));
    }
}

#[test]
fn generated_tick_vectors_decode_and_reject_every_truncation() {
    let mut bytes = [0; 12];
    for now_ms in [0_u64, u64::MAX] {
        let len = wire::tick_encode(now_ms, &mut bytes).unwrap();
        assert_eq!(Event::decode(&bytes[..len]), Ok(Event::Tick { now_ms }));
        for end in 0..len {
            assert_eq!(Event::decode(&bytes[..end]), Err(Error::Invalid));
        }
    }
}

#[test]
fn tick_optional_extensions_are_generated_decoder_compatible() {
    // One appended bitmap byte and two appended field bytes are forward-compatible.
    let bytes = [1, 3, 12, 1, 1, 7, 0, 0, 0, 0, 0, 0, 0, 99, 42];
    assert_eq!(wire::tick_decode(&bytes), Ok(7));
    assert_eq!(Event::decode(&bytes), Ok(Event::Tick { now_ms: 7 }));
    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert_eq!(Event::decode(&trailing), Err(Error::Invalid));
}

#[test]
fn tick_fallback_preserves_exact_tags_and_singleton_framing() {
    let mut bytes = [0; 12];
    let len = wire::tick_encode(17, &mut bytes).unwrap();
    for tag in [63, 64, 127] {
        bytes[1] = tag;
        assert_eq!(Event::decode(&bytes[..len]), Err(Error::Unsupported));
    }
    bytes[1] = 3;
    bytes[0] = 2;
    assert_eq!(Event::decode(&bytes[..len]), Err(Error::Invalid));
    let record = Record::Registration(Registration::Draw { token: 3 });
    let len = records_encode(&[record], &mut bytes).unwrap();
    assert_eq!(Event::decode(&bytes[..len]), Err(Error::Unsupported));
}
