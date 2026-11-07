#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::abi7::context::{DeviceUsageView, RowsEnd};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use paperpaw_i18n::Translator;
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Settings};

struct View<'a> {
    env: &'a Env<'a>,
    rows: &'a [PluginRow<'a>],
    name: &'a str,
}

impl Frame for View<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn device_usage(&self) -> Result<DeviceUsageView, i32> {
        Ok(DeviceUsageView {
            fast: 2,
            partial: 3,
            full: 4,
            pixels: 12500,
            inputs: 6,
            sleeps: 1,
            panel_ms: 700,
            bluetooth_ms: 900,
        })
    }
    fn plugin_rows(
        &self,
        from: u32,
        visit: &mut dyn FnMut(&PluginRow<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        let total = self.rows.len() as u32;
        let mut next = from.min(total);
        for row in self.rows.iter().skip(next as usize) {
            if visit(row).is_break() {
                break;
            }
            next += 1;
        }
        Ok(RowsEnd {
            total,
            generation: 17,
            next,
        })
    }
    fn plugin_name(&self, id: &str, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        if id != "fixture" {
            return Err(ERR_NOT_FOUND);
        }
        visit(self.name);
        Ok(())
    }
    fn work_note(&self, id: &str, visit: &mut dyn FnMut(u64, &str)) -> Result<(), i32> {
        if id != "fixture" {
            return Err(ERR_NOT_FOUND);
        }
        visit(19, "working");
        Ok(())
    }
}

fn row<'a>(plugin: &'a str, reason: &'a str) -> PluginRow<'a> {
    PluginRow {
        plugin,
        busy_us: u64::MAX,
        calls: 3,
        shown_ms: u32::MAX,
        fast: 5,
        partial: 6,
        full: 7,
        pixels: u64::MAX - 1,
        read: u32::MAX,
        written: 10,
        syncs: 11,
        requests: 12,
        refused: 13,
        awake_ms: u32::MAX,
        awake_reason: reason,
    }
}

#[test]
fn key_grammar_and_argument_statuses_are_exact() {
    assert_eq!(Query::parse("core.usage"), Ok(Query::Usage));
    assert_eq!(Query::parse("core.plugin-usage"), Ok(Query::PluginUsage(0)));
    assert_eq!(
        Query::parse("core.plugin-usage/from.4294967295"),
        Ok(Query::PluginUsage(u32::MAX))
    );
    assert_eq!(
        Query::parse("core.plugin/fixture").unwrap().base(),
        "core.plugin"
    );
    for key in [
        "",
        "core",
        "Core.usage",
        "core..usage",
        "core.usage/x",
        "core.plugin",
        "core.plugin/Bad",
        "core.work-note/",
        "core.plugin-usage/from.01",
        "core.plugin-usage/from.-1",
        "core.plugin-usage/from.4294967296",
        "core.plugin-usage/from.1/extra",
        "core.plugin-usage/from.１",
    ] {
        assert_eq!(Query::parse(key), Err(ERR_INVALID), "{key}");
    }
    assert_eq!(Query::parse("other.service"), Err(ERR_NOT_FOUND));
    assert_eq!(Query::parse(&"a".repeat(129)), Err(ERR_INVALID));
}

#[test]
fn usage_matches_the_existing_battery_binary_fixture() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let view = View {
        env: &env,
        rows: &[],
        name: "fixture",
    };
    let mut out = [77; 384];
    let length = encode(&view, Query::Usage, &mut out).unwrap();
    let expected =
        include_bytes!("../../../../paperpaw-sdk/tests/fixtures/abi7-services/usage.bin");
    assert_eq!(&out[..length], &expected[..]);
    assert!(out[length..].iter().all(|byte| *byte == 77));
    assert_eq!(length, 47);
}

#[test]
fn scalar_names_are_utf8_clipped_and_notes_and_missing_ids_are_preserved() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let name = "é".repeat(80);
    let view = View {
        env: &env,
        rows: &[],
        name: &name,
    };
    let mut out = [0; 384];
    let length = encode(&view, Query::Plugin("fixture"), &mut out).unwrap();
    assert!(
        matches!(wire::record_decode_one(&out[..length]), Ok(wire::Record::ServicePlugin(value)) if value.name == "é".repeat(64))
    );
    let length = encode(&view, Query::WorkNote("fixture"), &mut out).unwrap();
    assert!(
        matches!(wire::record_decode_one(&out[..length]), Ok(wire::Record::ServiceWorkNote(value)) if value.at_ms == 19 && value.text == "working")
    );
    assert_eq!(
        encode(&view, Query::Plugin("missing"), &mut out),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!(
        encode(&view, Query::WorkNote("missing"), &mut out),
        Err(ERR_NOT_FOUND)
    );
}

fn records(bytes: &[u8]) -> Vec<wire::Record<'_>> {
    let mut out = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let (record, used) = wire::record_decode_prefix(rest).unwrap();
        out.push(record);
        rest = &rest[used..];
    }
    out
}

#[test]
fn pages_reserve_the_end_record_widen_counters_and_advance_over_omitted_ids() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let ids: Vec<String> = [40, 65, 40, 40]
        .iter()
        .enumerate()
        .map(|(n, length)| char::from(b'a' + n as u8).to_string().repeat(*length))
        .collect();
    let reason = "é".repeat(40);
    let rows: Vec<_> = ids.iter().map(|id| row(id, &reason)).collect();
    let view = View {
        env: &env,
        rows: &rows,
        name: "fixture",
    };
    let mut out = [0; 384];
    let length = encode(&view, Query::PluginUsage(0), &mut out).unwrap();
    let decoded = records(&out[..length]);
    let wire::Record::ServicePluginUsage(first) = decoded[0] else {
        panic!("row");
    };
    assert_eq!(
        (first.shown_ms, first.read, first.awake_ms),
        (
            u64::from(u32::MAX),
            u64::from(u32::MAX),
            u64::from(u32::MAX)
        )
    );
    assert_eq!(first.awake_reason, "é".repeat(32));
    let wire::Record::ServiceEnd(end) = decoded[decoded.len() - 1] else {
        panic!("end");
    };
    assert_eq!(
        (end.flags, end.total, end.generation, end.next),
        (3, 4, 17, 2)
    );
    let length = encode(&view, Query::PluginUsage(end.next), &mut out).unwrap();
    assert!(
        matches!(records(&out[..length])[0], wire::Record::ServicePluginUsage(value) if value.plugin == ids[2])
    );
    let length = encode(&view, Query::PluginUsage(u32::MAX), &mut out).unwrap();
    assert!(
        matches!(wire::record_decode_one(&out[..length]), Ok(wire::Record::ServiceEnd(value)) if value.flags == 0 && value.next == 4)
    );
}
