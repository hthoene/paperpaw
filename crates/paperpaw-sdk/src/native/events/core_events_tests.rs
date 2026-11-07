#![allow(clippy::unwrap_used, reason = "invalid fixtures must fail tests")]
use super::*;
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
fn borrow(bytes: &[u8], text: &str) {
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
                borrow(&bytes, key);
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
                borrow(&bytes, reading.path());
                borrow(&bytes, reading.title());
                borrow(&bytes, reading.chapter());
            }
            Event::BookMoved { now_ms, from, path } => {
                assert_eq!((now_ms, from, path), (42, "old.epub", "new.epub"));
                borrow(&bytes, from);
                borrow(&bytes, path);
            }
            Event::BookRemoved { now_ms, path } => {
                assert_eq!((now_ms, path), (42, "old.epub"));
                borrow(&bytes, path);
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
                borrow(&bytes, power.mode());
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
            borrow(&bytes, text);
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
    borrow(&bytes, reading.path());
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
