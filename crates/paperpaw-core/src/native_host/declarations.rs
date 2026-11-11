//! Registry-free syntactic validation of declaration records (tags 5-18).
//!
//! Every function checks one record, or one whole batch, without allocating and
//! without looking at any registry. Cross-record rules (uniqueness, references
//! that must resolve, limits per plugin) belong to the commit and are not here.
//! Statuses: unknown enum values and flag bits return `ERR_UNSUPPORTED`, every
//! other violation returns `ERR_INVALID`; UTF-8 and byte caps are already
//! enforced by the codec (`ERR_UTF8`, `ERR_NO_SPACE`). The document, library and
//! lookup declarations (`declarations_docs.rs`) are the exception: their unknown
//! values are `ERR_INVALID`.

use super::registrations::{
    ActionSpec, ICON_SPEC_ICON_MAX_BYTES, IconSpec, MenuDescribe, MenuEntry, MenuHide, MenuNeed,
    MenuSection, Record, RecordBatch, RegistrationStage, ResourceFontExtension, ResourceFontFace,
    ResourceHyphenation, SETTING_SPEC_CHOICES_ITEM_MAX_BYTES, SETTING_SPEC_CHOICES_MAX_ITEMS,
    SETTING_SPEC_CHOICES_SEPARATOR, STAGE_CALL_PAYLOAD_MAX_BYTES, ScreenSpec, SettingExtra,
    SettingGroup, SettingSpec, StageSpec,
};
use crate::abi7::settings_wire::{SettingValueRef, settings_decode};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

/// Longest full id (plugin id, dot, relative name) in bytes: slot 15's key cap.
pub const FULL_ID_MAX_BYTES: usize = 128;
/// Most values in a choice list (generated from `abi.def`).
pub const CHOICES_MAX_VALUES: usize = SETTING_SPEC_CHOICES_MAX_ITEMS;
/// Longest single choice value in bytes (generated from `abi.def`).
pub const CHOICE_MAX_BYTES: usize = SETTING_SPEC_CHOICES_ITEM_MAX_BYTES;
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
/// `ERR_INVALID` for any other violated rule. The format, reader, lookup and
/// book declarations are the exception: their unknown values are
/// `ERR_INVALID` too (`declarations_docs.rs`).
#[expect(
    clippy::too_many_lines,
    reason = "one explicit arm per record kind (no catch-all): a new record fails to compile here until it is classified"
)]
pub fn validate_record(plugin_id: &str, record: &Record<'_>) -> Check {
    match record {
        Record::SettingGroup(group) => group_check(plugin_id, group),
        Record::SettingSpec(spec) => spec_check(plugin_id, spec),
        Record::SettingExtra(extra) => extra_check(plugin_id, extra),
        Record::ResourceHyphenation(record) => hyphenation_check(record),
        Record::ResourceFontFace(face) => face_check(plugin_id, face),
        Record::ResourceFontExtension(extension) => extension_check(plugin_id, extension),
        Record::ScreenSpec(screen) => screen_check(plugin_id, screen),
        Record::ActionSpec(action) => action_check(plugin_id, action),
        // Menus and icons (`docs/abi/screens-input.md` section 1): checked here,
        // the items of the plugin's own resolved at commit (`declaration_checks`).
        Record::MenuEntry(entry) => menu_entry_check(entry),
        Record::IconSpec(icon) => icon_check(icon),
        Record::MenuNeed(need) => need_check(need),
        Record::MenuSection(section) => section_check(plugin_id, section),
        Record::MenuHide(hide) => hide_check(hide),
        Record::MenuDescribe(describe) => describe_check(plugin_id, describe),
        // Stages (`docs/abi/stages-services.md` section 1.1): checked here;
        // the whole-plugin rules (eight subscriptions, none to its own stage,
        // one name and one token per stage) are `declaration_checks`.
        Record::StageSpec(spec) => stage_spec_check(plugin_id, spec),
        Record::RegistrationStage(registration) => stage_registration_check(registration),
        // Registry declarations (`docs/abi/registry.md` section 6): checked
        // here, admitted into the declaration store and the catalog. Their
        // per-plugin duplicates and own-screen references are checked over the
        // whole declaration set at commit (`declaration_checks.rs`).
        Record::SlotSpec(_)
        | Record::SlotOffer(_)
        | Record::KeyBind(_)
        | Record::PresetSpec(_)
        | Record::SettingDefault(_)
        | Record::SettingChoice(_)
        | Record::PresetValue(_) => super::declaration_reg_checks::validate(plugin_id, record),
        // A setup step (`registry.md` section 6): checked here, its name
        // against the plugin's actions, screens, settings and menu sections at
        // commit (`declaration_checks.rs`).
        Record::SetupStep(_) => super::declaration_reg_checks::validate(plugin_id, record),
        // Input and help declarations (`docs/abi/ui.md` sections 2 and 7):
        // checked here and kept in the declaration store (`declarations_input`).
        Record::InputSpec(_) | Record::HelpHint(_) => {
            super::declarations_input::validate(plugin_id, record)
        }
        // Widget and status declarations: checked in `declarations_uix`.
        Record::WidgetSpec(_)
        | Record::WidgetPlace(_)
        | Record::StatusItem(_)
        | Record::StatusLine(_)
        | Record::StatusPlace(_) => super::declarations_uix::validate(plugin_id, record),
        // Format and reader declarations (`docs/abi/document.md` section 1):
        // admitted (the cross-record limits are the commit's), as are the
        // book fields and badges (`library.md`). The lookup and book-data
        // declarations are checked, then refused until the host admits them
        // (DOC/LIB hand-off).
        Record::FormatSpec(spec) => docs::format_check(plugin_id, spec),
        Record::ReaderSpec(spec) => docs::reader_check(plugin_id, spec),
        // A lookup provider is admitted (the counts are the commit's and the
        // store's, `document.md` section 7), as are book fields and badges.
        Record::LookupSpec(spec) => docs::lookup_check(plugin_id, spec),
        Record::BookFieldSpec(spec) => docs::book_field_check(plugin_id, spec),
        Record::BookBadgeSpec(spec) => docs::book_badge_check(plugin_id, spec),
        Record::BookDataSpec(spec) => {
            docs::book_data_check(plugin_id, spec).and(Err(ERR_UNSUPPORTED))
        }
        Record::Registration(_) => Ok(()),
        // Events flow from the host to the plugin; a batch never carries one.
        Record::EventStarted(_)
        | Record::EventResume(_)
        | Record::EventSuspend(_)
        | Record::EventStopping(_)
        | Record::EventBatteryMeasured(_)
        | Record::EventPageTurned(_)
        | Record::EventScreenShown(_)
        | Record::EventActionRun(_)
        | Record::EventSettingChanged(_)
        | Record::EventBookOpened(_)
        | Record::EventBookClosed(_)
        | Record::EventBookFinished(_)
        | Record::EventBookMoved(_)
        | Record::EventBookRemoved(_)
        | Record::EventPluginsChanged(_)
        | Record::EventWifiChanged(_)
        | Record::EventPowerChanged(_)
        | Record::EventStatusChanged(_)
        | Record::EventChargeChanged(_)
        | Record::EventRegistryChanged(_)
        | Record::EventPreviewChanged(_)
        | Record::EventPluginsRequested(_)
        | Record::EventPluginAnswered(_)
        | Record::EventScreenOpened(_)
        | Record::EventScreenClosed(_)
        | Record::EventStackChanged(_)
        | Record::EventAnswered(_)
        | Record::EventWidgetMade(_)
        | Record::EventWidgetDropped(_)
        | Record::EventWidgetChanged(_)
        | Record::EventScreenReady(_)
        | Record::EventSpoken(_)
        | Record::EventImageReady(_)
        | Record::EventLibraryChanged(_)
        | Record::EventWifiScanned(_)
        | Record::EventWifiDone(_)
        | Record::EventNetworkWanted(_)
        | Record::EventNetworkIdle(_)
        | Record::EventFirmwareChanged(_)
        | Record::EventInstallDone(_)
        | Record::EventBluetooth(_)
        | Record::EventHttpDone(_)
        | Record::EventHttpProgress(_)
        | Record::EventServer(_)
        | Record::EventServedRequest(_)
        | Record::EventWorkDue(_)
        | Record::EventWorkRefused(_)
        | Record::EventWorkAllowed(_)
        | Record::EventSocket(_)
        | Record::EventBroadcast(_)
        | Record::EventDocumentClosed(_)
        | Record::EventTextTyped(_)
        | Record::EventDocumentChanged(_)
        | Record::EventLookupReady(_) => Err(ERR_INVALID),
        // Input records go to `PpPlugin.input`, frames to `draw`, format
        // arguments to slot 27.
        Record::DrawFrame(_)
        | Record::InputKey(_)
        | Record::InputPointer(_)
        | Record::InputGesture(_)
        | Record::InputAction(_)
        | Record::InputWidget(_)
        | Record::I18nArgStr(_)
        | Record::I18nArgInt(_)
        | Record::I18nCount(_) => Err(ERR_INVALID),
        // Service records are the host's answers (slots 17-19), never submitted.
        Record::ServiceUsage(_)
        | Record::ServicePluginUsage(_)
        | Record::ServicePlugin(_)
        | Record::ServiceWorkNote(_)
        | Record::ServiceEnd(_)
        | Record::ServiceEcho(_)
        | Record::ServiceRegQuery(_)
        | Record::ServiceRegKey(_)
        | Record::ServiceRegSetting(_)
        | Record::ServiceRegSettingExtra(_)
        | Record::ServiceRegChoice(_)
        | Record::ServiceRegText(_)
        | Record::ServiceRegGroup(_)
        | Record::ServiceRegPlugin(_)
        | Record::ServiceRegNeed(_)
        | Record::ServiceRegItem(_)
        | Record::ServiceRegSlot(_)
        | Record::ServiceRegMenuRow(_)
        | Record::ServiceRegBinding(_)
        | Record::ServiceRegSetupStep(_)
        | Record::ServiceRegValue(_)
        | Record::ServiceRegWidget(_)
        | Record::ServiceUiScreen(_)
        | Record::ServiceUiRequest(_)
        | Record::ServiceUiQuestion(_)
        | Record::ServiceUiHint(_)
        | Record::ServiceUiWidgetRef(_)
        | Record::ServiceUiInstance(_)
        | Record::ServiceUiWidgetInput(_)
        | Record::ServiceUiTarget(_)
        | Record::ServiceUiAsk(_)
        | Record::ServiceUiText(_)
        | Record::ServiceUiTextAt(_)
        | Record::ServiceUiAnswer(_)
        | Record::ServiceUiWidgetArea(_)
        | Record::ServiceUiSize(_)
        | Record::ServiceUiStatusAsk(_)
        | Record::ServiceUiStatusValue(_)
        | Record::ServiceUiParam(_)
        | Record::ServiceUiHandle(_)
        | Record::ServiceRegStatus(_)
        | Record::ServiceDocOpen(_)
        | Record::ServiceDocOption(_)
        | Record::ServiceDocState(_)
        | Record::ServiceDocRef(_)
        | Record::ServiceDocMeta(_)
        | Record::ServiceDocText(_)
        | Record::ServiceDocTextAt(_)
        | Record::ServiceDocPage(_)
        | Record::ServiceDocSection(_)
        | Record::ServiceDocAt(_)
        | Record::ServiceDocToc(_)
        | Record::ServiceDocIndex(_)
        | Record::ServiceDocLabel(_)
        | Record::ServiceDocXpointer(_)
        | Record::ServiceDocSpan(_)
        | Record::ServiceDocLink(_)
        | Record::ServiceDocHref(_)
        | Record::ServiceDocFont(_)
        | Record::ServiceDocFormat(_)
        | Record::ServiceDocPath(_)
        | Record::ServiceDocReader(_)
        | Record::ServiceDocProgress(_)
        | Record::ServiceDocReport(_)
        | Record::ServiceDocBlock(_)
        | Record::ServiceDocTextPart(_)
        | Record::ServiceDocColumns(_)
        | Record::ServiceDocRun(_)
        | Record::ServiceDocMap(_)
        | Record::ServiceDocCell(_)
        | Record::ServiceDocRuby(_)
        | Record::ServiceDocInlineLang(_)
        | Record::ServiceDocImage(_)
        | Record::ServiceDocImageAsk(_)
        | Record::ServiceLookupAsk(_)
        | Record::ServiceLookupEntry(_)
        | Record::ServiceLibState(_)
        | Record::ServiceLibQuery(_)
        | Record::ServiceLibEntry(_)
        | Record::ServiceLibRef(_)
        | Record::ServiceLibBook(_)
        | Record::ServiceLibTextAt(_)
        | Record::ServiceLibText(_)
        | Record::ServiceLibFacet(_)
        | Record::ServiceLibCover(_)
        | Record::ServiceLibStatus(_)
        | Record::ServiceLibPaths(_)
        | Record::ServiceLibResult(_)
        | Record::ServiceLibMove(_)
        | Record::ServiceLibImport(_)
        | Record::ServiceLibPosition(_)
        | Record::ServiceLibPositionSet(_)
        | Record::ServiceLibPage(_)
        | Record::ServiceLibAnnotation(_)
        | Record::ServiceLibAnchor(_)
        | Record::ServiceLibEditHead(_)
        | Record::ServiceLibTransaction(_)
        | Record::ServiceLibTransactionRef(_)
        | Record::ServiceLibEdit(_)
        | Record::ServiceLibEditText(_)
        | Record::ServiceLibCommitted(_)
        | Record::ServiceLibField(_)
        | Record::ServiceLibFieldSet(_)
        | Record::ServiceLibProvided(_)
        | Record::ServiceLibKey(_)
        | Record::ServiceLibReceiptAsk(_)
        | Record::ServiceLibReceipt(_)
        | Record::ServiceLibFieldPage(_)
        | Record::ServiceDevDevice(_)
        | Record::ServiceDevValue(_)
        | Record::ServiceDevFirmware(_)
        | Record::ServiceDevBuiltin(_)
        | Record::ServiceDevNative(_)
        | Record::ServiceDevCheck(_)
        | Record::ServiceDevMemory(_)
        | Record::ServiceDevWifi(_)
        | Record::ServiceDevNetwork(_)
        | Record::ServiceDevBluetooth(_)
        | Record::ServiceDevBonded(_)
        | Record::ServiceDevInterval(_)
        | Record::ServiceDevPower(_)
        | Record::ServiceDevRequest(_)
        | Record::ServiceDevInstall(_)
        | Record::ServiceDevNotInstalled(_)
        | Record::ServiceDevKey(_)
        | Record::ServiceFsPath(_)
        | Record::ServiceFsList(_)
        | Record::ServiceFsMove(_)
        | Record::ServiceFsEntry(_)
        | Record::ServiceFsSpace(_)
        | Record::ServiceFsProgress(_)
        | Record::ServiceNetRequest(_)
        | Record::ServiceNetHeader(_)
        | Record::ServiceNetFile(_)
        | Record::ServiceNetBody(_)
        | Record::ServiceNetStarted(_)
        | Record::ServiceNetToken(_)
        | Record::ServiceNetId(_)
        | Record::ServiceNetAnswer(_)
        | Record::ServiceNetRead(_)
        | Record::ServiceNetBytes(_)
        | Record::ServiceNetServe(_)
        | Record::ServiceNetResponse(_)
        | Record::ServiceSecName(_)
        | Record::ServiceSecValue(_)
        | Record::ServiceSecList(_)
        | Record::ServiceSecEntry(_)
        | Record::ServiceSecKeep(_)
        | Record::ServiceSecRandom(_)
        | Record::ServiceSecBytes(_)
        | Record::ServiceWorkSpec(_)
        | Record::ServiceWorkKey(_)
        | Record::ServiceWorkRow(_)
        | Record::ServiceSockConnect(_)
        | Record::ServiceSockPin(_)
        | Record::ServiceSockStarted(_)
        | Record::ServiceSockSend(_)
        | Record::ServiceSockRead(_)
        | Record::ServiceSockAt(_)
        | Record::ServiceSockBytes(_)
        | Record::ServiceSockTls(_)
        | Record::ServiceSockRef(_)
        | Record::ServiceSockPeer(_)
        | Record::ServiceSockBroadcast(_)
        | Record::ServiceSockPort(_) => Err(ERR_INVALID),
        // A stage call is the host's input to a subscriber, a reply its
        // answer to `stage.run`: never declared.
        Record::StageCall(_) | Record::StageReply(_) => Err(ERR_INVALID),
    }
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}

/// Dot-separated segments of `[a-z0-9-]`, none empty, at most 128 bytes.
pub(crate) fn full_id(id: &str) -> Check {
    ensure(
        id.len() <= FULL_ID_MAX_BYTES
            && id
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(is_name_byte)),
    )
}

/// A name relative to the plugin; the joined full id must also fit.
pub(crate) fn relative(plugin_id: &str, name: &str) -> Check {
    full_id(name)?;
    ensure(plugin_id.len() + 1 + name.len() <= FULL_ID_MAX_BYTES)
}

fn optional_full_id(id: &str) -> Check {
    if id.is_empty() { Ok(()) } else { full_id(id) }
}

fn group_check(plugin_id: &str, group: &SettingGroup<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let SettingGroup {
        name: _,
        title: _,
        order: _,
        page: _,
        parent: _,
    } = group;
    relative(plugin_id, group.name)?;
    ensure(!group.title.is_empty())?;
    if !group.page.is_empty() {
        relative(plugin_id, group.page)?;
    }
    optional_full_id(group.parent)
}

fn spec_check(plugin_id: &str, spec: &SettingSpec<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let SettingSpec {
        name: _,
        kind: _,
        flags: _,
        group: _,
        title: _,
        help: _,
        default: _,
        min: _,
        max: _,
        choices: _,
    } = spec;
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

/// 1 to 64 distinct, non-empty values of at most 96 bytes; `default` is one.
/// Refused as soon as a 65th value starts, so at most 64 values are compared.
fn choices_check(choices: &str, default: &str) -> Check {
    let mut count = 0;
    let mut found = false;
    for (index, value) in choices.split(SETTING_SPEC_CHOICES_SEPARATOR).enumerate() {
        ensure(index < CHOICES_MAX_VALUES)?;
        ensure(!value.is_empty() && value.len() <= CHOICE_MAX_BYTES)?;
        ensure(
            choices
                .split(SETTING_SPEC_CHOICES_SEPARATOR)
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
    // Every field named: a field added to the record must be checked here.
    let SettingExtra {
        name: _,
        unit: _,
        section: _,
        follows: _,
        shown_id: _,
        shown_value: _,
        editor_kind: _,
        editor: _,
        step: _,
        flags: _,
        check: _,
    } = extra;
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
    // Every field named: a field added to the record must be checked here.
    let ResourceHyphenation {
        language: _,
        path: _,
    } = record;
    ensure(language_tag(record.language, false))?;
    path_reference(record.path)
}

fn face_check(plugin_id: &str, face: &ResourceFontFace<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let ResourceFontFace {
        name: _,
        path: _,
        family: _,
        style: _,
        weight: _,
        role: _,
        priority: _,
    } = face;
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
    // Every field named: a field added to the record must be checked here.
    let ResourceFontExtension {
        name: _,
        path: _,
        extends: _,
    } = extension;
    relative(plugin_id, extension.name)?;
    path_reference(extension.path)?;
    // An extension adds glyphs to a face: only a PaperPaw pack can (PR #158).
    ensure(has_extension(extension.path, ".ppfp"))?;
    full_id(extension.extends)
}

/// `screen.spec` flag bit 0: full refresh when the screen opens.
pub const SCREEN_FULL_REFRESH_ON_OPEN: u32 = 1;
/// `screen.spec` flag bit 8: the screen shows no row of button hints.
pub const SCREEN_HINTS_HIDDEN: u32 = 1 << 8;
/// Every `screen.spec` flag bit this host knows.
const SCREEN_FLAGS_KNOWN: u32 = 0x1ff;
/// `action.spec` flag bit 0: runs only while one of the plugin's screens is on top.
pub const ACTION_NEEDS_SCREEN: u32 = 1;

fn screen_check(plugin_id: &str, screen: &ScreenSpec<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let ScreenSpec {
        name,
        title,
        token: _,
        flags,
    } = screen;
    relative(plugin_id, name)?;
    ensure(!title.is_empty())?;
    // Ready notification is not connected to the host help policy yet.
    if flags & !SCREEN_FLAGS_KNOWN != 0 || flags & (1 << 3) != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    if (flags >> 6) & 3 == 3 {
        return Err(ERR_UNSUPPORTED);
    }
    Ok(())
}

fn action_check(plugin_id: &str, action: &ActionSpec<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let ActionSpec {
        name,
        title,
        token: _,
        flags,
    } = action;
    relative(plugin_id, name)?;
    ensure(!title.is_empty())?;
    if flags & !ACTION_NEEDS_SCREEN != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    Ok(())
}

/// `menu.entry` places: by `order`, after/before another entry, in a section,
/// first, last. `anchor` names the entry or section for 1-3 and is empty else.
/// A place this host does not know is accepted and the entry is not shown, so a
/// plugin built for a later host still loads (as in interface 6).
pub const MENU_PLACE_ORDER: u32 = 0;
/// After the entry `anchor` (a full id).
pub const MENU_PLACE_AFTER: u32 = 1;
/// Before the entry `anchor` (a full id).
pub const MENU_PLACE_BEFORE: u32 = 2;
/// In the section `anchor` (a full id, a `menu.section`).
pub const MENU_PLACE_IN_SECTION: u32 = 3;
/// First in the menu.
pub const MENU_PLACE_FIRST: u32 = 4;
/// Last in the menu.
pub const MENU_PLACE_LAST: u32 = 5;

/// An item of a menu record: a full id, `<plugin>.<name>`. Its first segment
/// names the owning plugin; the caller's own items must resolve at commit,
/// another plugin's may come later (load order).
fn item_id(id: &str) -> Check {
    full_id(id)?;
    ensure(id.contains('.'))
}

/// The syntax of one `menu.entry`; whether `item` and `anchor` resolve is the
/// commit's.
pub(crate) fn menu_entry_check(entry: &MenuEntry<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let MenuEntry {
        menu,
        item,
        order,
        place,
        anchor,
    } = entry;
    item_id(menu)?;
    item_id(item)?;
    ensure(i16::try_from(*order).is_ok())?;
    match *place {
        MENU_PLACE_AFTER | MENU_PLACE_BEFORE | MENU_PLACE_IN_SECTION => item_id(anchor),
        MENU_PLACE_ORDER | MENU_PLACE_FIRST | MENU_PLACE_LAST => ensure(anchor.is_empty()),
        // A later place: kept and not shown; its anchor still has the syntax.
        _ => optional_item_id(anchor),
    }
}

fn optional_item_id(id: &str) -> Check {
    if id.is_empty() { Ok(()) } else { item_id(id) }
}

/// The syntax of one `icon.spec`: an icon name is one `[a-z0-9-]` segment. A
/// name the host's icon set lacks is not an error (the entry shows no icon).
/// Any item may get one (an icon theme); the last loaded plugin's wins.
pub(crate) fn icon_check(icon: &IconSpec<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let IconSpec { item, icon } = icon;
    item_id(item)?;
    ensure(
        !icon.is_empty()
            && icon.len() <= ICON_SPEC_ICON_MAX_BYTES
            && icon.bytes().all(is_name_byte),
    )
}

/// `menu.need` values: what a device must have for the item to be offered,
/// in the order of the SDK's `MenuNeed` (`docs/abi/screens-input.md` section 1).
/// A need this host does not know is accepted and not met.
pub const MENU_NEEDS: [&str; 13] = [
    "speech",
    "sleep",
    "rotation",
    "no-touch",
    "touch",
    "button-row",
    "colors",
    "wifi",
    "firmware",
    "black-and-white",
    "frontlight",
    "bluetooth",
    "has",
];
/// `menu.need` 12 (`has`): every bit of `capability` must be a capability the
/// device has; a bit this host does not know is not met.
pub const MENU_NEED_HAS: u32 = 12;

/// The capability bits `has` names, in bit order, as the SDK's `Capability`
/// (bit 0 first); a test keeps this list, the SDK and the contract text equal.
pub const MENU_NEED_CAPABILITIES: [&str; 17] = [
    "audio",
    "warmth",
    "charge-limit",
    "keyboard",
    "hot-code",
    "page-keys",
    "buttons",
    "usb-link",
    "render-hook",
    "render-regions",
    "look-extras",
    "usb-drive",
    "chords",
    "page-turner",
    "hotspot",
    "pen-tilt",
    "pen-hover",
];

/// The syntax of one `menu.need`; one need per item is the commit's rule.
pub(crate) fn need_check(need: &MenuNeed<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let MenuNeed {
        item,
        need,
        capability,
    } = need;
    item_id(item)?;
    need_value_check(*need, *capability)
}

pub(crate) fn need_value_check(need: u32, capability: u64) -> Check {
    let known = usize::try_from(need).is_ok_and(|need| need < MENU_NEEDS.len());
    // `capability` names the bits for `has` and is 0 for every other known
    // need; a later need may give it a meaning.
    ensure(!known || (need == MENU_NEED_HAS) == (capability != 0))
}

/// The syntax of one `menu.section`: a heading of the plugin's own in any menu.
pub(crate) fn section_check(plugin_id: &str, section: &MenuSection<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let MenuSection {
        name,
        title,
        menu,
        order,
    } = section;
    relative(plugin_id, name)?;
    ensure(!title.is_empty())?;
    item_id(menu)?;
    ensure(i16::try_from(*order).is_ok())
}

/// The syntax of one `menu.hide`: any plugin's entry may be taken out.
pub(crate) fn hide_check(hide: &MenuHide<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let MenuHide { menu, item } = hide;
    item_id(menu)?;
    item_id(item)
}

/// The syntax of one `menu.describe`: a line under one of the plugin's own
/// items, by its translation key.
pub(crate) fn describe_check(plugin_id: &str, describe: &MenuDescribe<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let MenuDescribe { item, key } = describe;
    relative(plugin_id, item)?;
    ensure(!key.is_empty())
}

/// Stage flag bits a host knows: `changes_layout`, `draws`, `takes_taps`.
const STAGE_FLAGS: u32 = 0b111;
/// Most records a subscriber's stage reply may hold.
const STAGE_MAX_RECORDS: u32 = 64;

/// The syntax of one `stage.spec`. Unknown flag bits are `ERR_UNSUPPORTED`
/// (a later host may know them), every other violation `ERR_INVALID`.
pub(crate) fn stage_spec_check(plugin_id: &str, spec: &StageSpec<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let StageSpec {
        name,
        schema: _,
        flags,
        max_input,
        max_reply,
        max_records,
    } = spec;
    stage_name(plugin_id, name)?;
    let payload = 1..=STAGE_CALL_PAYLOAD_MAX_BYTES as u32;
    ensure(payload.contains(max_input) && payload.contains(max_reply))?;
    ensure((1..=STAGE_MAX_RECORDS).contains(max_records))?;
    if flags & !STAGE_FLAGS != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    Ok(())
}

/// The syntax of one `registration.stage`: any plugin's stage, by full id.
pub(crate) fn stage_registration_check(registration: &RegistrationStage<'_>) -> Check {
    // Every field named: a field added to the record must be checked here.
    let RegistrationStage {
        stage,
        token: _,
        order,
        signature: _,
    } = registration;
    full_id(stage)?;
    // `<plugin>.<name>` splits at its last dot: both parts must be there, and
    // the name is one segment of at most `STAGE_NAME_MAX_BYTES`.
    let (plugin, name) = stage.rsplit_once('.').ok_or(ERR_INVALID)?;
    ensure(!plugin.is_empty() && !name.is_empty() && name.len() <= STAGE_NAME_MAX_BYTES)?;
    ensure(i16::try_from(*order).is_ok())
}

/// Longest stage name (one segment of the full id).
pub(crate) const STAGE_NAME_MAX_BYTES: usize = 64;

/// A stage's own name is exactly one id segment: `[a-z0-9-]`, 1..=64 bytes,
/// no dot, so the full id `<plugin>.<name>` splits at its last dot even though
/// plugin ids contain dots (`stages-services.md` section 1.1).
fn stage_name(plugin_id: &str, name: &str) -> Check {
    ensure(
        !name.is_empty()
            && name.len() <= STAGE_NAME_MAX_BYTES
            && name.bytes().all(is_name_byte)
            && plugin_id.len() + 1 + name.len() <= FULL_ID_MAX_BYTES,
    )
}

#[path = "declarations_docs.rs"]
mod docs;

#[cfg(test)]
#[path = "declarations_menu_tests.rs"]
mod menu_tests;

#[cfg(test)]
#[path = "declarations_stage_tests.rs"]
mod stage_tests;
