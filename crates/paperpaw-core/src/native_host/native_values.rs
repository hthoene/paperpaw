//! Fallible owned values for the host's native declaration transaction.
//!
//! Registry publication and callback factories belong to the caller. This
//! module never retains a borrowed record or modifies a live registry.

use super::{declarations, registrations as wire};
use crate::abi7::settings_wire::{SettingValueRef, settings_decode};
use alloc::{borrow::Cow, string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED};
use paperpaw_sdk::{
    Choices, Id, IdError, Level, SettingGroupSpec, SettingKind, SettingSpec, SettingValue,
};

const SCOPED: u32 = 1;
const GUARDED: u32 = 1 << 1;
const KEPT: u32 = 1 << 2;
const SECRET: u32 = 1 << 3;
const ADVANCED: u32 = 1 << 4;

/// Data for a callback factory; both names have already been copied.
#[derive(Debug)]
pub(crate) struct Callback {
    pub(crate) id: Id,
    pub(crate) title: String,
    pub(crate) token: u32,
    pub(crate) flags: u32,
}

#[derive(Debug)]
pub(crate) enum Value {
    Setting(SettingSpec),
    Group(SettingGroupSpec),
    Screen(Callback),
    Action(Callback),
}

/// Production entry point: dynamic ids use the SDK's checked allocation path.
pub(crate) fn from_record(owner: &str, record: wire::Record<'_>) -> Result<Value, i32> {
    normalize(owner, record, &mut checked_id)
}

fn checked_id(text: &str) -> Result<Id, i32> {
    Id::try_parse(text).map_err(|error| match error {
        IdError::Invalid => ERR_INVALID,
        IdError::NoMemory => ERR_NO_MEMORY,
        _ => ERR_INVALID,
    })
}

/// Normalize one decoded declaration without exposing any partial result.
/// The checked id factory maps invalid grammar to ERR_INVALID and allocation
/// refusal to ERR_NO_MEMORY. It must copy the borrowed input before returning.
pub(crate) fn normalize(
    owner: &str,
    record: wire::Record<'_>,
    id: &mut impl FnMut(&str) -> Result<Id, i32>,
) -> Result<Value, i32> {
    declarations::validate_record(owner, &record)?;
    match record {
        wire::Record::SettingSpec(spec) => setting(owner, spec, id).map(Value::Setting),
        wire::Record::SettingGroup(group) => group_value(owner, group, id).map(Value::Group),
        wire::Record::ScreenSpec(screen) => Ok(Value::Screen(Callback {
            id: qualified(owner, screen.name, id)?,
            title: copy(screen.title)?,
            token: screen.token,
            flags: screen.flags,
        })),
        wire::Record::ActionSpec(action) => Ok(Value::Action(Callback {
            id: qualified(owner, action.name, id)?,
            title: copy(action.title)?,
            token: action.token,
            flags: action.flags,
        })),
        // Extras need fallible SDK construction before they can be mapped.
        // Resources need host file/provider resolution. Neither is dropped.
        _ => Err(ERR_UNSUPPORTED),
    }
}

fn setting(
    owner: &str,
    spec: wire::SettingSpec<'_>,
    id: &mut impl FnMut(&str) -> Result<Id, i32>,
) -> Result<SettingSpec, i32> {
    if spec.flags & SECRET != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    let kind = match spec.kind {
        0 => SettingKind::Bool,
        1 => SettingKind::Int {
            min: i64::from(spec.min),
            max: i64::from(spec.max),
        },
        2 => SettingKind::Choice(choices(spec.choices)?),
        3 => SettingKind::Text,
        4 => SettingKind::Language,
        5 => SettingKind::List,
        _ => return Err(ERR_UNSUPPORTED),
    };
    let default = match settings_decode(spec.default)? {
        SettingValueRef::Bool(value) => SettingValue::Bool(value),
        SettingValueRef::Int(value) => SettingValue::Int(value),
        SettingValueRef::Text(text) => SettingValue::Text(Cow::Owned(copy(text)?)),
        _ => return Err(ERR_INVALID),
    };
    let mut value = SettingSpec::new(
        qualified(owner, spec.name, id)?,
        "",
        kind,
        default,
        id(spec.group)?,
    );
    value.title = copy(spec.title)?.into();
    value.description = if spec.help.is_empty() {
        None
    } else {
        Some(copy(spec.help)?.into())
    };
    value.scoped = spec.flags & SCOPED != 0;
    value.guarded = spec.flags & GUARDED != 0;
    value.kept_on_reset = spec.flags & KEPT != 0;
    value.level = if spec.flags & ADVANCED != 0 {
        Level::Advanced
    } else {
        Level::Basic
    };
    Ok(value)
}

fn choices(text: &str) -> Result<Choices, i32> {
    let count = text.split(wire::SETTING_SPEC_CHOICES_SEPARATOR).count();
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| ERR_NO_MEMORY)?;
    for value in text.split(wire::SETTING_SPEC_CHOICES_SEPARATOR) {
        values.push(Cow::Owned(copy(value)?));
    }
    Ok(Choices::Owned(values))
}

fn group_value(
    owner: &str,
    group: wire::SettingGroup<'_>,
    id: &mut impl FnMut(&str) -> Result<Id, i32>,
) -> Result<SettingGroupSpec, i32> {
    let mut value = SettingGroupSpec::new(qualified(owner, group.name, id)?, "");
    value.title = Cow::Owned(copy(group.title)?);
    value.order = group.order;
    value.page = if group.page.is_empty() {
        None
    } else {
        Some(qualified(owner, group.page, id)?)
    };
    value.parent = if group.parent.is_empty() {
        None
    } else {
        Some(id(group.parent)?)
    };
    Ok(value)
}

fn qualified(
    owner: &str,
    name: &str,
    id: &mut impl FnMut(&str) -> Result<Id, i32>,
) -> Result<Id, i32> {
    let end = owner
        .len()
        .checked_add(1)
        .and_then(|length| length.checked_add(name.len()))
        .filter(|length| *length <= declarations::FULL_ID_MAX_BYTES)
        .ok_or(ERR_INVALID)?;
    let mut bytes = [0; declarations::FULL_ID_MAX_BYTES];
    bytes[..owner.len()].copy_from_slice(owner.as_bytes());
    bytes[owner.len()] = b'.';
    bytes[owner.len() + 1..end].copy_from_slice(name.as_bytes());
    let text = core::str::from_utf8(&bytes[..end]).map_err(|_| ERR_INVALID)?;
    id(text)
}

fn copy(text: &str) -> Result<String, i32> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    owned.push_str(text);
    // Str adopts the string as Box<str>. Refuse spare capacity so adoption
    // cannot perform an unchecked shrink allocation on the X4 heap.
    if owned.capacity() != owned.len() {
        return Err(ERR_NO_MEMORY);
    }
    Ok(owned)
}

#[cfg(test)]
#[path = "native_values_tests.rs"]
mod tests;
