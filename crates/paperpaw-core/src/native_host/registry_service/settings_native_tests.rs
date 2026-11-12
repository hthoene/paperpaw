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
    Id, Manifest, Plugin, RegisterError, Registrar, SettingGroupSpec, SettingSpec, SettingValue,
    Version,
};

/// `reg_setting.flags` bits.
const SHOWN: u32 = 1 << 9;
const GOVERNS: u32 = 1 << 10;
const SLIDER: u32 = 1 << 11;

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

fn extra(
    name: &'static str,
    editor_kind: u32,
    editor: &'static str,
    shown_id: &'static str,
) -> Record<'static> {
    Record::SettingExtra(wire::SettingExtra {
        name,
        unit: "nat.unit.pt",
        section: "nat.section.look",
        follows: "nat.flag",
        shown_id,
        shown_value: if shown_id.is_empty() { "" } else { "on" },
        editor_kind,
        editor,
        step: 2,
        flags: 1,
        check: 0,
    })
}

/// The store: `nat.size` a slider with every field, shown while `nat.flag`
/// is on; `nat.flag` a switch that asks for a slider (an int's alone);
/// `nat.plain` nothing.
fn store() -> NativeDeclarations {
    declarations(&[
        extra("size", 1, "edit", "nat.flag"),
        extra("flag", 0, "", ""),
    ])
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

/// `reg_setting.flags` of `key` in op 1's list of `nat`.
fn flags(kernel: &crate::Kernel, directory: Option<&NativeDeclarations>, key: &str) -> u32 {
    let query = query_flags("nat", 0, 0, 0);
    let reply = call_in(kernel, directory, schema::OP_SETTINGS, &query, 4096).unwrap();
    let prefix = format!("{key} ");
    let row = rows(&reply)
        .into_iter()
        .find(|r| r.starts_with(&prefix))
        .unwrap();
    row[prefix.len()..].parse().unwrap()
}

fn slider(directory: Option<&NativeDeclarations>, key: &str) -> bool {
    flags(&kernel(), directory, key) & SLIDER != 0
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

#[test]
fn a_native_setting_shows_while_its_governing_setting_has_the_value() {
    let store = store();
    let mut kernel = kernel();
    let of = |kernel: &crate::Kernel, key: &str| flags(kernel, Some(&store), key);
    assert_eq!(of(&kernel, "nat.size") & SHOWN, 0, "nat.flag is off");
    assert_ne!(of(&kernel, "nat.flag") & GOVERNS, 0, "the page asks again");
    assert_eq!(of(&kernel, "nat.plain") & (SHOWN | GOVERNS), SHOWN);
    kernel
        .shared
        .settings
        .set("nat.flag", SettingValue::Bool(true))
        .unwrap();
    assert_ne!(of(&kernel, "nat.size") & SHOWN, 0, "nat.flag is on");
    // One row alone (op 2) finds what governs it the same way.
    let reply = call_in(
        &kernel,
        Some(&store),
        schema::OP_SETTING,
        &key("nat.flag"),
        4096,
    )
    .unwrap();
    let row = rows(&reply).remove(0);
    let bits: u32 = row.split(' ').nth(1).unwrap().parse().unwrap();
    assert_ne!(bits & GOVERNS, 0);
    // Without the store nothing is declared: shown, governing nothing.
    assert_ne!(flags(&kernel, None, "nat.size") & SHOWN, 0);
    assert_eq!(flags(&kernel, None, "nat.flag") & GOVERNS, 0);
}
