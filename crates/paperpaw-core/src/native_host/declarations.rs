//! Registry-free syntactic validation of declaration records (tags 5-10).
//!
//! Every function checks one record, or one whole batch, without allocating and
//! without looking at any registry. Cross-record rules (uniqueness, references
//! that must resolve, limits per plugin) belong to the commit and are not here.
//! Statuses: unknown enum values and flag bits return `ERR_UNSUPPORTED`, every
//! other violation returns `ERR_INVALID`; UTF-8 and byte caps are already
//! enforced by the codec (`ERR_UTF8`, `ERR_NO_SPACE`).

use super::registrations::{
    Record, RecordBatch, ResourceFontExtension, ResourceFontFace, ResourceHyphenation,
    SettingExtra, SettingGroup, SettingSpec,
};
use crate::abi7::settings_wire::{SettingValueRef, settings_decode};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

/// Longest full id (plugin id, dot, relative name) in bytes: slot 15's key cap.
pub const FULL_ID_MAX_BYTES: usize = 128;
/// Most values in a choice list.
pub const CHOICES_MAX_VALUES: usize = 32;
/// Longest single choice value in bytes.
pub const CHOICE_MAX_BYTES: usize = 96;
/// Most segments in a resource path.
pub const PATH_MAX_SEGMENTS: usize = 3;
/// Longest resource path in bytes.
pub const PATH_MAX_BYTES: usize = 100;
/// Longest language tag in bytes.
pub const LANGUAGE_MAX_BYTES: usize = 35;

/// Spec kinds: bool, int, choice, text, language, list.
const KIND_MAX: u32 = 5;
const SPEC_FLAGS: u32 = 0b1_1111;
const SPEC_SCOPED: u32 = 1;
const SPEC_SECRET: u32 = 1 << 3;
const EXTRA_FLAGS: u32 = 0b111;
const STYLE_BITS: u32 = 0b11;
const WEIGHT_MAX: u32 = 1000;
const PRIORITY_RANGE: core::ops::RangeInclusive<i32> = -100..=100;

type Check = Result<(), i32>;

fn ensure(condition: bool) -> Check {
    if condition { Ok(()) } else { Err(ERR_INVALID) }
}

/// Validate a whole decoded batch before anything is staged.
///
/// Callback registrations carry no text and pass; each declaration is checked
/// by [`validate_record`]. The first failure is returned.
///
/// # Errors
/// The status of the first invalid record.
pub fn validate_batch(plugin_id: &str, batch: &RecordBatch<'_>) -> Check {
    batch
        .iter()
        .try_for_each(|record| validate_record(plugin_id, &record))
}

/// Validate one record without a registry. `plugin_id` is the host-known id.
///
/// # Errors
/// `ERR_UNSUPPORTED` for unknown kinds, flag bits, styles or a nonzero `check`;
/// `ERR_INVALID` for any other violated rule.
pub fn validate_record(plugin_id: &str, record: &Record<'_>) -> Check {
    match record {
        Record::SettingGroup(group) => group_check(plugin_id, group),
        Record::SettingSpec(spec) => spec_check(plugin_id, spec),
        Record::SettingExtra(extra) => extra_check(plugin_id, extra),
        Record::ResourceHyphenation(record) => hyphenation_check(record),
        Record::ResourceFontFace(face) => face_check(plugin_id, face),
        Record::ResourceFontExtension(extension) => extension_check(plugin_id, extension),
        _ => Ok(()),
    }
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}

/// Dot-separated segments of `[a-z0-9-]`, none empty, at most 128 bytes.
fn full_id(id: &str) -> Check {
    ensure(
        id.len() <= FULL_ID_MAX_BYTES
            && id
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(is_name_byte)),
    )
}

/// A name relative to the plugin; the joined full id must also fit.
fn relative(plugin_id: &str, name: &str) -> Check {
    full_id(name)?;
    ensure(plugin_id.len() + 1 + name.len() <= FULL_ID_MAX_BYTES)
}

fn optional_full_id(id: &str) -> Check {
    if id.is_empty() { Ok(()) } else { full_id(id) }
}

fn group_check(plugin_id: &str, group: &SettingGroup<'_>) -> Check {
    relative(plugin_id, group.name)?;
    ensure(!group.title.is_empty())?;
    if !group.page.is_empty() {
        relative(plugin_id, group.page)?;
    }
    optional_full_id(group.parent)
}

fn spec_check(plugin_id: &str, spec: &SettingSpec<'_>) -> Check {
    relative(plugin_id, spec.name)?;
    if spec.kind > KIND_MAX || spec.flags & !SPEC_FLAGS != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    let secret_scoped = SPEC_SCOPED | SPEC_SECRET;
    ensure(spec.flags & secret_scoped != secret_scoped)?;
    ensure(!spec.group.is_empty())?;
    full_id(spec.group)?;
    ensure(!spec.title.is_empty())?;
    let value = settings_decode(spec.default)?;
    if spec.kind == 1 {
        ensure(spec.min <= spec.max)?;
    } else {
        ensure(spec.min == 0 && spec.max == 0)?;
    }
    if spec.kind != 2 {
        ensure(spec.choices.is_empty())?;
    }
    match (spec.kind, value) {
        (0, SettingValueRef::Bool(_)) | (3 | 5, SettingValueRef::Text(_)) => Ok(()),
        (1, SettingValueRef::Int(n)) => {
            ensure(i64::from(spec.min) <= n && n <= i64::from(spec.max))
        }
        (2, SettingValueRef::Text(text)) => choices_check(spec.choices, text),
        (4, SettingValueRef::Text(tag)) => ensure(language_tag(tag, true)),
        _ => Err(ERR_INVALID),
    }
}

/// 1 to 32 distinct, non-empty values of at most 96 bytes; `default` is one.
/// Refused as soon as a 33rd value starts, so at most 32 values are compared.
fn choices_check(choices: &str, default: &str) -> Check {
    let mut count = 0;
    let mut found = false;
    for (index, value) in choices.split('\n').enumerate() {
        ensure(index < CHOICES_MAX_VALUES)?;
        ensure(!value.is_empty() && value.len() <= CHOICE_MAX_BYTES)?;
        ensure(
            choices
                .split('\n')
                .take(index)
                .all(|earlier| earlier != value),
        )?;
        found |= value == default;
        count += 1;
    }
    ensure(count <= CHOICES_MAX_VALUES && found)
}

/// `[A-Za-z0-9-]`, at most 35 bytes; empty only where `allow_empty`.
fn language_tag(tag: &str, allow_empty: bool) -> bool {
    (allow_empty || !tag.is_empty())
        && tag.len() <= LANGUAGE_MAX_BYTES
        && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn extra_check(plugin_id: &str, extra: &SettingExtra<'_>) -> Check {
    relative(plugin_id, extra.name)?;
    if extra.check != 0 || extra.editor_kind > 2 || extra.flags & !EXTRA_FLAGS != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    ensure(extra.step >= 0)?;
    match extra.editor_kind {
        0 => ensure(extra.editor.is_empty())?,
        // A screen of the plugin: relative to it.
        1 => relative(plugin_id, extra.editor)?,
        // A slot: a full id.
        _ => full_id(extra.editor)?,
    }
    ensure(extra.shown_id.is_empty() == extra.shown_value.is_empty())?;
    optional_full_id(extra.follows)?;
    optional_full_id(extra.shown_id)
}

/// Relative path rule: `[A-Za-z0-9._-]` and `/`, at most 3 segments of at most
/// 100 bytes in all, no empty, `.`, `..` or dot-leading segment.
fn path_reference(path: &str) -> Check {
    let segment = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    ensure(
        path.len() <= PATH_MAX_BYTES
            && path.split('/').count() <= PATH_MAX_SEGMENTS
            && path.split('/').all(segment),
    )
}

/// The font files a face may name; the extension selects the reader.
const FONT_EXTENSIONS: [&str; 4] = [".ppfp", ".ttf", ".otf", ".ttc"];

fn has_extension(path: &str, extension: &str) -> bool {
    path.len() > extension.len()
        && path.as_bytes()[path.len() - extension.len()..]
            .eq_ignore_ascii_case(extension.as_bytes())
}

fn hyphenation_check(record: &ResourceHyphenation<'_>) -> Check {
    ensure(language_tag(record.language, false))?;
    path_reference(record.path)
}

fn face_check(plugin_id: &str, face: &ResourceFontFace<'_>) -> Check {
    relative(plugin_id, face.name)?;
    path_reference(face.path)?;
    ensure(FONT_EXTENSIONS.iter().any(|e| has_extension(face.path, e)))?;
    if face.style & !STYLE_BITS != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    ensure(face.weight <= WEIGHT_MAX)?;
    ensure(face.weight == 0 || !has_extension(face.path, ".ppfp"))?;
    ensure(PRIORITY_RANGE.contains(&face.priority))?;
    // A priority only ranks a role binding: without a role it must be 0.
    ensure(!face.role.is_empty() || face.priority == 0)?;
    optional_full_id(face.role)
}

fn extension_check(plugin_id: &str, extension: &ResourceFontExtension<'_>) -> Check {
    relative(plugin_id, extension.name)?;
    path_reference(extension.path)?;
    full_id(extension.extends)
}
