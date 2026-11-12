//! A native plugin's `setting.extra` in `core.registry` ops 1 and 2 (todo
//! 541): its spec keeps no extra, the record is read from the declaration
//! store when a row is asked for.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
extern crate std;

use super::{
    menus_tests::declarations,
    schema,
    tests::{call_in, key, query_flags},
};
use crate::native_host::{
    native_declarations::NativeDeclarations,
    registrations::{self as wire, Record},
};
use alloc::{format, string::String, vec::Vec};
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::{
    Id, Manifest, Plugin, RegisterError, Registrar, SettingGroupSpec, SettingSpec, Version,
};

/// The settings a native plugin's host makes from its spec records: no extra.
struct Nat;
impl Plugin for Nat {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("nat"), "Nat", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let top = Id::new_static("nat.top");
        r.add_setting_group(SettingGroupSpec::new(top.clone(), "nat.group.top"))?;
        r.add_setting(SettingSpec::number(
            "nat.size",
            "nat.title.size",
            8,
            40,
            12,
            top.clone(),
        ))?;
        r.add_setting(SettingSpec::number(
            "nat.plain",
            "nat.title.plain",
            0,
            9,
            1,
            top.clone(),
        ))?;
        r.add_setting(SettingSpec::switch(
            "nat.flag",
            "nat.title.flag",
            false,
            top,
        ))?;
        Ok(())
    }
}

fn kernel() -> crate::Kernel {
    crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Nat)
        .build()
        .unwrap()
}

fn extra(name: &'static str, editor_kind: u32, editor: &'static str) -> Record<'static> {
    Record::SettingExtra(wire::SettingExtra {
        name,
        unit: "nat.unit.pt",
        section: "nat.section.look",
        follows: "nat.flag",
        shown_id: "nat.flag",
        shown_value: "on",
        editor_kind,
        editor,
        step: 2,
        flags: 1,
        check: 0,
    })
}

/// The store: `nat.size` a slider with every field, `nat.flag` a switch
/// that asks for a slider (an int's alone), `nat.plain` nothing.
fn store() -> NativeDeclarations {
    declarations(&[extra("size", 1, "edit"), extra("flag", 0, "")])
}

/// `key flags` of op 1's rows, `extra …` of op 2's.
fn rows(reply: &[u8]) -> Vec<String> {
    let mut rows = Vec::new();
    let mut rest = reply;
    while !rest.is_empty() {
        let (record, used) = wire::record_decode_prefix(rest).unwrap();
        rest = &rest[used..];
        match record {
            Record::ServiceRegSetting(r) => rows.push(format!("{} {}", r.key, r.flags)),
            Record::ServiceRegSettingExtra(r) => rows.push(format!(
                "extra {}..{}/{} unit={} section={} follows={} shown={}={} editor={}:{}",
                r.min,
                r.max,
                r.step,
                r.unit,
                r.section,
                r.follows,
                r.shown_id,
                r.shown_value,
                r.editor_kind,
                r.editor,
            )),
            _ => {}
        }
    }
    rows
}

fn slider(directory: Option<&NativeDeclarations>, key: &str) -> bool {
    let kernel = kernel();
    let query = query_flags("nat", 0, 0, 0);
    let reply = call_in(&kernel, directory, schema::OP_SETTINGS, &query, 4096).unwrap();
    let prefix = format!("{key} ");
    let row = rows(&reply)
        .into_iter()
        .find(|r| r.starts_with(&prefix))
        .unwrap();
    row[prefix.len()..].parse::<u32>().unwrap() & (1 << 11) != 0
}

fn detail(directory: Option<&NativeDeclarations>, setting: &str) -> Result<String, i32> {
    let kernel = kernel();
    let reply = call_in(&kernel, directory, schema::OP_SETTING, &key(setting), 4096)?;
    Ok(rows(&reply).remove(1))
}

#[test]
fn a_native_int_setting_that_declares_a_slider_has_the_flag() {
    let store = store();
    assert!(slider(Some(&store), "nat.size"));
    assert!(!slider(Some(&store), "nat.flag"), "an int's alone");
    assert!(!slider(Some(&store), "nat.plain"), "nothing declared");
    assert!(!slider(None, "nat.size"), "no store, no record");
}

#[test]
fn op_two_shows_what_a_native_plugin_declared() {
    let store = store();
    assert_eq!(
        detail(Some(&store), "nat.size").unwrap(),
        "extra 8..40/2 unit=nat.unit.pt section=nat.section.look follows=nat.flag \
         shown=nat.flag=on editor=1:nat.edit"
    );
    assert_eq!(
        detail(Some(&store), "nat.plain").unwrap(),
        "extra 0..9/1 unit= section= follows= shown== editor=0:"
    );
    // A store being changed: asked again later.
    let _held = store.store.borrow_mut();
    assert_eq!(detail(Some(&store), "nat.size"), Err(ERR_BUSY));
}
