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

/// Data for a callback factory; both names are the host's own (copied, or
/// kept in the program: [`Source`]).
#[derive(Debug)]
pub(crate) struct Callback {
    pub(crate) id: Id,
    pub(crate) title: Cow<'static, str>,
    pub(crate) token: u32,
    pub(crate) flags: u32,
    /// The entry of the plugin's titles table with the same id and title,
    /// where it has one: the kernel keeps that, not a row of its own (#248).
    pub(crate) kept: Option<&'static paperpaw_sdk::TitleEntry>,
}

#[derive(Debug)]
pub(crate) enum Value {
    Setting(SettingSpec),
    Group(SettingGroupSpec),
    Screen(Callback),
    Action(Callback),
}

/// Production entry point: what `source` keeps in the program is taken from
/// there; any other id or text takes the SDK's checked allocation path.
pub(crate) fn from_record(
    owner: &str,
    record: wire::Record<'_>,
    source: &mut impl Source,
) -> Result<Value, i32> {
    normalize(owner, record, source)
}

/// Where the ids and texts of a declaration come from: by default copies on
/// the heap; a linked plugin's ([`Kept`]) mostly from the program (#248).
pub(crate) trait Source {
    /// The id `text` (a full id, already composed).
    ///
    /// # Errors
    /// `ERR_INVALID` for an id that is not one, `ERR_NO_MEMORY` when the
    /// copy cannot be allocated.
    fn id(&mut self, text: &str) -> Result<Id, i32>;

    /// The text `text` (a title, a description, a choice).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` when the copy cannot be allocated.
    fn text(&mut self, text: &str) -> Result<Cow<'static, str>, i32> {
        copy(text).map(Cow::Owned)
    }

    /// The entry kept in the program with the id `id` and the title `title`
    /// (a screen's, an action's); by default none.
    fn titled(&self, id: &str, title: &str) -> Option<&'static paperpaw_sdk::TitleEntry> {
        let _ = (id, title);
        None
    }
}

/// An id factory alone (tests, and every owner without tables): ids from
/// it, texts copied.
impl<F: FnMut(&str) -> Result<Id, i32>> Source for F {
    fn id(&mut self, text: &str) -> Result<Id, i32> {
        self(text)
    }
}

/// What a linked plugin keeps in the program: the ids its build made a
/// table of, and its read-only data, which stays mapped for the whole run
/// (`Kernel::register_linked`). An id or a text found there is taken from
/// there and costs no heap; any other is copied as for every plugin.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Kept {
    /// Sorted by text ([`super::FixedTables::ids`]).
    pub(crate) ids: &'static [Id],
    /// The program's read-only data; empty for none.
    pub(crate) image: &'static [u8],
    /// Sorted by id ([`super::FixedTables::titles`]).
    pub(crate) titles: &'static [paperpaw_sdk::TitleEntry],
}

impl Kept {
    /// `text` as the same bytes of the image, where it lies in it.
    fn in_image(&self, text: &str) -> Option<&'static str> {
        let start = (text.as_ptr() as usize).checked_sub(self.image.as_ptr() as usize)?;
        let bytes = self.image.get(start..start.checked_add(text.len())?)?;
        // The same bytes `text` reads (they are UTF-8); checked once more, as
        // every read of the image is: the bytes are not trusted.
        core::str::from_utf8(bytes).ok()
    }
}

impl Source for Kept {
    fn id(&mut self, text: &str) -> Result<Id, i32> {
        match self.ids.binary_search_by(|kept| kept.as_str().cmp(text)) {
            // A static id: its copy allocates nothing.
            Ok(at) => self.ids[at].try_clone().ok_or(ERR_NO_MEMORY),
            Err(_) => checked_id(text),
        }
    }

    fn titled(&self, id: &str, title: &str) -> Option<&'static paperpaw_sdk::TitleEntry> {
        let at = self
            .titles
            .binary_search_by(|kept| kept.id.as_str().cmp(id))
            .ok()?;
        // Only the very same: a title that differs keeps a row of its own.
        let entry = &self.titles[at];
        (entry.title == title).then_some(entry)
    }

    fn text(&mut self, text: &str) -> Result<Cow<'static, str>, i32> {
        if text.is_empty() {
            return Ok(Cow::Borrowed(""));
        }
        match self.in_image(text) {
            Some(kept) => Ok(Cow::Borrowed(kept)),
            None => copy(text).map(Cow::Owned),
        }
    }
}

pub(super) fn checked_id(text: &str) -> Result<Id, i32> {
    Id::try_parse(text).map_err(|error| match error {
        IdError::Invalid => ERR_INVALID,
        IdError::NoMemory => ERR_NO_MEMORY,
        _ => ERR_INVALID,
    })
}

/// Normalize one decoded declaration without exposing any partial result.
/// The source's id factory maps invalid grammar to ERR_INVALID and
/// allocation refusal to ERR_NO_MEMORY. Nothing it returns borrows the input
/// record (an id or a text is copied, or kept in the program).
pub(crate) fn normalize(
    owner: &str,
    record: wire::Record<'_>,
    id: &mut impl Source,
) -> Result<Value, i32> {
    declarations::validate_record(owner, &record)?;
    match record {
        wire::Record::SettingSpec(spec) => setting(owner, spec, id).map(Value::Setting),
        wire::Record::SettingGroup(group) => group_value(owner, group, id).map(Value::Group),
        wire::Record::ScreenSpec(screen) => Ok(Value::Screen(callback(
            owner,
            (screen.name, screen.title),
            (screen.token, screen.flags),
            id,
        )?)),
        wire::Record::ActionSpec(action) => Ok(Value::Action(callback(
            owner,
            (action.name, action.title),
            (action.token, action.flags),
            id,
        )?)),
        // Extras need fallible SDK construction before they can be mapped.
        // Resources need host file/provider resolution. Neither is dropped.
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// A screen's or an action's id and title as the kernel keeps them: the
/// entry of the plugin's titles table where the declaration has one (no heap,
/// #248), else a copy of its own (`ERR_NO_MEMORY` where the heap refuses it).
pub(crate) fn titled(
    kept: Option<&'static paperpaw_sdk::TitleEntry>,
    id: Id,
    title: Cow<'static, str>,
) -> Result<paperpaw_sdk::Titled, i32> {
    match kept {
        Some(entry) => Ok(paperpaw_sdk::Titled::kept(entry)),
        None => super::shared::try_box((id, title))
            .map(paperpaw_sdk::Titled::owned)
            .ok_or(ERR_NO_MEMORY),
    }
}

/// A screen's or an action's callback, named `name` and called `title`.
fn callback(
    owner: &str,
    (name, title): (&str, &str),
    (token, flags): (u32, u32),
    source: &mut impl Source,
) -> Result<Callback, i32> {
    let id = qualified(owner, name, source)?;
    let kept = source.titled(id.as_str(), title);
    Ok(Callback {
        id,
        title: source.text(title)?,
        token,
        flags,
        kept,
    })
}

fn setting(
    owner: &str,
    spec: wire::SettingSpec<'_>,
    id: &mut impl Source,
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
        2 => SettingKind::Choice(choices(spec.choices, id)?),
        3 => SettingKind::Text,
        4 => SettingKind::Language,
        5 => SettingKind::List,
        _ => return Err(ERR_UNSUPPORTED),
    };
    let default = match settings_decode(spec.default)? {
        SettingValueRef::Bool(value) => SettingValue::Bool(value),
        SettingValueRef::Int(value) => SettingValue::Int(value),
        SettingValueRef::Text(text) => SettingValue::Text(id.text(text)?),
        _ => return Err(ERR_INVALID),
    };
    let mut value = SettingSpec::new(
        qualified(owner, spec.name, id)?,
        "",
        kind,
        default,
        id.id(spec.group)?,
    );
    value.title = paperpaw_sdk::Str::new(id.text(spec.title)?);
    value.description = if spec.help.is_empty() {
        None
    } else {
        Some(paperpaw_sdk::Str::new(id.text(spec.help)?))
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

fn choices(text: &str, source: &mut impl Source) -> Result<Choices, i32> {
    let count = text.split(wire::SETTING_SPEC_CHOICES_SEPARATOR).count();
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| ERR_NO_MEMORY)?;
    for value in text.split(wire::SETTING_SPEC_CHOICES_SEPARATOR) {
        values.push(source.text(value)?);
    }
    Ok(Choices::Owned(values))
}

fn group_value(
    owner: &str,
    group: wire::SettingGroup<'_>,
    id: &mut impl Source,
) -> Result<SettingGroupSpec, i32> {
    let mut value = SettingGroupSpec::new(qualified(owner, group.name, id)?, "");
    value.title = id.text(group.title)?;
    value.order = group.order;
    value.page = if group.page.is_empty() {
        None
    } else {
        Some(qualified(owner, group.page, id)?)
    };
    value.parent = if group.parent.is_empty() {
        None
    } else {
        Some(id.id(group.parent)?)
    };
    Ok(value)
}

pub(super) fn qualified(owner: &str, name: &str, id: &mut impl Source) -> Result<Id, i32> {
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
    id.id(text)
}

pub(super) fn copy(text: &str) -> Result<String, i32> {
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
