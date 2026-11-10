//! Event records 32-37: host encode, decode, caps, and registration admission.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::{
    DispatchDepth, HostOwner,
    events::{self, PageTurn},
    registration_scope::RegistrationScope,
    registrations::{
        self, EVENT_BATTERY_MEASURED_TAG, EVENT_PAGE_TURNED_CHAPTER_MAX_BYTES,
        EVENT_PAGE_TURNED_PATH_MAX_BYTES, EVENT_PAGE_TURNED_TAG, EVENT_PAGE_TURNED_TITLE_MAX_BYTES,
        EVENT_RESUME_TAG, EVENT_STARTED_TAG, EVENT_STOPPING_TAG, EVENT_SUSPEND_TAG, EVENT_TICK_TAG,
        EventPageTurned, EventStarted, Record, Registration, SettingGroup, records_encode,
    },
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_UNSUPPORTED, OK};
use std::pin::Pin;

type Encoder = fn(u64, &mut [u8]) -> Result<usize, i32>;

const TAGS: [u32; 6] = [
    EVENT_STARTED_TAG,
    EVENT_RESUME_TAG,
    EVENT_SUSPEND_TAG,
    EVENT_STOPPING_TAG,
    EVENT_BATTERY_MEASURED_TAG,
    EVENT_PAGE_TURNED_TAG,
];

fn turn() -> PageTurn<'static> {
    PageTurn {
        now_ms: 0x0102_0304_0506_0708,
        section: 3,
        offset: 4_000,
        progress_ppm: 250_000,
        path: "books/a.epub",
        title: "A Title",
        chapter: "One",
        page: Some(12),
        pages: Some(300),
    }
}

fn page(turn: &PageTurn<'_>) -> Vec<u8> {
    let mut out = [0; 4096];
    let n = events::encode_page_turned(turn, &mut out).unwrap();
    out[..n].to_vec()
}

#[test]
fn simple_events_round_trip_with_distinct_tags() {
    let encoders: [Encoder; 5] = [
        events::encode_started,
        events::encode_resume,
        events::encode_suspend,
        events::encode_stopping,
        events::encode_battery_measured,
    ];
    for (index, encode) in encoders.iter().enumerate() {
        for now in [0, 1, u64::MAX] {
            let mut out = [0xAA; 32];
            let n = encode(now, &mut out).unwrap();
            // Count, tag, length, empty bitmap, eight little-endian bytes.
            assert_eq!(&out[..4], &[1, TAGS[index] as u8, 9, 0]);
            assert_eq!(&out[4..n], &now.to_le_bytes());
            assert_eq!(n, 12);
            let record = events::decode(&out[..n]).unwrap();
            let again = match record {
                Record::EventStarted(r) => (EVENT_STARTED_TAG, r.now_ms),
                Record::EventResume(r) => (EVENT_RESUME_TAG, r.now_ms),
                Record::EventSuspend(r) => (EVENT_SUSPEND_TAG, r.now_ms),
                Record::EventStopping(r) => (EVENT_STOPPING_TAG, r.now_ms),
                Record::EventBatteryMeasured(r) => (EVENT_BATTERY_MEASURED_TAG, r.now_ms),
                _ => panic!("not a simple event"),
            };
            assert_eq!(again, (TAGS[index], now));
        }
    }
}

#[test]
fn short_output_is_refused_and_left_unchanged() {
    for size in 0..12 {
        let mut out = vec![0x5A; size];
        assert_eq!(events::encode_started(7, &mut out), Err(ERR_NO_SPACE));
        assert!(out.iter().all(|byte| *byte == 0x5A));
    }
}

#[test]
fn page_turned_round_trips_with_and_without_counters() {
    for (page_no, pages) in [
        (Some(12), Some(300)),
        (None, None),
        (Some(1), None),
        (None, Some(9)),
    ] {
        let wanted = PageTurn {
            page: page_no,
            pages,
            ..turn()
        };
        let bytes = page(&wanted);
        let Ok(Record::EventPageTurned(got)) = events::decode(&bytes) else {
            panic!("not a page turn");
        };
        assert_eq!(got.now_ms, wanted.now_ms);
        assert_eq!(
            (got.section, got.offset, got.progress_ppm),
            (3, 4_000, 250_000)
        );
        assert_eq!(
            (got.path, got.title, got.chapter),
            ("books/a.epub", "A Title", "One")
        );
        assert_eq!((got.page, got.pages), (page_no, pages));
    }
    // Absent counters cost no bytes: the bitmap stays empty.
    assert_eq!(
        page(&PageTurn {
            page: None,
            pages: None,
            ..turn()
        })[3],
        0
    );
    // Count, tag, a one-byte length, an extended-bitmap flag, then the presence bits.
    assert_eq!(page(&turn())[4], 0b11);
}

#[test]
fn page_turned_strings_at_their_caps_fit_and_longer_ones_are_cut() {
    let path = "p".repeat(EVENT_PAGE_TURNED_PATH_MAX_BYTES);
    let title = "t".repeat(EVENT_PAGE_TURNED_TITLE_MAX_BYTES);
    let chapter = "c".repeat(EVENT_PAGE_TURNED_CHAPTER_MAX_BYTES);
    let longest = PageTurn {
        path: &path,
        title: &title,
        chapter: &chapter,
        ..turn()
    };
    let bytes = page(&longest);
    assert!(bytes.len() < 1_200);
    let Ok(Record::EventPageTurned(got)) = events::decode(&bytes) else {
        panic!()
    };
    assert_eq!(
        (got.path.len(), got.title.len(), got.chapter.len()),
        (512, 256, 256)
    );
    // One over each cap is cut by the host, never refused.
    let (p, t, c) = (
        format!("{path}x"),
        format!("{title}x"),
        format!("{chapter}x"),
    );
    let over = PageTurn {
        path: &p,
        title: &t,
        chapter: &c,
        ..turn()
    };
    assert_eq!(page(&over), bytes);
    // A two-byte char straddling the cap is dropped whole.
    let straddle = format!("{}é", "a".repeat(EVENT_PAGE_TURNED_TITLE_MAX_BYTES - 1));
    let cut = PageTurn {
        title: &straddle,
        ..turn()
    };
    let bytes = page(&cut);
    let Ok(Record::EventPageTurned(got)) = events::decode(&bytes) else {
        panic!()
    };
    assert_eq!(got.title.len(), EVENT_PAGE_TURNED_TITLE_MAX_BYTES - 1);
    assert_eq!(events::cut("héllo", 2), "h");
    assert_eq!(events::cut("héllo", 0), "");
    assert_eq!(events::cut("ab", 9), "ab");
}

#[test]
fn raw_encoder_refuses_over_cap_fields_and_non_events() {
    let long = "x".repeat(EVENT_PAGE_TURNED_PATH_MAX_BYTES + 1);
    let record = Record::EventPageTurned(EventPageTurned {
        now_ms: 1,
        section: 0,
        offset: 0,
        progress_ppm: 0,
        path: &long,
        title: "",
        chapter: "",
        page: None,
        pages: None,
        insert: None,
        chapter_left: None,
    });
    let mut out = [0x77; 2048];
    assert_eq!(events::encode(&record, &mut out), Err(ERR_NO_SPACE));
    assert!(out.iter().all(|byte| *byte == 0x77));
    let group = Record::SettingGroup(SettingGroup {
        name: "g",
        title: "G",
        order: 0,
        page: "",
        parent: "",
    });
    assert_eq!(events::encode(&group, &mut out), Err(ERR_INVALID));
    let draw = Record::Registration(Registration::Draw { token: 1 });
    assert_eq!(events::encode(&draw, &mut out), Err(ERR_INVALID));
}

#[test]
fn page_turned_values_that_cannot_be_said_are_invalid() {
    let mut out = [0; 2048];
    for bad in [
        PageTurn {
            progress_ppm: 1_000_001,
            ..turn()
        },
        PageTurn {
            page: Some(0),
            ..turn()
        },
        PageTurn {
            pages: Some(0),
            ..turn()
        },
        PageTurn {
            page: Some(301),
            pages: Some(300),
            ..turn()
        },
    ] {
        assert_eq!(events::encode_page_turned(&bad, &mut out), Err(ERR_INVALID));
    }
    let last = PageTurn {
        page: Some(300),
        pages: Some(300),
        ..turn()
    };
    assert!(events::encode_page_turned(&last, &mut out).is_ok());
    let full = PageTurn {
        progress_ppm: 1_000_000,
        ..turn()
    };
    assert!(events::encode_page_turned(&full, &mut out).is_ok());
}

/// What the encoder refuses, the decoder refuses too, also when another
/// encoder wrote it: a present 0 (the bit set, the field 0) and a page past
/// the count.
#[test]
fn decode_refuses_page_turns_that_cannot_be_said() {
    let mut out = [0; 2048];
    for (page_no, pages) in [(Some(0), None), (None, Some(0)), (Some(9), Some(8))] {
        let record = Record::EventPageTurned(EventPageTurned {
            now_ms: 1,
            section: 0,
            offset: 0,
            progress_ppm: 0,
            path: "",
            title: "",
            chapter: "",
            page: page_no,
            pages,
            insert: None,
            chapter_left: None,
        });
        let n = events::encode(&record, &mut out).unwrap();
        assert_eq!(
            events::decode(&out[..n]),
            Err(ERR_INVALID),
            "{page_no:?} {pages:?}"
        );
    }
}

#[test]
fn decode_refuses_malformed_input_without_panicking() {
    let good = page(&turn());
    // Every truncation, and every single-byte corruption of the framing, is
    // either decoded or refused with a status.
    for end in 0..good.len() {
        assert!(events::decode(&good[..end]).is_err());
    }
    for index in 0..good.len() {
        for value in [0, 1, 0x7F, 0x80, 0xFF] {
            let mut bytes = good.clone();
            bytes[index] = value;
            let _ = events::decode(&bytes);
        }
    }
    let mut trailing = good.clone();
    trailing.push(0);
    assert!(events::decode(&trailing).is_err());
    assert!(events::decode(&[]).is_err());
    assert_eq!(events::decode(&[0]), Err(ERR_INVALID));
    // Two events in one batch, and a non-event record, are not one event.
    let mut two = [0; 64];
    let both = [
        Record::EventStarted(EventStarted { now_ms: 1 }),
        Record::EventStarted(EventStarted { now_ms: 2 }),
    ];
    let n = records_encode(&both, &mut two).unwrap();
    assert_eq!(events::decode(&two[..n]), Err(ERR_INVALID));
    let draw = [Record::Registration(Registration::Draw { token: 1 })];
    let n = records_encode(&draw, &mut two).unwrap();
    assert_eq!(events::decode(&two[..n]), Err(ERR_UNSUPPORTED));
    // Over-cap path length in the wire is refused.
    let mut over = vec![1, EVENT_PAGE_TURNED_TAG as u8, 0xFF, 0x7F, 0];
    over.extend([0; 20]);
    assert!(events::decode(&over).is_err());
}

#[test]
fn decoder_accepts_trailing_fields_of_a_future_minor() {
    // now_ms, then one extra trailing byte inside the record body.
    let mut bytes = vec![1, EVENT_STARTED_TAG as u8, 10, 0];
    bytes.extend(5u64.to_le_bytes());
    bytes.push(9);
    let Ok(Record::EventStarted(got)) = events::decode(&bytes) else {
        panic!()
    };
    assert_eq!(got.now_ms, 5);
}

#[test]
fn only_suspend_and_stopping_bypass_the_queue() {
    for tag in TAGS
        .into_iter()
        .chain([EVENT_TICK_TAG])
        .chain(EDGES_AND_HINTS)
    {
        assert!(events::is_event_tag(tag));
        assert_eq!(
            events::bypasses_queue(tag),
            tag == EVENT_SUSPEND_TAG || tag == EVENT_STOPPING_TAG
        );
    }
    // 64, the book events 65-69 (registered, delivered by the reader port and
    // the library moves) and the hints 70-74 are admitted.
    for tag in [0, 1, 2, 4, 5, 31, 38, 43, 63, u32::MAX] {
        assert!(!events::is_event_tag(tag));
    }
}

/// The events with a delivery: setting changes (64) and the hints (70-74).
const EDGES_AND_HINTS: [u32; 6] = [64, 70, 71, 72, 73, 74];

type Context = RegistrationScope<Registration>;

fn submit_one(tag: u32) -> (i32, bool) {
    let owner = Box::pin(HostOwner::new(RegistrationScope::new(), ()));
    let slot: Pin<&Context> = owner.as_ref().context();
    let mut bytes = [0; 64];
    let n = registrations::records_encode(
        &[Record::Registration(Registration::Event {
            event_tag: tag,
            token: 9,
        })],
        &mut bytes,
    )
    .unwrap();
    let mut construction = owner.as_ref().begin("weather").unwrap();
    let mut status = -99;
    slot.scope(&mut construction, || {
        // SAFETY: the array outlives the call.
        status = unsafe { registrations::submit(&slot, bytes.as_ptr(), n) };
    })
    .unwrap();
    let batch = construction.commit(paperpaw_core::native_host::Teardown {
        report: |_, _, _| {},
        cancel: |_| Ok(()),
        release: |_| Ok(()),
        destroy: |_| {},
    });
    let dispatched = batch.is_ok_and(|batch| {
        batch.dispatch(0, &DispatchDepth::new(), |_, r| *r)
            == Ok(Registration::Event {
                event_tag: tag,
                token: 9,
            })
    });
    drop(owner);
    (status, dispatched)
}

#[test]
fn registration_admits_tick_and_every_event_kind_and_refuses_the_rest() {
    for tag in TAGS
        .into_iter()
        .chain([EVENT_TICK_TAG])
        .chain(EDGES_AND_HINTS)
    {
        assert_eq!(submit_one(tag), (OK, true), "tag {tag}");
    }
    for tag in [0, 4, 5, 31, 38, 63, u32::MAX] {
        assert_eq!(submit_one(tag), (ERR_UNSUPPORTED, false), "tag {tag}");
    }
}

/// `event.book_opened`, `book_closed`, `book_finished`, `book_moved` and
/// `book_removed` register (rules, setup and the reader subscribe to them);
/// nothing produces them until the reader port and the library moves.
#[test]
fn the_book_events_register_and_each_is_admitted() {
    for tag in 65..=69 {
        assert!(events::is_event_tag(tag), "tag {tag}");
        assert_eq!(submit_one(tag), (OK, true), "tag {tag}");
        assert!(!events::bypasses_queue(tag), "tag {tag}");
    }
}

/// The answers of a plugin's own requests register: `event.wifi_done` (93) and the work
/// events `work_due`, `work_refused` and `work_allowed` (103-105).
#[test]
fn the_wifi_done_and_work_events_register_and_each_is_admitted() {
    for tag in [93, 103, 104, 105] {
        assert!(events::is_event_tag(tag), "tag {tag}");
        assert_eq!(submit_one(tag), (OK, true), "tag {tag}");
        assert!(!events::bypasses_queue(tag), "tag {tag}");
    }
}

#[test]
fn a_registration_batch_cannot_carry_an_event_record() {
    let events = [
        Record::EventStarted(EventStarted { now_ms: 1 }),
        Record::EventPluginsChanged(registrations::EventPluginsChanged { now_ms: 1 }),
        Record::EventSettingChanged(registrations::EventSettingChanged {
            now_ms: 1,
            key: "weather.city",
            old: &[],
        }),
    ];
    for event in events {
        let owner = Box::pin(HostOwner::new(RegistrationScope::<Registration>::new(), ()));
        let slot = owner.as_ref().context();
        let mut bytes = [0; 64];
        let n = records_encode(&[event], &mut bytes).unwrap();
        let mut construction = owner.as_ref().begin("weather").unwrap();
        slot.scope(&mut construction, || {
            // SAFETY: the array outlives the call.
            let status = unsafe { registrations::submit(&slot, bytes.as_ptr(), n) };
            assert_eq!(status, ERR_INVALID);
        })
        .unwrap();
        assert!(
            construction
                .commit(paperpaw_core::native_host::Teardown {
                    report: |_, _, _| {},
                    cancel: |_| Ok(()),
                    release: |_| Ok(()),
                    destroy: |_| {}
                })
                .is_err()
        );
    }
}

/// `event.stack_changed` (81, a hint) registers; the stack path raises it
/// later.
#[test]
fn the_stack_changed_hint_registers() {
    assert!(events::is_event_tag(81));
    assert_eq!(submit_one(81), (OK, true));
    assert!(!events::bypasses_queue(81));
}
