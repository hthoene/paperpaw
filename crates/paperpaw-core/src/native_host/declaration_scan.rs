//! Owners whose declarations are all read from a read-only image keep no
//! index (#248, Option B): their records of a kind are found by walking the
//! frame headers of their batches (a tag and a length each) and decoding only
//! the records of that kind.
//!
//! Such an owner's records are visited in declaration order (batch, then
//! record), as an indexed owner's are. Its names are not checked again for
//! duplicates at every visit: there is no index left to keep consistent,
//! admission refused duplicates, and the record id and namespace are still
//! checked on every visit (`declarations::validate_record`). A plugin that
//! rewrites its own image after admission (a card plugin's mapped module)
//! can hide its own records behind a duplicate name (a name lookup finds the
//! first in declaration order) and can also declare more formats or readers
//! than the device-wide caps counted at admission allow: whatever reads those
//! records pushes them fallibly, and plugins are trusted code (decision 44),
//! so this is robustness only, never another plugin's names or memory.
use super::super::raw_declarations::varint;
use super::super::registrations::{
    ACTION_SPEC_TAG, BOOK_BADGE_SPEC_TAG, BOOK_FIELD_SPEC_TAG, FORMAT_SPEC_TAG, HELP_HINT_TAG,
    ICON_SPEC_TAG, INPUT_SPEC_TAG, KEY_BIND_TAG, LOOKUP_SPEC_TAG, MENU_DESCRIBE_TAG,
    MENU_ENTRY_TAG, MENU_HIDE_TAG, MENU_NEED_TAG, MENU_SECTION_TAG, PRESET_SPEC_TAG,
    PRESET_VALUE_TAG, READER_SPEC_TAG, REGISTRATION_MAX_RECORDS, REGISTRATION_STAGE_TAG,
    ResourceFontExtension, ResourceFontFace, ResourceHyphenation, SCREEN_SPEC_TAG,
    SETTING_CHOICE_TAG, SETTING_DEFAULT_TAG, SETUP_STEP_TAG, SLOT_OFFER_TAG, SLOT_SPEC_TAG,
    STAGE_SPEC_TAG, STATUS_ITEM_TAG, STATUS_LINE_TAG, STATUS_PLACE_TAG, SettingExtra, SettingGroup,
    SettingSpec, WIDGET_PLACE_TAG, WIDGET_SPEC_TAG, WireRecord,
};
use super::Kind;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE};

const _: () = assert!(REGISTRATION_MAX_RECORDS <= u8::BITS as usize);

// Tags the registrations module does not name: the records' own.
const SETTING_GROUP_TAG: u32 = <SettingGroup<'static> as WireRecord<'static>>::TAG;
const SETTING_SPEC_TAG: u32 = <SettingSpec<'static> as WireRecord<'static>>::TAG;
const SETTING_EXTRA_TAG: u32 = <SettingExtra<'static> as WireRecord<'static>>::TAG;
const RESOURCE_HYPHENATION_TAG: u32 = <ResourceHyphenation<'static> as WireRecord<'static>>::TAG;
const RESOURCE_FONT_FACE_TAG: u32 = <ResourceFontFace<'static> as WireRecord<'static>>::TAG;
const RESOURCE_FONT_EXTENSION_TAG: u32 =
    <ResourceFontExtension<'static> as WireRecord<'static>>::TAG;

impl Kind {
    /// The kind of a record by its frame's tag, as [`Kind::of`] gives it for
    /// the decoded record (the tests compare both).
    pub(super) fn of_tag(tag: u32) -> Option<Self> {
        Some(match tag {
            SETTING_GROUP_TAG => Self::Group,
            SETTING_SPEC_TAG => Self::Spec,
            SETTING_EXTRA_TAG => Self::Extra,
            RESOURCE_HYPHENATION_TAG => Self::Hyphenation,
            RESOURCE_FONT_FACE_TAG => Self::Face,
            RESOURCE_FONT_EXTENSION_TAG => Self::Extension,
            SCREEN_SPEC_TAG => Self::Screen,
            ACTION_SPEC_TAG => Self::Action,
            WIDGET_SPEC_TAG => Self::Widget,
            WIDGET_PLACE_TAG => Self::WidgetPlace,
            STATUS_ITEM_TAG => Self::StatusItem,
            STATUS_LINE_TAG => Self::StatusLine,
            STATUS_PLACE_TAG => Self::StatusPlace,
            MENU_ENTRY_TAG => Self::MenuEntry,
            ICON_SPEC_TAG => Self::Icon,
            MENU_NEED_TAG => Self::MenuNeed,
            MENU_SECTION_TAG => Self::MenuSection,
            MENU_HIDE_TAG => Self::MenuHide,
            MENU_DESCRIBE_TAG => Self::MenuDescribe,
            SLOT_SPEC_TAG => Self::SlotSpec,
            SLOT_OFFER_TAG => Self::SlotOffer,
            KEY_BIND_TAG => Self::KeyBind,
            SETTING_DEFAULT_TAG => Self::SettingDefault,
            SETTING_CHOICE_TAG => Self::SettingChoice,
            PRESET_SPEC_TAG => Self::PresetSpec,
            PRESET_VALUE_TAG => Self::PresetValue,
            STAGE_SPEC_TAG => Self::Stage,
            REGISTRATION_STAGE_TAG => Self::Subscription,
            SETUP_STEP_TAG => Self::SetupStep,
            INPUT_SPEC_TAG => Self::InputSpec,
            HELP_HINT_TAG => Self::HelpHint,
            FORMAT_SPEC_TAG => Self::Format,
            READER_SPEC_TAG => Self::Reader,
            LOOKUP_SPEC_TAG => Self::Lookup,
            BOOK_FIELD_SPEC_TAG => Self::BookField,
            BOOK_BADGE_SPEC_TAG => Self::BookBadge,
            _ => return None,
        })
    }
}

/// The records of `kind` in the batch `wire`, one bit each by their place
/// (bit 0 the first): their frames walked, nothing decoded. `ERR_INVALID` for
/// a count over the batch limit or a frame that leaves the batch.
pub(super) fn places_of(wire: &[u8], kind: Kind) -> Result<u8, i32> {
    let mut cursor = 0;
    let count = varint(wire, &mut cursor)? as usize;
    if count > REGISTRATION_MAX_RECORDS {
        return Err(ERR_INVALID);
    }
    let mut places = 0_u8;
    for place in 0..count {
        let tag = varint(wire, &mut cursor)?;
        let len = usize::try_from(varint(wire, &mut cursor)?).map_err(|_| ERR_INVALID)?;
        cursor = cursor
            .checked_add(len)
            .filter(|end| *end <= wire.len())
            .ok_or(ERR_INVALID)?;
        if Kind::of_tag(tag) == Some(kind) {
            places |= 1 << place;
        }
    }
    Ok(places)
}

/// The batch that starts at `start` in `image`: its count and frames walked to
/// its end, nothing decoded. `ERR_INVALID` for a start past the image, a count
/// over the batch limit or a frame that leaves the image.
pub(super) fn batch_at(image: &[u8], start: usize) -> Result<&[u8], i32> {
    let rest = image.get(start..).ok_or(ERR_INVALID)?;
    let mut cursor = 0;
    let count = varint(rest, &mut cursor)? as usize;
    if count > REGISTRATION_MAX_RECORDS {
        return Err(ERR_INVALID);
    }
    for _ in 0..count {
        varint(rest, &mut cursor)?;
        let len = usize::try_from(varint(rest, &mut cursor)?).map_err(|_| ERR_INVALID)?;
        cursor = cursor
            .checked_add(len)
            .filter(|end| *end <= rest.len())
            .ok_or(ERR_INVALID)?;
    }
    rest.get(..cursor).ok_or(ERR_INVALID)
}

/// The places of the records of `kind` of an image-only `owner` whose
/// batches start at `starts` (with the kinds each may declare), for
/// [`StoredOwner::places`](super::StoredOwner): a batch without the kind is
/// not walked, and one that may have it is walked once.
pub(super) fn image_places(
    owner: &super::StoredOwner,
    starts: &super::Starts,
    kind: Kind,
    visit: &mut dyn FnMut(u16, u16) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let bit = 1_u64 << kind as u32;
    for at in 0..starts.len() {
        if starts.kinds(at) & bit == 0 {
            continue;
        }
        let Some(start) = starts.start(at) else {
            break;
        };
        // Below `BATCHES_MAX`: checked when the batch was staged.
        let Ok(batch) = u16::try_from(at) else {
            break;
        };
        let mut found = Ok(0);
        owner.source.with_image(&mut |image| {
            // The frames are walked once: from the start to the batch's
            // last record of any kind, each checked to stay in the image.
            found = image
                .ok_or(ERR_STATE)
                .and_then(|image| {
                    let start = usize::try_from(start).map_err(|_| ERR_INVALID)?;
                    image.get(start..).ok_or(ERR_INVALID)
                })
                .and_then(|bytes| places_of(bytes, kind));
        });
        let places = match found {
            Ok(places) => places,
            Err(ERR_STATE) => return ControlFlow::Continue(()),
            Err(error) => {
                owner.source.invalid_declaration(error);
                continue;
            }
        };
        for record in 0..u8::BITS as u16 {
            if places & 1 << record != 0 {
                visit(batch, record)?;
            }
        }
    }
    ControlFlow::Continue(())
}

/// The first record of `kind` of the image-only `owner` (its batches start at
/// `starts`) whose key fields are `parts`, in declaration order, given to
/// `visit`: as a lookup through
/// [`DeclarationStore::with_record`](super::DeclarationStore::with_record)
/// would find it. The image is held once and each batch's frames are walked
/// once; only a record that [`may_match`] is decoded, and only the one that
/// matches is validated (`declarations::validate_record`): a key press, a
/// menu's open or a settings page looks up names among every record of the
/// kind (#248, R1). Whether one was found; a batch whose frames do not walk
/// and a decoded record that does not decode or validate are said to the
/// plugin and are no match.
pub(super) fn find_in_image(
    owner: &super::StoredOwner,
    starts: &super::Starts,
    kind: Kind,
    parts: &[&str],
    visit: &mut dyn for<'a> FnMut(super::Record<'a>),
) -> bool {
    // A stale owner (a later generation of its plugin) answers nothing, as
    // `with_record` checks it for every key.
    if owner.source.generation() != owner.generation {
        return false;
    }
    let bit = 1_u64 << kind as u32;
    let mut found = false;
    owner.source.with_image(&mut |image| {
        let Some(image) = image else {
            return;
        };
        for at in 0..starts.len() {
            if starts.kinds(at) & bit == 0 {
                continue;
            }
            let Some(start) = starts.start(at) else {
                break;
            };
            match find_in_batch(owner, image, start, kind, parts, visit) {
                Ok(false) => {}
                Ok(true) => {
                    found = true;
                    break;
                }
                Err(error) => owner.source.invalid_declaration(error),
            }
        }
    });
    found
}

/// [`find_in_image`] in the batch that starts at `start` in `image`. Its
/// frames are walked to its end first, so that a batch with a broken frame
/// answers nothing (`ERR_INVALID`, as [`batch_at`]); then the records that
/// may match are decoded in their order.
fn find_in_batch(
    owner: &super::StoredOwner,
    image: &[u8],
    start: u32,
    kind: Kind,
    parts: &[&str],
    visit: &mut dyn for<'a> FnMut(super::Record<'a>),
) -> Result<bool, i32> {
    let start = usize::try_from(start).map_err(|_| ERR_INVALID)?;
    let rest = image.get(start..).ok_or(ERR_INVALID)?;
    let category = kind.category();
    let mut cursor = 0;
    let count = varint(rest, &mut cursor)? as usize;
    if count > REGISTRATION_MAX_RECORDS {
        return Err(ERR_INVALID);
    }
    // Where the frames that may match start: at most a batch's records.
    let mut frames = [0_usize; REGISTRATION_MAX_RECORDS];
    let mut waiting = 0;
    for _ in 0..count {
        let frame = cursor;
        let tag = varint(rest, &mut cursor)?;
        let len = usize::try_from(varint(rest, &mut cursor)?).map_err(|_| ERR_INVALID)?;
        let end = cursor.checked_add(len).ok_or(ERR_INVALID)?;
        let body = rest.get(cursor..end).ok_or(ERR_INVALID)?;
        cursor = end;
        if Kind::of_tag(tag) != Some(kind) || !may_match(body, category, parts) {
            continue;
        }
        if let Some(slot) = frames.get_mut(waiting) {
            *slot = frame;
            waiting += 1;
        }
    }
    let batch = rest.get(..cursor).ok_or(ERR_INVALID)?;
    for frame in frames.iter().take(waiting) {
        // From the record's frame to the batch's end, as `record_at` gives it.
        let result = batch
            .get(*frame..)
            .ok_or(ERR_INVALID)
            .and_then(super::super::registrations::host_decode_prefix)
            .and_then(|(record, _)| {
                if !super::super::declaration_index::matches_parts(record, category, parts) {
                    return Ok(false);
                }
                super::super::declarations::validate_record(owner.id.as_str(), &record)?;
                visit(record);
                Ok(true)
            });
        match result {
            Ok(false) => {}
            Ok(true) => return Ok(true),
            Err(error) => owner.source.invalid_declaration(error),
        }
    }
    Ok(false)
}

/// Whether the record in the frame body `body` may have the key fields
/// `parts`, without decoding it. A record's first key field is text its
/// decoded form borrows from the frame (`declaration_index::key`), so a body
/// without those bytes cannot match; names that compare without case are
/// always decoded.
fn may_match(body: &[u8], category: u8, parts: &[&str]) -> bool {
    if super::super::declaration_index::folds_case(category) {
        return true;
    }
    let Some((first, tail)) = parts.first().and_then(|name| name.as_bytes().split_first()) else {
        return true;
    };
    let mut rest = body;
    while let Some(at) = rest.iter().position(|byte| byte == first) {
        let after = rest.get(at + 1..).unwrap_or_default();
        if after.starts_with(tail) {
            return true;
        }
        rest = after;
    }
    false
}

#[cfg(test)]
#[path = "declaration_scan_tests.rs"]
mod tests;
