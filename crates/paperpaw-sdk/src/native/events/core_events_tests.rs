#![allow(clippy::unwrap_used, reason = "invalid fixtures must fail tests")]
use super::*;
use crate::native::declare::records_wire::Record;
use crate::native::{declare::settings_value as settings, settings::SettingValue};
use crate::power::PowerLevel;
use alloc::{vec, vec::Vec};
use core::mem::MaybeUninit;

macro_rules! position {
    ($name:ident, $progress:expr, $page:expr, $pages:expr, $reason:expr) => {
        wire::$name {
            now_ms: 42,
            section: 2,
            offset: 300,
            insert: Some(3),
            progress_ppm: $progress,
            path: "books/a.epub",
            title: "A title",
            chapter: "Chapter",
            page: $page,
            pages: $pages,
            reason: $reason,
            chapter_left: None,
        }
    };
}
fn books(
    progress: u32,
    page: Option<u32>,
    pages: Option<u32>,
    reason: u32,
) -> [Record<'static>; 3] {
    [
        Record::EventBookOpened(position!(EventBookOpened, progress, page, pages, reason)),
        Record::EventBookClosed(position!(EventBookClosed, progress, page, pages, reason)),
        Record::EventBookFinished(position!(EventBookFinished, progress, page, pages, reason)),
    ]
}
fn fixtures() -> [Record<'static>; 11] {
    let [opened, closed, finished] = books(500_000, Some(1), Some(2), 1);
    [
        Record::EventSettingChanged(wire::EventSettingChanged {
            now_ms: 42,
            key: "reader.size",
            old: &[],
        }),
        opened,
        closed,
        finished,
        Record::EventBookMoved(wire::EventBookMoved {
            now_ms: 42,
            from: "old.epub",
            path: "new.epub",
        }),
        Record::EventBookRemoved(wire::EventBookRemoved {
            now_ms: 42,
            path: "old.epub",
        }),
        Record::EventPluginsChanged(wire::EventPluginsChanged { now_ms: 42 }),
        Record::EventWifiChanged(wire::EventWifiChanged {
            now_ms: 42,
            state: 4,
        }),
        Record::EventPowerChanged(wire::EventPowerChanged {
            now_ms: 42,
            level: 2,
            flags: 1,
            stretch: 4,
            mode: "core.offline",
        }),
        Record::EventStatusChanged(wire::EventStatusChanged { now_ms: 42 }),
        Record::EventChargeChanged(wire::EventChargeChanged {
            now_ms: 42,
            flags: 7,
        }),
    ]
}
fn encode(record: Record<'_>) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    let n = wire::records_encode(&[record], &mut bytes).unwrap();
    bytes.truncate(n);
    bytes
}
fn assert_borrowed_text(bytes: &[u8], text: &str) {
    assert!(bytes.as_ptr_range().contains(&text.as_ptr()));
}

#[test]
fn every_evt_record_has_an_exact_typed_view_and_borrows_its_text() {
    for (index, record) in fixtures().into_iter().enumerate() {
        let bytes = encode(record);
        assert_eq!(bytes[1], 64 + index as u8);
        match decode(&bytes).unwrap() {
            Event::SettingChanged { now_ms, key, old } => {
                assert_eq!((now_ms, key, old), (42, "reader.size", None));
                assert_borrowed_text(&bytes, key);
            }
            Event::Book {
                kind,
                reason,
                reading,
            } => {
                let expected = [BookKind::Opened, BookKind::Closed, BookKind::Finished][index - 1];
                assert_eq!(kind, expected);
                assert_eq!(reason, u32::from(kind != BookKind::Finished));
                assert_eq!(
                    (
                        reading.now_ms(),
                        reading.section(),
                        reading.offset(),
                        reading.insert()
                    ),
                    (42, 2, 300, 3)
                );
                assert_eq!(
                    (reading.progress_ppm(), reading.page(), reading.pages()),
                    (500_000, Some(1), Some(2))
                );
                assert_eq!(
                    (reading.path(), reading.title(), reading.chapter()),
                    ("books/a.epub", "A title", "Chapter")
                );
                assert_borrowed_text(&bytes, reading.path());
                assert_borrowed_text(&bytes, reading.title());
                assert_borrowed_text(&bytes, reading.chapter());
            }
            Event::BookMoved { now_ms, from, path } => {
                assert_eq!((now_ms, from, path), (42, "old.epub", "new.epub"));
                assert_borrowed_text(&bytes, from);
                assert_borrowed_text(&bytes, path);
            }
            Event::BookRemoved { now_ms, path } => {
                assert_eq!((now_ms, path), (42, "old.epub"));
                assert_borrowed_text(&bytes, path);
            }
            Event::PluginsChanged { now_ms } | Event::StatusChanged { now_ms } => {
                assert_eq!(now_ms, 42)
            }
            Event::WifiChanged { now_ms, state } => {
                assert_eq!((now_ms, state), (42, WifiLinkState::AccessPoint))
            }
            Event::PowerChanged(power) => {
                assert_eq!(
                    (
                        power.now_ms(),
                        power.level(),
                        power.automatic(),
                        power.stretch(),
                        power.mode()
                    ),
                    (42, PowerLevel::Offline, true, 4, "core.offline")
                );
                assert_borrowed_text(&bytes, power.mode());
            }
            Event::ChargeChanged { now_ms, flags } => {
                assert_eq!(now_ms, 42);
                assert!(flags.charging() && flags.external_power() && flags.battery_low());
            }
            other => panic!("unexpected event {other:?}"),
        }
    }
}

#[test]
fn every_truncation_and_multiple_or_trailing_records_are_refused() {
    for record in fixtures() {
        let mut bytes = encode(record);
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err(), "tag {} end {end}", bytes[1]);
        }
        bytes.push(0);
        assert!(decode(&bytes).is_err());
        let mut two = [0; 4096];
        let n = wire::records_encode(&[record, record], &mut two).unwrap();
        assert_eq!(decode(&two[..n]), Err(Error::Invalid));
    }
}

fn future_fields(bytes: &[u8]) -> Vec<u8> {
    assert!(bytes[1] < 128);
    let mut start = 2;
    while bytes[start] & 0x80 != 0 {
        start += 1;
    }
    start += 1;
    let mut body = bytes[start..].to_vec();
    let bitmap_len = usize::from(body[0]);
    body[0] += 1;
    body.insert(1 + bitmap_len, 0x80); // Extend the length-prefixed presence bitmap.
    body.extend_from_slice(&[0xfe, 0xed]);
    let mut result = vec![1, bytes[1]];
    let mut length = body.len();
    loop {
        let byte = (length & 0x7f) as u8;
        length >>= 7;
        result.push(byte | if length == 0 { 0 } else { 0x80 });
        if length == 0 {
            break;
        }
    }
    result.extend_from_slice(&body);
    result
}
#[test]
fn future_optional_bits_and_appended_fields_preserve_known_values() {
    for record in fixtures() {
        let original = encode(record);
        let future = future_fields(&original);
        assert_eq!(decode(&future), decode(&original));
    }
}

fn setting_bytes(value: settings::SettingValueRef<'_>) -> Vec<u8> {
    let mut out = [MaybeUninit::uninit(); 512];
    let n = settings::settings_encode(value, &mut out).unwrap();
    // SAFETY: the encoder initialized the entire returned prefix inside capacity.
    out[..n]
        .iter()
        .map(|value| unsafe { value.assume_init() })
        .collect()
}
#[test]
fn old_setting_values_are_typed_and_malformed_is_never_missing() {
    for (wire_value, expected) in [
        (settings::SettingValueRef::Absent, SettingValue::Absent),
        (
            settings::SettingValueRef::Bool(false),
            SettingValue::Bool(false),
        ),
        (settings::SettingValueRef::Int(-42), SettingValue::Int(-42)),
        (
            settings::SettingValueRef::Text("prior"),
            SettingValue::Text("prior"),
        ),
    ] {
        let old = setting_bytes(wire_value);
        let bytes = encode(Record::EventSettingChanged(wire::EventSettingChanged {
            now_ms: 42,
            key: "reader.size",
            old: &old,
        }));
        let Event::SettingChanged { old, .. } = decode(&bytes).unwrap() else {
            panic!()
        };
        assert_eq!(old, Some(expected));
        if let Some(SettingValue::Text(text)) = old {
            assert_borrowed_text(&bytes, text);
        }
    }
    let valid = setting_bytes(settings::SettingValueRef::Bool(true));
    for end in 1..valid.len() {
        let bytes = encode(Record::EventSettingChanged(wire::EventSettingChanged {
            now_ms: 0,
            key: "reader.size",
            old: &valid[..end],
        }));
        assert_eq!(decode(&bytes), Err(Error::Invalid));
    }
    let mut invalid = valid;
    invalid[12] = 2;
    let bytes = encode(Record::EventSettingChanged(wire::EventSettingChanged {
        now_ms: 0,
        key: "reader.size",
        old: &invalid,
    }));
    assert_eq!(decode(&bytes), Err(Error::Invalid));
    let overflow = encode(Record::EventSettingChanged(wire::EventSettingChanged {
        now_ms: 42,
        key: "",
        old: &[],
    }));
    assert_eq!(
        decode(&overflow),
        Ok(Event::SettingChanged {
            now_ms: 42,
            key: "",
            old: None
        })
    );
}

#[test]
fn book_counter_rules_refuse_bad_values_and_reserved_reasons_become_zero() {
    for (progress, page, pages) in [
        (1_000_001, None, None),
        (0, Some(0), None),
        (0, None, Some(0)),
        (0, Some(3), Some(2)),
    ] {
        for record in books(progress, page, pages, 0) {
            assert_eq!(decode(&encode(record)), Err(Error::Invalid));
        }
    }
    for reason in [0, 1, 2, u32::MAX] {
        for record in books(1_000_000, None, Some(2), reason) {
            let bytes = encode(record);
            let Event::Book {
                kind,
                reason: got,
                reading,
            } = decode(&bytes).unwrap()
            else {
                panic!()
            };
            assert_eq!(got, u32::from(kind != BookKind::Finished && reason == 1));
            assert_eq!((reading.page(), reading.pages()), (None, Some(2)));
        }
    }
}

#[test]
fn radio_power_and_charge_reserved_values_follow_contract_fallbacks() {
    for (raw, state) in [
        (0, WifiLinkState::Off),
        (1, WifiLinkState::On),
        (2, WifiLinkState::Connecting),
        (3, WifiLinkState::Connected),
        (4, WifiLinkState::AccessPoint),
        (u32::MAX, WifiLinkState::On),
    ] {
        assert_eq!(
            decode(&encode(Record::EventWifiChanged(wire::EventWifiChanged {
                now_ms: 42,
                state: raw
            }))),
            Ok(Event::WifiChanged { now_ms: 42, state })
        );
    }
    for (raw, level) in [
        (0, PowerLevel::Normal),
        (1, PowerLevel::Saving),
        (2, PowerLevel::Offline),
        (3, PowerLevel::Normal),
        (u32::MAX, PowerLevel::Normal),
    ] {
        let bytes = encode(Record::EventPowerChanged(wire::EventPowerChanged {
            now_ms: 42,
            level: raw,
            flags: 0xffff_fffe,
            stretch: 0,
            mode: "preset.mine",
        }));
        let Event::PowerChanged(power) = decode(&bytes).unwrap() else {
            panic!()
        };
        assert_eq!(power.level(), level);
        assert!(!power.automatic());
        assert_eq!(power.stretch(), 0);
    }
    for raw in [0, 1, 2, 4, 7, 0xffff_fff8, u32::MAX] {
        let flags = ChargeFlags::from_bits(raw);
        assert_eq!(
            (
                flags.charging(),
                flags.external_power(),
                flags.battery_low()
            ),
            (raw & 1 != 0, raw & 2 != 0, raw & 4 != 0)
        );
    }
}

#[test]
fn invalid_utf8_and_noncanonical_framing_are_refused() {
    for record in fixtures() {
        let mut bytes = encode(record);
        let tag = bytes[1];
        if let Some(at) = bytes.windows(4).position(|part| part == b"epub") {
            bytes[at] = 0xff;
            assert_eq!(decode(&bytes), Err(Error::Utf8));
        }
        let mut bytes = encode(record);
        bytes[1] = tag | 0x80;
        bytes.insert(2, 0); // Overlong encoding of an unchanged exact tag.
        assert_eq!(decode(&bytes), Err(Error::Invalid));
    }
}

#[test]
fn complete_book_paths_and_text_caps_survive_without_truncation() {
    let path = "p".repeat(wire::EVENT_BOOK_OPENED_PATH_MAX_BYTES);
    let title = "t".repeat(wire::EVENT_BOOK_OPENED_TITLE_MAX_BYTES);
    let chapter = "c".repeat(wire::EVENT_BOOK_OPENED_CHAPTER_MAX_BYTES);
    let record = wire::EventBookOpened {
        path: &path,
        title: &title,
        chapter: &chapter,
        insert: None,
        page: None,
        pages: None,
        ..position!(EventBookOpened, 0, None, None, 0)
    };
    let bytes = encode(Record::EventBookOpened(record));
    let Event::Book { reading, .. } = decode(&bytes).unwrap() else {
        panic!()
    };
    assert_eq!(
        (reading.path(), reading.title(), reading.chapter()),
        (path.as_str(), title.as_str(), chapter.as_str())
    );
    assert_eq!(
        (reading.insert(), reading.page(), reading.pages()),
        (0, None, None)
    );
    assert_borrowed_text(&bytes, reading.path());
    let too_long = "p".repeat(wire::EVENT_BOOK_OPENED_PATH_MAX_BYTES + 1);
    let mut out = [0; 4096];
    assert_eq!(
        wire::records_encode(
            &[Record::EventBookOpened(wire::EventBookOpened {
                path: &too_long,
                ..record
            })],
            &mut out
        ),
        Err(paperpaw_native::host_glue::ERR_NO_SPACE)
    );
}

#[test]
fn bluetooth_event_has_an_exact_typed_view_and_borrows_its_name() {
    let bytes = encode(Record::EventBluetooth(wire::EventBluetooth {
        now_ms: 42,
        kind: 2,
        device: 9,
        name: "Remote",
        device_kind: 2,
        signal: -61,
        reason: 5,
    }));
    assert_eq!(bytes[1], 98);
    match Event::decode(&bytes).unwrap() {
        Event::Bluetooth(change) => {
            assert_eq!(
                (
                    change.now_ms(),
                    change.kind(),
                    change.device(),
                    change.name(),
                    change.device_kind(),
                    change.signal(),
                    change.reason()
                ),
                (
                    42,
                    BluetoothKind::Found,
                    9,
                    "Remote",
                    BluetoothDeviceKind::Remote,
                    Some(-61),
                    Some(BluetoothReason::Timeout)
                )
            );
            assert_borrowed_text(&bytes, change.name());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn bluetooth_values_follow_the_contract_and_unknown_ones_are_other() {
    let change = |kind, device_kind, signal, reason| {
        let bytes = encode(Record::EventBluetooth(wire::EventBluetooth {
            now_ms: 1,
            kind,
            device: 4,
            name: "x",
            device_kind,
            signal,
            reason,
        }));
        match Event::decode(&bytes).unwrap() {
            Event::Bluetooth(change) => (
                change.kind(),
                change.device_kind(),
                change.signal(),
                change.reason(),
            ),
            other => panic!("{other:?}"),
        }
    };
    use BluetoothDeviceKind as D;
    use BluetoothKind as K;
    use BluetoothReason as R;
    let kinds = [
        (1, K::Changed),
        (2, K::Found),
        (3, K::ScanEnded),
        (4, K::Connected),
        (5, K::Disconnected),
        (6, K::Paired),
        (7, K::Done),
        (8, K::Failed),
        (9, K::Other),
        (0, K::Other),
    ];
    for (wire, expected) in kinds {
        assert_eq!(change(wire, 0, 0, 0).0, expected, "kind {wire}");
    }
    let devices = [
        (0, D::Other),
        (1, D::Keyboard),
        (2, D::Remote),
        (3, D::Sensor),
        (4, D::Phone),
        (5, D::Other),
    ];
    for (wire, expected) in devices {
        assert_eq!(change(2, wire, 0, 0).1, expected, "device kind {wire}");
    }
    // Zero is "not known" for the signal and "not said" for the reason.
    assert_eq!(change(2, 0, 0, 0).2, None);
    assert_eq!(change(2, 0, -60, 0).2, Some(-60));
    assert_eq!(change(8, 0, 0, 0).3, None);
    let reasons = [
        (1, R::Off),
        (2, R::NotAllowed),
        (3, R::NotFound),
        (4, R::Busy),
        (5, R::Timeout),
        (6, R::Declined),
        (7, R::Lost),
        (8, R::Att),
        (9, R::Held),
        (10, R::Unsupported),
        (11, R::Other),
    ];
    for (wire, expected) in reasons {
        assert_eq!(change(8, 0, 0, wire).3, Some(expected), "reason {wire}");
    }
}

fn wifi_done(request: u32, status: i32, reason: u32, state: u32) -> Record<'static> {
    Record::EventWifiDone(wire::EventWifiDone {
        now_ms: 7,
        request,
        ssid: "Home",
        status,
        reason,
        state,
    })
}

#[test]
fn radio_hints_and_the_network_manager_edges_are_typed() {
    let bytes = encode(Record::EventWifiScanned(wire::EventWifiScanned {
        now_ms: 7,
        networks: 3,
    }));
    assert_eq!(
        decode(&bytes),
        Ok(Event::WifiScanned {
            now_ms: 7,
            networks: 3
        })
    );
    let bytes = encode(Record::EventNetworkWanted(wire::EventNetworkWanted {
        now_ms: 8,
    }));
    assert_eq!(decode(&bytes), Ok(Event::NetworkWanted { now_ms: 8 }));
    let bytes = encode(Record::EventNetworkIdle(wire::EventNetworkIdle {
        now_ms: 9,
    }));
    assert_eq!(decode(&bytes), Ok(Event::NetworkIdle { now_ms: 9 }));
}

#[test]
fn a_wifi_outcome_says_how_this_request_ended() {
    use crate::native::device::DeviceRequest;
    // Online, and hosting.
    for (state, expected) in [
        (3, WifiLinkState::Connected),
        (4, WifiLinkState::AccessPoint),
    ] {
        let bytes = encode(wifi_done(5, 0, 0, state));
        let Ok(Event::WifiDone(done)) = decode(&bytes) else {
            panic!("a typed outcome");
        };
        assert_eq!(done.now_ms(), 7);
        assert_eq!(done.request(), DeviceRequest::new(5));
        assert_eq!(done.ssid(), "Home");
        assert_borrowed_text(&bytes, done.ssid());
        assert_eq!((done.status(), done.failure()), (Ok(()), None));
        assert_eq!(done.state(), expected);
    }
    // Every reason of the contract, and an unknown one as `Other`.
    let reasons = [
        (1, Error::NotFound, WifiFailure::NotFound),
        (2, Error::Denied, WifiFailure::Password),
        (3, Error::Timeout, WifiFailure::Timeout),
        (4, Error::Cancelled, WifiFailure::Superseded),
        (5, Error::Unsupported, WifiFailure::Unsupported),
        (6, Error::Denied, WifiFailure::NeedsUser),
        (0, Error::Io, WifiFailure::Other),
        (99, Error::Io, WifiFailure::Other),
    ];
    for (reason, error, failure) in reasons {
        let status = error.status();
        let bytes = encode(wifi_done(5, status, reason, 0));
        let Ok(Event::WifiDone(done)) = decode(&bytes) else {
            panic!("a typed outcome");
        };
        assert_eq!(done.status(), Err(error));
        assert_eq!(done.failure(), Some(failure), "reason {reason}");
        assert_eq!(done.state(), WifiLinkState::Off);
    }
    // A join the host started itself has no request number; a failed one never
    // reports a state it did not reach.
    let bytes = encode(wifi_done(0, Error::Io.status(), 1, 3));
    let Ok(Event::WifiDone(done)) = decode(&bytes) else {
        panic!("a typed outcome");
    };
    assert_eq!(done.request(), None);
    assert_eq!(done.state(), WifiLinkState::Off);
}

#[test]
fn firmware_progress_and_install_outcomes_are_typed() {
    use crate::native::device::{DeviceOutcome, DeviceRequest, FirmwareState, InstallKind};
    let bytes = encode(Record::EventFirmwareChanged(wire::EventFirmwareChanged {
        now_ms: 5,
        request: 2,
        state: 2,
    }));
    assert_eq!(
        decode(&bytes),
        Ok(Event::FirmwareChanged {
            now_ms: 5,
            request: DeviceRequest::new(2),
            state: FirmwareState::Installing,
        })
    );
    let done = |request, kind, status, reason, flags| {
        encode(Record::EventInstallDone(wire::EventInstallDone {
            now_ms: 6,
            request,
            kind,
            status,
            reason,
            flags,
            id: "wifi",
        }))
    };
    // A removal that kept the data and takes effect at the next start.
    let bytes = done(9, 2, 0, 0, 0b11);
    let Ok(Event::InstallDone(end)) = decode(&bytes) else {
        panic!("a typed outcome");
    };
    assert_eq!(end.now_ms(), 6);
    assert_eq!(end.request(), DeviceRequest::new(9));
    assert_eq!(end.kind(), InstallKind::Removal);
    assert_eq!((end.status(), end.outcome()), (Ok(()), DeviceOutcome::Ok));
    assert!(end.data_kept() && end.at_next_start());
    assert_eq!(end.id(), "wifi");
    assert_borrowed_text(&bytes, end.id());
    // A refused install, an unknown reason and a future kind.
    let bytes = done(1, 0, Error::Denied.status(), 2, 0);
    let Ok(Event::InstallDone(end)) = decode(&bytes) else {
        panic!("a typed outcome");
    };
    assert_eq!(end.kind(), InstallKind::Plugin);
    assert_eq!(end.status(), Err(Error::Denied));
    assert_eq!(end.outcome(), DeviceOutcome::Declined);
    assert!(!end.data_kept() && !end.at_next_start());
    let bytes = done(1, 77, Error::Io.status(), 999, 0);
    let Ok(Event::InstallDone(end)) = decode(&bytes) else {
        panic!("a typed outcome");
    };
    assert_eq!(end.kind(), InstallKind::Unknown(77));
    assert_eq!(end.outcome(), DeviceOutcome::Other);
}

#[test]
fn a_library_hint_for_several_books_has_an_empty_path() {
    let record = Record::EventLibraryChanged(wire::EventLibraryChanged {
        now_ms: 1,
        what: 1 << 5,
        generation: 0,
        path: "",
    });
    assert!(matches!(
        decode(&encode(record)),
        Ok(Event::LibraryChanged { path: "", .. })
    ));
}

#[test]
fn a_library_hint_is_typed_keeps_future_bits_and_borrows_its_path() {
    let record = Record::EventLibraryChanged(wire::EventLibraryChanged {
        now_ms: 42,
        what: 0b100_0011 | (1 << 20),
        generation: 9,
        path: "books/a.epub",
    });
    let bytes = encode(record);
    let Ok(Event::LibraryChanged {
        now_ms,
        what,
        generation,
        path,
    }) = decode(&bytes)
    else {
        panic!("a typed library hint");
    };
    assert_eq!((now_ms, generation, path), (42, 9, "books/a.epub"));
    assert_eq!(what.bits(), 0b100_0011 | (1 << 20), "future bits are kept");
    assert!(what.books() && what.status() && what.cover());
    assert!(!(what.positions() || what.annotations() || what.fields() || what.walk()));
    assert_borrowed_text(&bytes, path);
}

#[test]
fn the_book_events_carry_chapter_left_with_the_same_bounds_as_a_page_turn() {
    let mut bytes = vec![0; 4096];
    for (left, ok) in [
        (None, true),
        (Some(0), true),
        (Some(1), true),
        (Some(2), false),
    ] {
        // Page 1 of 2: one page follows.
        let opened = wire::EventBookOpened {
            chapter_left: left,
            ..position!(EventBookOpened, 500_000, Some(1), Some(2), 0)
        };
        let closed = wire::EventBookClosed {
            chapter_left: left,
            ..position!(EventBookClosed, 500_000, Some(1), Some(2), 0)
        };
        let finished = wire::EventBookFinished {
            chapter_left: left,
            ..position!(EventBookFinished, 500_000, Some(1), Some(2), 0)
        };
        for record in [
            Record::EventBookOpened(opened),
            Record::EventBookClosed(closed),
            Record::EventBookFinished(finished),
        ] {
            let n = wire::records_encode(&[record], &mut bytes).unwrap();
            match (decode(&bytes[..n]), ok) {
                (Ok(Event::Book { reading, .. }), true) => assert_eq!(reading.chapter_left(), left),
                (Err(Error::Invalid), false) => {}
                (other, _) => panic!("{left:?}: {other:?}"),
            }
        }
    }
}
