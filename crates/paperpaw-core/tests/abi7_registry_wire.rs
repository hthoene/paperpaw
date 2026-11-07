//! Registry (REG) wire records (`docs/abi/registry.md`): the slot-15 commands
//! 1536-1546 generated into the command codec, the declarations 21-28, the
//! events 75-78 and the `core.registry` rows 1025-1040 in the record codec,
//! and how the host classifies them before Core wires them.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::commands::{WireCommand, commands_decode, commands_encode};
use paperpaw_core::native_host::declarations::validate_record;
use paperpaw_core::native_host::registrations::{
    EventPluginsRequested, EventRegistryChanged, KeyBind, PresetValue, Record, ServiceRegMenuRow,
    ServiceRegQuery, SetupStep, SlotOffer, record_decode_one, record_encode_one, records_decode,
    records_encode,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_UNSUPPORTED, ERR_UTF8};

fn every_command() -> Vec<WireCommand<'static>> {
    vec![
        WireCommand::ResetSetting {
            key: "reader.font",
            flags: 1,
        },
        WireCommand::PreviewSetting {
            key: "reader.size",
            value: &[2, 18, 0, 0, 0, 0, 0, 0, 0],
            flags: 0,
        },
        WireCommand::PreviewEnd { flags: 0 },
        WireCommand::SetScope {
            path: "books/a.epub",
        },
        WireCommand::SetMenu {
            menu: "core.main",
            items: "library.open\nsettings.open",
            flags: 0,
        },
        WireCommand::BindKey {
            layer: "global",
            input: "button.menu",
            action: "core.menu",
            flags: 0,
        },
        WireCommand::BindSlot {
            slot: "core.home",
            screen: "library.shelf",
        },
        WireCommand::SetPluginEnabled {
            plugin: "weather",
            enabled: 0,
        },
        WireCommand::RequestPlugins {
            plugins: "weather\nclock",
        },
        WireCommand::AnswerPlugin {
            request: 7,
            plugin: "weather",
            outcome: 1,
        },
        WireCommand::ApplyPreset {
            preset: "core.power-mode.saver",
            flags: 1,
        },
    ]
}

fn encode(commands: &[WireCommand<'_>]) -> Vec<u8> {
    let mut out = vec![0; 4096];
    let len = commands_encode(commands, &mut out).unwrap();
    out.truncate(len);
    out
}

#[test]
fn every_registry_command_round_trips_with_its_tag() {
    let tags = 1536..=1546;
    for (command, tag) in every_command().into_iter().zip(tags) {
        let bytes = encode(&[command]);
        // Count 1, then the record tag as two LEB128 bytes.
        assert_eq!(u32::from(bytes[1] & 0x7f) | (u32::from(bytes[2]) << 7), tag);
        let batch = commands_decode(&bytes).unwrap();
        assert_eq!(batch.iter().collect::<Vec<_>>(), [command]);
    }
    // Eight in one batch, as any other commands.
    let eight = &every_command()[..8];
    let bytes = encode(eight);
    let batch = commands_decode(&bytes).unwrap();
    assert_eq!(batch.iter().collect::<Vec<_>>(), eight);
}

#[test]
fn caps_text_and_framing_keep_the_codec_statuses() {
    let long = "k".repeat(129);
    let too_long = WireCommand::ResetSetting {
        key: &long,
        flags: 0,
    };
    let mut out = vec![0; 4096];
    assert_eq!(commands_encode(&[too_long], &mut out), Err(ERR_NO_SPACE));
    // The same record on the wire: tag 1536, bitmap, key of 129 bytes, flags.
    let mut body = vec![0, 129, 1];
    body.extend(long.as_bytes());
    body.extend([0; 4]);
    // Count 1, tag 1536, then the body length (136) as two LEB128 bytes.
    let mut wire = vec![1, 0x80, 0x0c, 0x88, 0x01];
    assert_eq!(body.len(), 136);
    wire.extend(&body);
    assert_eq!(commands_decode(&wire).map(|_| ()), Err(ERR_NO_SPACE));
    // Bad UTF-8 in text is ERR_UTF8; a cut body is ERR_INVALID.
    let mut bytes = encode(&[WireCommand::SetScope { path: "ab" }]);
    let last = bytes.len() - 1;
    bytes[last] = 0xff;
    assert_eq!(commands_decode(&bytes).map(|_| ()), Err(ERR_UTF8));
    let bytes = encode(&[WireCommand::AnswerPlugin {
        request: 1,
        plugin: "a",
        outcome: 0,
    }]);
    let mut cut = bytes[..bytes.len() - 1].to_vec();
    cut[3] -= 1;
    assert_eq!(commands_decode(&cut).map(|_| ()), Err(ERR_INVALID));
    // Empty text is allowed by the codec; the host checks what it means.
    let empty = WireCommand::BindSlot {
        slot: "core.home",
        screen: "",
    };
    assert_eq!(
        commands_decode(&encode(&[empty])).unwrap().iter().next(),
        Some(empty)
    );
    // The next tag in the growth block has no command yet.
    assert_eq!(
        commands_decode(&[1, 0x8b, 0x0c, 1, 0]).map(|_| ()),
        Err(ERR_UNSUPPORTED)
    );
}

fn leb128(mut value: usize, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// One command on the wire, built by hand so the decoder sees what the
/// encoder refuses: count 1, tag, body length, empty bitmap, text fields.
fn raw(tag: usize, texts: &[&str], scalars: usize) -> Vec<u8> {
    let mut body = vec![0];
    for text in texts {
        leb128(text.len(), &mut body);
        body.extend(text.as_bytes());
    }
    body.extend(vec![0; scalars]);
    let mut wire = vec![1];
    leb128(tag, &mut wire);
    leb128(body.len(), &mut wire);
    wire.extend(body);
    wire
}

fn items(count: usize, width: usize) -> String {
    (0..count)
        .map(|index| format!("{index:0width$}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Encoder and decoder refuse the same lists: too many items, an item over its
/// cap, an empty item or a trailing separator are ERR_INVALID; a list over the
/// field's byte cap is ERR_NO_SPACE; no items is fine.
#[test]
fn list_fields_enforce_item_count_length_and_empty_items() {
    fn menu(items: &str) -> WireCommand<'_> {
        WireCommand::SetMenu {
            menu: "core.main",
            items,
            flags: 0,
        }
    }
    fn plugins(plugins: &str) -> WireCommand<'_> {
        WireCommand::RequestPlugins { plugins }
    }
    let cases = [
        (items(64, 2), items(16, 2), Ok(())),
        (items(65, 2), items(17, 2), Err(ERR_INVALID)),
        (items(1, 128), items(1, 64), Ok(())),
        (items(1, 129), items(1, 65), Err(ERR_INVALID)),
        ("a\n\nb".into(), "a\n\nb".into(), Err(ERR_INVALID)),
        ("a\n".into(), "a\n".into(), Err(ERR_INVALID)),
        ("\n".into(), "\n".into(), Err(ERR_INVALID)),
        (String::new(), String::new(), Ok(())),
        // Over the field's byte cap is ERR_NO_SPACE before any item rule, on both sides.
        (items(65, 32), items(17, 64), Err(ERR_NO_SPACE)),
    ];
    let mut out = vec![0; 4096];
    for (menu_items, plugin_ids, expected) in cases {
        for (command, wire) in [
            (menu(&menu_items), raw(1540, &["core.main", &menu_items], 4)),
            (plugins(&plugin_ids), raw(1544, &[&plugin_ids], 0)),
        ] {
            let encoded = commands_encode(&[command], &mut out);
            assert_eq!(encoded.map(|_| ()), expected, "{command:?}");
            let decoded = commands_decode(&wire).map(|batch| batch.iter().next());
            assert_eq!(decoded, expected.map(|()| Some(command)), "{command:?}");
        }
    }
}

/// `answer_plugin` waits for DEV's `uses.install` (`registry.md` section 5);
/// until then the host refuses it from every plugin (`abi7_scoped_commands`).
/// When DEV adds the feature, this fails: wire the check to it there.
#[test]
fn answer_plugin_has_no_install_feature_yet() {
    let schema = include_str!("../../../abi/abi.def");
    assert!(schema.contains("\"key\": \"uses.registry\""));
    assert!(!schema.contains("\"key\": \"uses.install\""));
}

const STEP: Record<'static> = Record::SetupStep(SetupStep {
    name: "wifi",
    title: "setup.wifi",
    help: "setup.wifi.help",
    order: 2,
    page_kind: 1,
    page: "wifi.networks",
    applies_key: "",
    applies_value: "",
    need: 1,
    capability: 4,
    flags: 1,
});
const OFFER: Record<'static> = Record::SlotOffer(SlotOffer {
    slot: "core.home",
    screen: "shelf",
    priority: -3,
});
const BIND: Record<'static> = Record::KeyBind(KeyBind {
    layer: "global",
    input: "button.menu",
    action: "core.menu",
    when_key: "reader.mode",
    when_value: "night",
});
const PRESET: Record<'static> = Record::PresetValue(PresetValue {
    preset: "saver",
    key: "core.refresh",
    value: &[2, 6, 0, 0, 0, 0, 0, 0, 0],
    flags: 0,
});

#[test]
fn registry_declarations_round_trip_and_wait_for_the_host() {
    let records = [STEP, OFFER, BIND, PRESET];
    let mut out = vec![0; 4096];
    let len = records_encode(&records, &mut out).unwrap();
    let batch = records_decode(&out[..len]).unwrap();
    assert_eq!(batch.iter().collect::<Vec<_>>(), records);
    // Refused until the host admits them (Core hand-off).
    for record in records {
        assert_eq!(validate_record("demo", &record), Err(ERR_UNSUPPORTED));
    }
}

#[test]
fn registry_events_and_rows_round_trip_and_are_never_submitted() {
    let rows = [
        Record::EventRegistryChanged(EventRegistryChanged {
            now_ms: 5,
            what: 1 | 1 << 31,
        }),
        Record::EventPluginsRequested(EventPluginsRequested {
            now_ms: 6,
            request: 2,
            by: "store",
            plugins: "weather\nclock",
        }),
        Record::ServiceRegQuery(ServiceRegQuery {
            scope: "core.main",
            cursor: 0,
            generation: 0,
            flags: 1,
        }),
        Record::ServiceRegMenuRow(ServiceRegMenuRow {
            item: "library.open",
            kind: 0,
            flags: 0b10_0010,
            order: -1,
            by: "library",
            title: "Bibliothek",
            icon: "book",
            badge: "3",
            describe: "",
        }),
    ];
    for row in rows {
        let mut out = vec![0; 4096];
        let len = record_encode_one(&row, &mut out).unwrap();
        assert_eq!(record_decode_one(&out[..len]), Ok(row));
        assert_eq!(validate_record("demo", &row), Err(ERR_INVALID));
    }
}
