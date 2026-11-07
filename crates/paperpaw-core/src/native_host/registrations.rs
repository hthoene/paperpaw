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
// Single-record and streaming decoders for plugin-side readers (lock format 8).
pub use wire::{RECORD_CARRY_MAX_BYTES, RecordStream, record_decode_prefix, records_decode_single};
// The host's decoders (lock format 10, #985): only records a plugin sends. The
// host decodes through these alone, so a device links no decoder for events,
// input, frames or service replies.
pub use wire::{host_decode_one, host_decode_prefix, host_records_decode_into};

/// A plugin's batch as the host decodes it ([`host_records_decode_into`]),
/// returned by value. A plugin-reachable slot holds its batch on the heap
/// instead ([`submit`]).
///
/// # Errors
/// The statuses of [`records_decode`]; a record only the host sends is
/// `ERR_INVALID`.
pub fn host_records_decode(bytes: &[u8]) -> Result<RecordBatch<'_>, i32> {
    let mut batch = RecordBatch::EMPTY;
    host_records_decode_into(bytes, &mut batch)?;
    Ok(batch)
}

use super::{Error, registration_scope::RegistrationScope};
use alloc::vec::Vec;
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
    // The decoded batch (8 records, 840 bytes on riscv32) lives on the heap:
    // this slot runs on a plugin's stack band, whose frames stay under 2 KiB
    // (#953, #985).
    let mut held = Vec::new();
    if held.try_reserve_exact(1).is_err() {
        return refuse_memory(scope);
    }
    held.push(RecordBatch::EMPTY);
    let Some(batch) = held.first_mut() else {
        return refuse_memory(scope);
    };
    if let Err(error) = wire::host_records_decode_into(wire, batch) {
        return refuse(scope, error);
    }
    let batch = &*batch;
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
    match scope.submit_wire(batch, |registration| registration, wire) {
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

/// Refuse for lack of memory and latch Memory, not Invalid, as the cause.
fn refuse_memory(scope: &RegistrationScope<Registration>) -> i32 {
    match scope.reject_with(Error::Memory) {
        Ok(()) => ERR_NO_MEMORY,
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
