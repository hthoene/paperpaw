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

/// Rust source of `pub static <name>: [paperpaw_sdk::Id; N]`: every full id
/// the `batches` of `plugin` name where the kernel makes an id of a record
/// (the plugin's own id; its screens, actions, settings and setting groups
/// as `<plugin>.<name>`; a group's page; and as written, a setting's group,
/// a group's parent, a slot offer's slot and screen, a choice's setting),
/// sorted by their text and each once. The kernel takes an id it finds here
/// in place of a copy on the heap (`FixedTables::with_ids`). Meant for a
/// build script; the generated code names `paperpaw_sdk::Id` in full. A text
/// that is not an id is left out (the kernel refuses its record anyway).
///
/// # Errors
/// A batch that does not decode.
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn fixed_ids_source(plugin: &str, name: &str, batches: &[&[u8]]) -> Result<String, String> {
    let mut ids: Vec<String> = Vec::new();
    ids.push(String::from(plugin));
    let own = |name: &str| format!("{plugin}.{name}");
    for bytes in batches {
        let batch = records_decode(bytes).map_err(|code| format!("batch: error {code}"))?;
        for record in batch.iter() {
            match record {
                Record::ScreenSpec(screen) => ids.push(own(screen.name)),
                Record::ActionSpec(action) => ids.push(own(action.name)),
                Record::SettingSpec(spec) => {
                    ids.push(own(spec.name));
                    ids.push(String::from(spec.group));
                }
                Record::SettingGroup(group) => {
                    ids.push(own(group.name));
                    if !group.page.is_empty() {
                        ids.push(own(group.page));
                    }
                    if !group.parent.is_empty() {
                        ids.push(String::from(group.parent));
                    }
                }
                Record::SlotOffer(offer) => {
                    ids.push(String::from(offer.slot));
                    ids.push(String::from(offer.screen));
                }
                Record::SettingChoice(choice) => ids.push(String::from(choice.key)),
                // No other record is made an id of by the kernel.
                _ => {}
            }
        }
    }
    ids.retain(|id| crate::id::is_valid(id));
    ids.sort_unstable();
    ids.dedup();
    let mut rows = String::new();
    for id in &ids {
        let _ = writeln!(rows, "    paperpaw_sdk::Id::new_static({id:?}),");
    }
    Ok(format!(
        "pub static {name}: [paperpaw_sdk::Id; {}] = [\n{rows}];\n",
        ids.len()
    ))
}

/// Rust source of `pub static <name>: [paperpaw_sdk::TitleEntry; N]`: the id
/// and title of every screen and action the `batches` of `plugin` declare,
/// sorted by the id's text, each id once. The kernel keeps a native screen's
/// or action's id and title as the entry it finds here, where both are the
/// same, instead of in a row of its own ([`Titled::kept`](crate::Titled::kept),
/// #248). Meant for a build script; the generated code names
/// `paperpaw_sdk::TitleEntry` and `paperpaw_sdk::Id` in full. An id that is not valid is left out (the kernel
/// refuses its record anyway).
///
/// # Errors
/// A batch that does not decode.
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn fixed_titles_source(plugin: &str, name: &str, batches: &[&[u8]]) -> Result<String, String> {
    let mut titles: Vec<(String, String)> = Vec::new();
    for bytes in batches {
        let batch = records_decode(bytes).map_err(|code| format!("batch: error {code}"))?;
        for record in batch.iter() {
            let (own, title) = match record {
                Record::ScreenSpec(screen) => (screen.name, screen.title),
                Record::ActionSpec(action) => (action.name, action.title),
                _ => continue,
            };
            titles.push((format!("{plugin}.{own}"), String::from(title)));
        }
    }
    titles.retain(|(id, _)| crate::id::is_valid(id));
    // Sorted by id; of an id declared twice the first stays (the kernel
    // refuses the second's record).
    titles.sort_by(|a, b| a.0.cmp(&b.0));
    titles.dedup_by(|later, first| later.0 == first.0);
    let mut rows = String::new();
    for (id, title) in &titles {
        let _ = writeln!(
            rows,
            "    paperpaw_sdk::TitleEntry::new(paperpaw_sdk::Id::new_static({id:?}), {title:?}),"
        );
    }
    Ok(format!(
        "pub static {name}: [paperpaw_sdk::TitleEntry; {}] = [\n{rows}];\n",
        titles.len()
    ))
}
