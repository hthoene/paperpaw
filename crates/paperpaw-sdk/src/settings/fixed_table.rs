//! The settings of a plugin as a `static` table, made from its declaration
//! batches when the plugin is built.
//!
//! A linked (built-in) plugin declares its settings as wire records, and the
//! kernel would make one heap-owned `SettingSpec` of each. Beta 3 kept them in
//! flash instead (`Registrar::add_settings`): this makes the same table from
//! the very records the plugin declares, in a build script, so a plugin keeps
//! one source of truth. The kernel takes a spec from the table only after it
//! equals the one the record makes; any other spec is made as before.

use crate::native::declare::records_wire::{
    Record, SETTING_SPEC_CHOICES_SEPARATOR, records_decode,
};
use crate::native::declare::settings_value::{SettingValueRef, settings_decode};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

const SCOPED: u32 = 1;
const GUARDED: u32 = 1 << 1;
const KEPT: u32 = 1 << 2;
const SECRET: u32 = 1 << 3;
const ADVANCED: u32 = 1 << 4;

/// Rust source of `pub static <name>: [SettingSpec; N]`: the settings the
/// `batches` of `plugin` declare, in order, as `SettingSpec` values the kernel
/// takes in place of its own copies. Meant for a build script; the generated
/// code names `paperpaw_sdk::{Id, SettingSpec}`.
///
/// A secret setting is left out (the kernel refuses those from a plugin).
///
/// # Errors
/// A batch that does not decode, or a setting whose default does not fit its
/// kind.
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn fixed_settings_source(
    plugin: &str,
    name: &str,
    batches: &[&[u8]],
) -> Result<String, String> {
    let mut rows = String::new();
    let mut count = 0usize;
    for bytes in batches {
        let batch = records_decode(bytes).map_err(|code| format!("batch: error {code}"))?;
        for record in batch.iter() {
            let Record::SettingSpec(spec) = record else {
                continue;
            };
            if spec.flags & SECRET != 0 {
                continue;
            }
            let id = format!("{plugin}.{}", spec.name);
            let default = settings_decode(spec.default)
                .map_err(|code| format!("{id}: default error {code}"))?;
            let group = format!("Id::new_static({:?})", spec.group);
            let head = match (spec.kind, default) {
                (0, SettingValueRef::Bool(value)) => format!(
                    "SettingSpec::switch({id:?}, {:?}, {value}, {group})",
                    spec.title
                ),
                (1, SettingValueRef::Int(value)) => format!(
                    "SettingSpec::number({id:?}, {:?}, {}, {}, {value}, {group})",
                    spec.title, spec.min, spec.max
                ),
                (2, SettingValueRef::Text(value)) => {
                    let options: Vec<&str> =
                        spec.choices.split(SETTING_SPEC_CHOICES_SEPARATOR).collect();
                    format!(
                        "SettingSpec::choice({id:?}, {:?}, &{options:?}, {value:?}, {group})",
                        spec.title
                    )
                }
                (3..=5, SettingValueRef::Text(value)) => {
                    let constructor = match spec.kind {
                        3 => "text",
                        4 => "language",
                        _ => "list",
                    };
                    format!(
                        "SettingSpec::{constructor}({id:?}, {:?}, {value:?}, {group})",
                        spec.title
                    )
                }
                _ => return Err(format!("{id}: default does not fit kind {}", spec.kind)),
            };
            let mut row = head;
            if !spec.help.is_empty() {
                let _ = write!(row, ".description({:?})", spec.help);
            }
            for (flag, method) in [
                (ADVANCED, "advanced"),
                (SCOPED, "scoped"),
                (GUARDED, "guarded"),
                (KEPT, "kept_on_reset"),
            ] {
                if spec.flags & flag != 0 {
                    let _ = write!(row, ".{method}()");
                }
            }
            let _ = writeln!(rows, "    {row},");
            count += 1;
        }
    }
    Ok(format!(
        "pub static {name}: [SettingSpec; {count}] = [\n{rows}];\n"
    ))
}

/// Rust source of `pub static <name>: [paperpaw_sdk::SettingGroupSpec; N]`: the
/// setting groups the `batches` of `plugin` declare, in order, which the
/// kernel takes in place of its own copies like the settings of
/// [`fixed_settings_source`]. Meant for a build script; the generated code
/// names `paperpaw_sdk::{Id, SettingGroupSpec}` in full.
///
/// # Errors
/// A batch that does not decode.
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn fixed_groups_source(plugin: &str, name: &str, batches: &[&[u8]]) -> Result<String, String> {
    let mut rows = String::new();
    let mut count = 0usize;
    for bytes in batches {
        let batch = records_decode(bytes).map_err(|code| format!("batch: error {code}"))?;
        for record in batch.iter() {
            // Every other record of the batches is skipped on purpose: a
            // batch holds the plugin's screens, actions and settings too.
            let Record::SettingGroup(group) = record else {
                continue;
            };
            let id = |text: &str| format!("paperpaw_sdk::Id::new_static({text:?})");
            let mut row = format!(
                "paperpaw_sdk::SettingGroupSpec::new({}, {:?}).order({})",
                id(&format!("{plugin}.{}", group.name)),
                group.title,
                group.order
            );
            if !group.page.is_empty() {
                let _ = write!(row, ".page({})", id(&format!("{plugin}.{}", group.page)));
            }
            if !group.parent.is_empty() {
                let _ = write!(row, ".within({})", id(group.parent));
            }
            let _ = writeln!(rows, "    {row},");
            count += 1;
        }
    }
    Ok(format!(
        "pub static {name}: [paperpaw_sdk::SettingGroupSpec; {count}] = [\n{rows}];\n"
    ))
}
