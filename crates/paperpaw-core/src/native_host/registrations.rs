//! Registration wire bridge for a loader's explicitly typed context.
//!
//! The loader installs its construction scope only around `make`. This module
//! never casts a foreign context or invokes plugin code. Committed declaration
//! bytes are retained as fallible host copies; submitted addresses are metadata.
//! A refused submission poisons construction even if the plugin ignores it.

#[path = "../abi7/records_wire.rs"]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    reason = "schema-generated bounded wire codec"
)]
mod wire;

pub use wire::{
    EVENT_TICK_TAG, REGISTRATION_MAX_BYTES, REGISTRATION_MAX_RECORDS, Registration,
    RegistrationBatch as DecodedRegistrations, registrations_decode, registrations_encode,
    tick_decode, tick_encode,
};
// Mixed registration and declaration batches: borrowed records with capped
// strings. `registrations_decode` still admits callback registrations only.
pub use wire::{
    EVENT_BATTERY_MEASURED_TAG, EVENT_PAGE_TURNED_CHAPTER_MAX_BYTES,
    EVENT_PAGE_TURNED_PATH_MAX_BYTES, EVENT_PAGE_TURNED_TAG, EVENT_PAGE_TURNED_TITLE_MAX_BYTES,
    EVENT_RESUME_TAG, EVENT_STARTED_TAG, EVENT_STOPPING_TAG, EVENT_SUSPEND_TAG,
    EventBatteryMeasured, EventPageTurned, EventResume, EventStarted, EventStopping, EventSuspend,
    Record, RecordBatch, RecordIter, ResourceFontExtension, ResourceFontFace, ResourceHyphenation,
    SETTING_SPEC_CHOICES_ITEM_MAX_BYTES, SETTING_SPEC_CHOICES_MAX_ITEMS,
    SETTING_SPEC_CHOICES_SEPARATOR, SettingExtra, SettingGroup, SettingSpec, records_decode,
    records_encode,
};
// Menu entries, icons, needs, sections, hides and descriptions (13-18,
// `docs/abi/screens-input.md` section 1).
pub use wire::{
    ICON_SPEC_ICON_MAX_BYTES, ICON_SPEC_TAG, IconSpec, MENU_DESCRIBE_TAG, MENU_ENTRY_TAG,
    MENU_HIDE_TAG, MENU_NEED_TAG, MENU_SECTION_TAG, MenuDescribe, MenuEntry, MenuHide, MenuNeed,
    MenuSection,
};
// The i18n formatting limits (slots 27 and 28, `docs/abi/screens-input.md`
// section 5): the host's format and digits entries bound their input by these.
pub use wire::{
    I18N_DIGITS_MAX_BYTES, I18N_FORMAT_MAX_ARGUMENT_BYTES, I18N_FORMAT_MAX_KEY_BYTES,
    I18N_FORMAT_MAX_RECORDS,
};
// Service records 48-52 and the bare framing of service blobs (slots 17-19,
// `docs/abi/proposals/interface-7-usage-clock.md`): built by the host, never
// submitted by a plugin.
pub use wire::{
    SERVICE_END_TAG, SERVICE_PLUGIN_NAME_MAX_BYTES, SERVICE_PLUGIN_TAG,
    SERVICE_PLUGIN_USAGE_AWAKE_REASON_MAX_BYTES, SERVICE_PLUGIN_USAGE_PLUGIN_MAX_BYTES,
    SERVICE_PLUGIN_USAGE_TAG, SERVICE_USAGE_TAG, SERVICE_WORK_NOTE_TAG,
    SERVICE_WORK_NOTE_TEXT_MAX_BYTES, ServiceEnd, ServicePlugin, ServicePluginUsage, ServiceUsage,
    ServiceWorkNote, record_decode_one, record_encode_one, record_framed_len,
};
// Screen/action declarations (11/12), screen events (38/39), input records
// (40-43), i18n format arguments (44-46) and the draw frame (192):
// `docs/abi/screens-input.md`.
pub use wire::{
    ACTION_SPEC_TAG, ActionSpec, DRAW_FRAME_PAYLOAD_MAX_BYTES, DRAW_FRAME_TAG, DrawFrame,
    EVENT_ACTION_RUN_TAG, EVENT_SCREEN_SHOWN_TAG, EventActionRun, EventScreenShown,
    I18N_ARG_INT_TAG, I18N_ARG_STR_TAG, I18N_COUNT_TAG, I18nArgInt, I18nArgStr, I18nCount,
    INPUT_ACTION_TAG, INPUT_GESTURE_TAG, INPUT_KEY_TAG, INPUT_POINTER_TAG, InputAction,
    InputGesture, InputKey, InputPointer, SCREEN_SPEC_TAG, ScreenSpec,
};
// Stages (19, 20, 512, 513) and the test-only service record 1024
// (`docs/abi/stages-services.md`).
pub use wire::{
    REGISTRATION_STAGE_TAG, RegistrationStage, SERVICE_ECHO_TAG, STAGE_CALL_PAYLOAD_MAX_BYTES,
    STAGE_CALL_TAG, STAGE_REPLY_PAYLOAD_MAX_BYTES, STAGE_REPLY_TAG, STAGE_SPEC_TAG, ServiceEcho,
    StageCall, StageReply, StageSpec,
};
// Core events of family 1b (64-74, `docs/abi/core-events.md`); hosts admit
// their registrations once a producer is wired.
pub use wire::{
    EVENT_BOOK_CLOSED_TAG, EVENT_BOOK_FINISHED_TAG, EVENT_BOOK_MOVED_TAG, EVENT_BOOK_OPENED_TAG,
    EVENT_BOOK_REMOVED_TAG, EVENT_CHARGE_CHANGED_TAG, EVENT_PLUGINS_CHANGED_TAG,
    EVENT_POWER_CHANGED_TAG, EVENT_SETTING_CHANGED_TAG, EVENT_STATUS_CHANGED_TAG,
    EVENT_WIFI_CHANGED_TAG, EventBookClosed, EventBookFinished, EventBookMoved, EventBookOpened,
    EventBookRemoved, EventChargeChanged, EventPluginsChanged, EventPowerChanged,
    EventSettingChanged, EventStatusChanged, EventWifiChanged,
};
// Registry (REG, `docs/abi/registry.md`): declarations 21-28, events 75-78 and
// the `core.registry` rows 1025-1040; the host admits and answers them once
// wired (Core hand-off).
pub use wire::{
    EVENT_PLUGIN_ANSWERED_TAG, EVENT_PLUGINS_REQUESTED_PLUGINS_ITEM_MAX_BYTES,
    EVENT_PLUGINS_REQUESTED_PLUGINS_MAX_ITEMS, EVENT_PLUGINS_REQUESTED_PLUGINS_SEPARATOR,
    EVENT_PLUGINS_REQUESTED_TAG, EVENT_PREVIEW_CHANGED_TAG, EVENT_REGISTRY_CHANGED_TAG,
    EventPluginAnswered, EventPluginsRequested, EventPreviewChanged, EventRegistryChanged,
    KEY_BIND_TAG, KeyBind, PRESET_SPEC_TAG, PRESET_VALUE_TAG, PresetSpec, PresetValue,
    SERVICE_REG_BINDING_TAG, SERVICE_REG_CHOICE_TAG, SERVICE_REG_GROUP_TAG, SERVICE_REG_ITEM_TAG,
    SERVICE_REG_KEY_TAG, SERVICE_REG_MENU_ROW_TAG, SERVICE_REG_NEED_TAG, SERVICE_REG_PLUGIN_TAG,
    SERVICE_REG_QUERY_TAG, SERVICE_REG_SETTING_EXTRA_TAG, SERVICE_REG_SETTING_TAG,
    SERVICE_REG_SETUP_STEP_TAG, SERVICE_REG_SLOT_TAG, SERVICE_REG_TEXT_TAG, SERVICE_REG_VALUE_TAG,
    SERVICE_REG_WIDGET_TAG, SETTING_CHOICE_TAG, SETTING_DEFAULT_TAG, SETUP_STEP_TAG,
    SLOT_OFFER_TAG, SLOT_SPEC_TAG, ServiceRegBinding, ServiceRegChoice, ServiceRegGroup,
    ServiceRegItem, ServiceRegKey, ServiceRegMenuRow, ServiceRegNeed, ServiceRegPlugin,
    ServiceRegQuery, ServiceRegSetting, ServiceRegSettingExtra, ServiceRegSetupStep,
    ServiceRegSlot, ServiceRegText, ServiceRegValue, ServiceRegWidget, SettingChoice,
    SettingDefault, SetupStep, SlotOffer, SlotSpec,
};
// Single-record and streaming decoders for plugin-side readers (lock format 8).
pub use wire::{RECORD_CARRY_MAX_BYTES, RecordStream, record_decode_prefix, records_decode_single};

use super::{Error, registration_scope::RegistrationScope};
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE, ERR_UNSUPPORTED, OK, PpBytes,
    buffers,
};

/// Decode and validate an entire borrowed batch, then stage it: callback
/// registrations and declaration records (tags 5-10) together, all or nothing.
/// All records remain private until the loader commits validated `make` output.
/// Every decode, validation, unsupported-event or staging refusal poisons that
/// transaction; the loader must abort it even when the foreign caller ignores
/// this status. The plugin id for name checks is the one given to `begin`.
/// Bounded staging copies include raw declaration bytes. The submitted address
/// is retained only as metadata for later image admission, never for reading.
///
/// # Safety
/// When non-null with nonzero length, `bytes` must designate initialized storage
/// readable for `len` bytes, stable and unmodified until this call returns.
/// The caller must use its own pinned context's scope, on the dispatch thread.
/// Host state cannot be mutated through an alias to the input during this call.
#[must_use]
pub unsafe fn submit(scope: &RegistrationScope<Registration>, bytes: *const u8, len: usize) -> i32 {
    if let Err(error) = scope.admission() {
        return refuse(scope, status(error));
    }
    let wire = if len > REGISTRATION_MAX_BYTES {
        Err(ERR_NO_SPACE)
    } else {
        // SAFETY: caller guarantees readable, stable storage; the helper also
        // checks null and size bounds before constructing any nonempty slice.
        unsafe { buffers::bytes_in(PpBytes { ptr: bytes, len }, REGISTRATION_MAX_BYTES) }
    };
    let wire = match wire {
        Ok(wire) => wire,
        Err(error) => return refuse(scope, error),
    };
    let batch = match wire::records_decode(wire) {
        Ok(batch) => batch,
        Err(error) => return refuse(scope, error),
    };
    // Only events with a host encoder can be named; refuse before staging anything.
    let supported = batch.iter().all(|record| match record {
        Record::Registration(Registration::Event { event_tag, .. }) => {
            super::events::is_event_tag(event_tag)
        }
        _ => true,
    });
    if !supported {
        return refuse(scope, ERR_UNSUPPORTED);
    }
    match scope.submit_wire(&batch, |registration| registration, wire) {
        Ok(()) => OK,
        // The construction latched itself; this also covers an overlapping call.
        Err(error) => refuse(scope, error),
    }
}

fn refuse(scope: &RegistrationScope<Registration>, error: i32) -> i32 {
    // A missing scope cannot be poisoned and is a lifecycle error regardless
    // of the bytes supplied. Active scopes latch before the status is returned.
    match scope.reject() {
        Ok(()) => error,
        Err(error) => status(error),
    }
}

pub(super) fn status(error: Error) -> i32 {
    match error {
        Error::State => ERR_STATE,
        Error::Busy | Error::Depth => ERR_BUSY,
        Error::Capacity => ERR_NO_SPACE,
        Error::Invalid => ERR_INVALID,
        Error::Memory => ERR_NO_MEMORY,
        Error::DeclarationViewsUnavailable => ERR_UNSUPPORTED,
    }
}
