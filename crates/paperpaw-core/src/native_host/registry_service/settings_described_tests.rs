//! `core.registry` ops 1 to 3 over settings a plugin describes from its
//! image (#248): the lists and details of the widget pack are those of its
//! table, nothing of them stays made, and no room is told as no room.
//!
//! Linux x86_64 only, as the fixture is (`described_settings::tests`).
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
extern crate std;

use super::{
    schema,
    tests::{call_failing, call_in, key, query_flags},
};
use crate::native_host::described_settings::tests::{held, kernel, no_room_for_a_spec};
use crate::native_host::registrations::{self as wire, Record};
use crate::native_host::shell::tests::FAIL_SIZE;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NOT_FOUND};
use paperpaw_plugin_widgets::{NATIVE_GROUPS, NATIVE_SETTINGS};

/// Every page of op 1's list of `scope`, as the bytes of each reply.
fn pages(kernel: &crate::Kernel, scope: &str) -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    let (mut cursor, mut generation) = (0, 0);
    loop {
        let query = query_flags(scope, cursor, generation, 0);
        let reply = call_in(kernel, None, schema::OP_SETTINGS, &query, 4096).unwrap();
        let mut end = None;
        let mut rest = &reply[..];
        while !rest.is_empty() {
            let (record, used) = wire::record_decode_prefix(rest).unwrap();
            rest = &rest[used..];
            if let Record::ServiceEnd(found) = record {
                end = Some((found.flags, found.next, found.generation));
            }
        }
        all.push(reply);
        let (flags, next, of) = end.unwrap();
        if flags & 1 != 0 {
            return all;
        }
        (cursor, generation) = (next, of);
    }
}

/// How many records of `pages` are no page's end.
fn rows(pages: &[Vec<u8>]) -> usize {
    let mut rows = 0;
    for page in pages {
        let mut rest = &page[..];
        while !rest.is_empty() {
            let (record, used) = wire::record_decode_prefix(rest).unwrap();
            rest = &rest[used..];
            rows += usize::from(!matches!(record, Record::ServiceEnd(_)));
        }
    }
    rows
}

#[test]
fn the_lists_and_details_of_described_settings_are_the_table_s() {
    let (described, table) = (kernel(false), kernel(true));
    let lazy = &described.shared.settings;
    let group = NATIVE_GROUPS[0].id.as_str();
    // The plugin's own list, a group's and the list of every setting.
    for scope in ["widgets", group, ""] {
        let listed = pages(&described, scope);
        assert_eq!(listed, pages(&table, scope), "{scope}");
        // Every one of the pack's settings is a row of its list at least.
        let least = if scope == group {
            2
        } else {
            NATIVE_SETTINGS.len()
        };
        assert!(rows(&listed) >= least, "{scope}: {}", rows(&listed));
        // A list keeps none of the specs it read.
        assert!(!lazy.any_made(), "{scope}");
    }
    assert_eq!(held(lazy), 0);
    for spec in NATIVE_SETTINGS {
        let id = spec.id.as_str();
        for op in [schema::OP_SETTING, schema::OP_CHOICES] {
            let input = match op {
                schema::OP_SETTING => key(id),
                _ => query_flags(id, 0, 0, 0),
            };
            assert_eq!(
                call_in(&described, None, op, &input, 4096),
                call_in(&table, None, op, &input, 4096),
                "{id}"
            );
        }
    }
}

#[test]
fn a_list_without_room_for_a_spec_says_so_and_is_never_a_short_list() {
    let described = kernel(false);
    let query = query_flags("widgets", 0, 0, 0);
    let whole = call_in(&described, None, schema::OP_SETTINGS, &query, 4096).unwrap();
    let mut refused = 0;
    for nth in 1..4096 {
        let (reply, fired) = call_failing(&described, None, schema::OP_SETTINGS, &query, 4096, nth);
        if !fired {
            assert_eq!(reply.as_ref(), Ok(&whole));
            break;
        }
        match reply {
            Err(status) => {
                assert_eq!(status, ERR_NO_MEMORY, "allocation {nth}");
                refused += 1;
            }
            // Only what is no spec (the list of governing ids) may be done
            // without: the rows are all there then.
            Ok(reply) => assert_eq!(reply, whole, "allocation {nth}"),
        }
    }
    assert!(refused > 0, "the list made no spec");
}

#[test]
fn a_setting_asked_for_without_room_for_its_spec_is_not_one_not_found() {
    let mut described = kernel(false);
    let choice = NATIVE_SETTINGS
        .iter()
        .find(|spec| matches!(spec.kind, paperpaw_sdk::SettingKind::Choice(_)))
        .unwrap();
    let id = choice.id.as_str();
    for (op, input) in [
        (schema::OP_SETTING, key(id)),
        (schema::OP_CHOICES, query_flags(id, 0, 0, 0)),
    ] {
        let whole = call_in(&described, None, op, &input, 4096).unwrap();
        described.shared.settings.rest();
        no_room_for_a_spec();
        let refused = call_in(&described, None, op, &input, 4096);
        assert_eq!(FAIL_SIZE.with(|size| size.replace(0)), 0, "op {op}");
        assert_eq!(refused, Err(ERR_NO_MEMORY), "op {op}");
        assert_eq!(call_in(&described, None, op, &input, 4096), Ok(whole));
    }
    let none = call_in(
        &described,
        None,
        schema::OP_SETTING,
        &key("widgets.none"),
        4096,
    );
    assert_eq!(none, Err(ERR_NOT_FOUND));
}
