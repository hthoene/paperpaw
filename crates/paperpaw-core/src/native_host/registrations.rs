//! Registration wire bridge for a loader's explicitly typed context.
//!
//! The loader installs its construction scope only around `make`. This module
//! never casts a foreign context or invokes plugin code. Committed declaration
//! bytes are retained as fallible host copies; submitted addresses are metadata.
//! A refused submission poisons construction even if the plugin ignores it.

#[path = "registrations/subset.rs"]
mod subset;
#[path = "../abi7/records_wire.rs"]
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    dead_code,
    reason = "schema-generated bounded wire codec; the host does not use the plugins' typed codec"
)]
mod wire;
#[cfg(test)]
pub(super) use subset::decode_any;
pub(super) use subset::{as_wire, decode_record, unexpected};
/// The size of a wire varint (the lookup broker finds fields behind one).
pub(crate) use wire::varint_len;

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
    EVENT_ACTION_RUN_TAG, EVENT_SCREEN_SHOWN_TAG, EVENT_TEXT_TYPED_TEXT_MAX_BYTES, EventActionRun,
    EventScreenShown, EventTextTyped, I18N_ARG_INT_TAG, I18N_ARG_STR_TAG, I18N_COUNT_TAG,
    I18nArgInt, I18nArgStr, I18nCount, INPUT_ACTION_TAG, INPUT_GESTURE_TAG, INPUT_KEY_TAG,
    INPUT_POINTER_TAG, InputAction, InputGesture, InputKey, InputPointer, SCREEN_SPEC_TAG,
    ScreenSpec,
};
// Stages (19, 20, 512, 513) and the test-only service record 1024
// (`docs/abi/stages-services.md`).
pub(crate) use wire::EVENT_SETTING_CHANGED_OLD_MAX_BYTES;
pub(crate) use wire::{EVENT_BOOK_MOVED_FROM_MAX_BYTES, EVENT_BOOK_MOVED_PATH_MAX_BYTES};
pub use wire::{
    REGISTRATION_STAGE_TAG, RegistrationStage, SERVICE_ECHO_TAG, STAGE_CALL_PAYLOAD_MAX_BYTES,
    STAGE_CALL_TAG, STAGE_REPLY_PAYLOAD_MAX_BYTES, STAGE_REPLY_SUBSCRIBER_MAX_BYTES,
    STAGE_REPLY_TAG, STAGE_SPEC_TAG, ServiceEcho, StageCall, StageReply, StageSpec,
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
// the `core.registry` rows 1025-1040 and 1059; the host admits and answers them once
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
    SERVICE_REG_SETUP_STEP_TAG, SERVICE_REG_SLOT_TAG, SERVICE_REG_STATUS_TAG, SERVICE_REG_TEXT_TAG,
    SERVICE_REG_VALUE_TAG, SERVICE_REG_WIDGET_TAG, SETTING_CHOICE_TAG, SETTING_DEFAULT_TAG,
    SETUP_STEP_TAG, SLOT_OFFER_TAG, SLOT_SPEC_TAG, ServiceRegBinding, ServiceRegChoice,
    ServiceRegGroup, ServiceRegItem, ServiceRegKey, ServiceRegMenuRow, ServiceRegNeed,
    ServiceRegPlugin, ServiceRegQuery, ServiceRegSetting, ServiceRegSettingExtra,
    ServiceRegSetupStep, ServiceRegSlot, ServiceRegStatus, ServiceRegText, ServiceRegValue,
    ServiceRegWidget, SettingChoice, SettingDefault, SetupStep, SlotOffer, SlotSpec,
};
// Screens, dialogs and composition (UIX, `docs/abi/ui.md`): declarations 29-30
// and 224-228, events 79-89, input 193 and the `core.ui` rows 1041-1058; the
// host admits and answers them once wired (UIX hand-off).
pub use wire::{
    EVENT_ANSWERED_TAG, EVENT_SCREEN_CLOSED_TAG, EVENT_SCREEN_OPENED_TAG, EVENT_SCREEN_READY_TAG,
    EVENT_SPOKEN_TAG, EVENT_STACK_CHANGED_TAG, EVENT_WIDGET_CHANGED_TAG, EVENT_WIDGET_DROPPED_TAG,
    EVENT_WIDGET_MADE_TAG, EventAnswered, EventScreenClosed, EventScreenOpened, EventScreenReady,
    EventSpoken, EventStackChanged, EventWidgetChanged, EventWidgetDropped, EventWidgetMade,
    HELP_HINT_TAG, HelpHint, INPUT_SPEC_TAG, INPUT_WIDGET_TAG, InputSpec, InputWidget,
    SERVICE_UI_ANSWER_TAG, SERVICE_UI_ASK_CHOICES_ITEM_MAX_BYTES, SERVICE_UI_ASK_CHOICES_MAX_ITEMS,
    SERVICE_UI_ASK_CHOICES_SEPARATOR, SERVICE_UI_ASK_TAG, SERVICE_UI_HANDLE_TAG,
    SERVICE_UI_HINT_TAG, SERVICE_UI_INSTANCE_TAG, SERVICE_UI_PARAM_TAG,
    SERVICE_UI_QUESTION_CHOICES_ITEM_MAX_BYTES, SERVICE_UI_QUESTION_CHOICES_MAX_ITEMS,
    SERVICE_UI_QUESTION_CHOICES_SEPARATOR, SERVICE_UI_QUESTION_TAG, SERVICE_UI_REQUEST_TAG,
    SERVICE_UI_SCREEN_OWNER_MAX_BYTES, SERVICE_UI_SCREEN_SCREEN_MAX_BYTES,
    SERVICE_UI_SCREEN_SLOT_MAX_BYTES, SERVICE_UI_SCREEN_TAG, SERVICE_UI_SIZE_TAG,
    SERVICE_UI_STATUS_ASK_ITEMS_ITEM_MAX_BYTES, SERVICE_UI_STATUS_ASK_ITEMS_MAX_ITEMS,
    SERVICE_UI_STATUS_ASK_ITEMS_SEPARATOR, SERVICE_UI_STATUS_ASK_TAG,
    SERVICE_UI_STATUS_VALUE_ITEM_MAX_BYTES, SERVICE_UI_STATUS_VALUE_TAG, SERVICE_UI_TARGET_TAG,
    SERVICE_UI_TEXT_AT_TAG, SERVICE_UI_TEXT_TAG, SERVICE_UI_WIDGET_AREA_TAG,
    SERVICE_UI_WIDGET_INPUT_TAG, SERVICE_UI_WIDGET_REF_TAG, STATUS_ITEM_TAG, STATUS_LINE_TAG,
    STATUS_PLACE_TAG, ServiceUiAnswer, ServiceUiAsk, ServiceUiHandle, ServiceUiHint,
    ServiceUiInstance, ServiceUiParam, ServiceUiQuestion, ServiceUiRequest, ServiceUiScreen,
    ServiceUiSize, ServiceUiStatusAsk, ServiceUiStatusValue, ServiceUiTarget, ServiceUiText,
    ServiceUiTextAt, ServiceUiWidgetArea, ServiceUiWidgetInput, ServiceUiWidgetRef, StatusItem,
    StatusLine, StatusPlace, WIDGET_PLACE_LIST_KEYS_ITEM_MAX_BYTES,
    WIDGET_PLACE_LIST_KEYS_MAX_ITEMS, WIDGET_PLACE_LIST_KEYS_SEPARATOR, WIDGET_PLACE_TAG,
    WIDGET_SPEC_TAG, WidgetPlace, WidgetSpec,
};
// Documents, formats, lookup and the library (DOC and LIB, `docs/abi/document.md`,
// `library.md`): declarations, events 90-91 and the `core.document`, `core.lookup` and
// `core.library` rows 1060-1124; the host admits and answers them once wired.
pub use wire::{
    BOOK_BADGE_SPEC_TAG, BOOK_DATA_SPEC_TAG, BOOK_FIELD_SPEC_TAG, BookBadgeSpec, BookDataSpec,
    BookFieldSpec, EVENT_IMAGE_READY_TAG, EVENT_LIBRARY_CHANGED_TAG, EVENT_LOOKUP_READY_TAG,
    EventImageReady, EventLibraryChanged, EventLookupReady, FORMAT_SPEC_EXTENSIONS_ITEM_MAX_BYTES,
    FORMAT_SPEC_EXTENSIONS_MAX_ITEMS, FORMAT_SPEC_EXTENSIONS_SEPARATOR, FORMAT_SPEC_TAG,
    FormatSpec, LOOKUP_SPEC_LANGUAGES_ITEM_MAX_BYTES, LOOKUP_SPEC_LANGUAGES_MAX_ITEMS,
    LOOKUP_SPEC_LANGUAGES_SEPARATOR, LOOKUP_SPEC_TAG, LookupSpec,
    READER_SPEC_FORMATS_ITEM_MAX_BYTES, READER_SPEC_FORMATS_MAX_ITEMS,
    READER_SPEC_FORMATS_SEPARATOR, READER_SPEC_TAG, ReaderSpec, SERVICE_DOC_AT_TAG,
    SERVICE_DOC_BLOCK_TAG, SERVICE_DOC_CELL_TAG, SERVICE_DOC_COLUMNS_TAG, SERVICE_DOC_FONT_TAG,
    SERVICE_DOC_FORMAT_EXTENSIONS_ITEM_MAX_BYTES, SERVICE_DOC_FORMAT_EXTENSIONS_MAX_ITEMS,
    SERVICE_DOC_FORMAT_EXTENSIONS_SEPARATOR, SERVICE_DOC_FORMAT_TAG, SERVICE_DOC_HREF_TAG,
    SERVICE_DOC_IMAGE_ASK_TAG, SERVICE_DOC_IMAGE_TAG, SERVICE_DOC_INDEX_TAG,
    SERVICE_DOC_INLINE_LANG_TAG, SERVICE_DOC_LABEL_TAG, SERVICE_DOC_LINK_TAG, SERVICE_DOC_MAP_TAG,
    SERVICE_DOC_META_TAG, SERVICE_DOC_OPEN_TAG, SERVICE_DOC_OPTION_TAG, SERVICE_DOC_PAGE_TAG,
    SERVICE_DOC_PATH_TAG, SERVICE_DOC_PROGRESS_TAG, SERVICE_DOC_READER_FORMATS_ITEM_MAX_BYTES,
    SERVICE_DOC_READER_FORMATS_MAX_ITEMS, SERVICE_DOC_READER_FORMATS_SEPARATOR,
    SERVICE_DOC_READER_TAG, SERVICE_DOC_REF_TAG, SERVICE_DOC_REPORT_TAG, SERVICE_DOC_RUBY_TAG,
    SERVICE_DOC_RUN_TAG, SERVICE_DOC_SECTION_TAG, SERVICE_DOC_SPAN_TAG, SERVICE_DOC_STATE_TAG,
    SERVICE_DOC_TEXT_AT_TAG, SERVICE_DOC_TEXT_PART_TAG, SERVICE_DOC_TEXT_TAG, SERVICE_DOC_TOC_TAG,
    SERVICE_DOC_XPOINTER_TAG, SERVICE_LIB_ANCHOR_TAG, SERVICE_LIB_ANNOTATION_TAG,
    SERVICE_LIB_BOOK_TAG, SERVICE_LIB_COMMITTED_TAG, SERVICE_LIB_COVER_TAG,
    SERVICE_LIB_EDIT_HEAD_TAG, SERVICE_LIB_EDIT_TAG, SERVICE_LIB_EDIT_TEXT_TAG,
    SERVICE_LIB_ENTRY_BADGES_ITEM_MAX_BYTES, SERVICE_LIB_ENTRY_BADGES_MAX_ITEMS,
    SERVICE_LIB_ENTRY_BADGES_SEPARATOR, SERVICE_LIB_ENTRY_TAG, SERVICE_LIB_FACET_TAG,
    SERVICE_LIB_FIELD_SET_TAG, SERVICE_LIB_FIELD_TAG, SERVICE_LIB_IMPORT_TAG, SERVICE_LIB_KEY_TAG,
    SERVICE_LIB_MOVE_TAG, SERVICE_LIB_PAGE_TAG, SERVICE_LIB_PATHS_PATHS_ITEM_MAX_BYTES,
    SERVICE_LIB_PATHS_PATHS_MAX_ITEMS, SERVICE_LIB_PATHS_PATHS_SEPARATOR, SERVICE_LIB_PATHS_TAG,
    SERVICE_LIB_POSITION_SET_TAG, SERVICE_LIB_POSITION_TAG, SERVICE_LIB_PROVIDED_TAG,
    SERVICE_LIB_PROVIDED_TAGS_ITEM_MAX_BYTES, SERVICE_LIB_PROVIDED_TAGS_MAX_ITEMS,
    SERVICE_LIB_PROVIDED_TAGS_SEPARATOR, SERVICE_LIB_QUERY_TAG, SERVICE_LIB_RECEIPT_ASK_TAG,
    SERVICE_LIB_RECEIPT_TAG, SERVICE_LIB_REF_TAG, SERVICE_LIB_RESULT_TAG, SERVICE_LIB_STATE_TAG,
    SERVICE_LIB_STATUS_TAG, SERVICE_LIB_TEXT_AT_TAG, SERVICE_LIB_TEXT_TAG,
    SERVICE_LIB_TRANSACTION_REF_TAG, SERVICE_LIB_TRANSACTION_TAG, SERVICE_LOOKUP_ASK_TAG,
    SERVICE_LOOKUP_ENTRY_TAG, ServiceDocAt, ServiceDocBlock, ServiceDocCell, ServiceDocColumns,
    ServiceDocFont, ServiceDocFormat, ServiceDocHref, ServiceDocImage, ServiceDocImageAsk,
    ServiceDocIndex, ServiceDocInlineLang, ServiceDocLabel, ServiceDocLink, ServiceDocMap,
    ServiceDocMeta, ServiceDocOpen, ServiceDocOption, ServiceDocPage, ServiceDocPath,
    ServiceDocProgress, ServiceDocReader, ServiceDocRef, ServiceDocReport, ServiceDocRuby,
    ServiceDocRun, ServiceDocSection, ServiceDocSpan, ServiceDocState, ServiceDocText,
    ServiceDocTextAt, ServiceDocTextPart, ServiceDocToc, ServiceDocXpointer, ServiceLibAnchor,
    ServiceLibAnnotation, ServiceLibBook, ServiceLibCommitted, ServiceLibCover, ServiceLibEdit,
    ServiceLibEditHead, ServiceLibEditText, ServiceLibEntry, ServiceLibFacet, ServiceLibField,
    ServiceLibFieldPage, ServiceLibFieldSet, ServiceLibImport, ServiceLibKey, ServiceLibMove,
    ServiceLibPage, ServiceLibPaths, ServiceLibPosition, ServiceLibPositionSet, ServiceLibProvided,
    ServiceLibQuery, ServiceLibReceipt, ServiceLibReceiptAsk, ServiceLibRef, ServiceLibResult,
    ServiceLibState, ServiceLibStatus, ServiceLibText, ServiceLibTextAt, ServiceLibTransaction,
    ServiceLibTransactionRef, ServiceLookupAsk, ServiceLookupEntry,
};
// The device (DEV, `docs/abi/device.md`): events 92-98 and the `core.device` rows
// 1125-1141; the host answers them once wired (its commands 1564-1592 are in the
// command codec).
pub use wire::{
    EVENT_BLUETOOTH_TAG, EVENT_FIRMWARE_CHANGED_TAG, EVENT_INSTALL_DONE_TAG,
    EVENT_NETWORK_IDLE_TAG, EVENT_NETWORK_WANTED_TAG, EVENT_WIFI_DONE_TAG, EVENT_WIFI_SCANNED_TAG,
    EventBluetooth, EventFirmwareChanged, EventInstallDone, EventNetworkIdle, EventNetworkWanted,
    EventWifiDone, EventWifiScanned, SERVICE_DEV_BLUETOOTH_TAG, SERVICE_DEV_BONDED_TAG,
    SERVICE_DEV_BUILTIN_TAG, SERVICE_DEV_CHECK_TAG, SERVICE_DEV_DEVICE_TAG,
    SERVICE_DEV_FIRMWARE_TAG, SERVICE_DEV_INSTALL_TAG, SERVICE_DEV_INTERVAL_TAG,
    SERVICE_DEV_KEY_TAG, SERVICE_DEV_MEMORY_TAG, SERVICE_DEV_NATIVE_TAG, SERVICE_DEV_NETWORK_TAG,
    SERVICE_DEV_NOT_INSTALLED_TAG, SERVICE_DEV_POWER_TAG, SERVICE_DEV_REQUEST_TAG,
    SERVICE_DEV_VALUE_TAG, SERVICE_DEV_WIFI_TAG, ServiceDevBluetooth, ServiceDevBonded,
    ServiceDevBuiltin, ServiceDevCheck, ServiceDevDevice, ServiceDevFirmware, ServiceDevInstall,
    ServiceDevInterval, ServiceDevKey, ServiceDevMemory, ServiceDevNative, ServiceDevNetwork,
    ServiceDevNotInstalled, ServiceDevPower, ServiceDevRequest, ServiceDevValue, ServiceDevWifi,
};
// Field caps of the `core.device` replies (`device.md` 2), for the host's clipping.
pub use wire::{
    SERVICE_DEV_BONDED_GIVEN_MAX_BYTES, SERVICE_DEV_BONDED_NAME_MAX_BYTES,
    SERVICE_DEV_BUILTIN_ID_MAX_BYTES, SERVICE_DEV_BUILTIN_VERSION_MAX_BYTES,
    SERVICE_DEV_CHECK_ID_MAX_BYTES, SERVICE_DEV_CHECK_SIGNER_MAX_BYTES,
    SERVICE_DEV_CHECK_VERSION_MAX_BYTES, SERVICE_DEV_DEVICE_CATALOG_MAX_BYTES,
    SERVICE_DEV_DEVICE_ID_MAX_BYTES, SERVICE_DEV_DEVICE_MODEL_MAX_BYTES,
    SERVICE_DEV_FIRMWARE_PROJECT_MAX_BYTES, SERVICE_DEV_FIRMWARE_VERSION_MAX_BYTES,
    SERVICE_DEV_KEY_NAME_MAX_BYTES, SERVICE_DEV_NATIVE_CODE_FILE_MAX_BYTES,
    SERVICE_DEV_NATIVE_CRASHED_MAX_BYTES, SERVICE_DEV_NATIVE_LAST_LOADED_MAX_BYTES,
    SERVICE_DEV_NETWORK_SSID_MAX_BYTES, SERVICE_DEV_NOT_INSTALLED_PLUGIN_MAX_BYTES,
    SERVICE_DEV_POWER_MODE_MAX_BYTES, SERVICE_DEV_VALUE_NAME_MAX_BYTES,
    SERVICE_DEV_WIFI_ADDRESS_MAX_BYTES, SERVICE_DEV_WIFI_HOST_NAME_MAX_BYTES,
    SERVICE_DEV_WIFI_SSID_MAX_BYTES,
};
// Folders and files (FS, `docs/abi/files.md`): the `core.files` rows 1142-1147;
// the host answers them once wired.
pub use wire::{
    SERVICE_FS_ENTRY_NAME_MAX_BYTES, SERVICE_FS_ENTRY_TAG, SERVICE_FS_LIST_PATH_MAX_BYTES,
    SERVICE_FS_LIST_TAG, SERVICE_FS_MOVE_FROM_MAX_BYTES, SERVICE_FS_MOVE_TAG,
    SERVICE_FS_MOVE_TO_MAX_BYTES, SERVICE_FS_PATH_PATH_MAX_BYTES, SERVICE_FS_PATH_TAG,
    SERVICE_FS_PROGRESS_TAG, SERVICE_FS_SPACE_TAG, ServiceFsEntry, ServiceFsList, ServiceFsMove,
    ServiceFsPath, ServiceFsProgress, ServiceFsSpace,
};
// The network, secrets, work and sockets (NET, SEC, ASY, SOCK: `network.md`, `secrets.md`,
// `work.md`, `sockets.md`): events 99-107, the reader's `event.document_closed` (108,
// `document.md`) and the service rows 1148-1181; the host answers them once wired.
pub use wire::{
    EVENT_BROADCAST_FROM_MAX_BYTES, EVENT_BROADCAST_TAG, EVENT_DOCUMENT_CHANGED_TAG,
    EVENT_DOCUMENT_CLOSED_TAG, EVENT_HTTP_DONE_TAG, EVENT_HTTP_PROGRESS_TAG,
    EVENT_SERVED_REQUEST_METHOD_MAX_BYTES, EVENT_SERVED_REQUEST_TAG,
    EVENT_SERVED_REQUEST_TARGET_MAX_BYTES, EVENT_SERVER_ADDRESS_MAX_BYTES, EVENT_SERVER_TAG,
    EVENT_SOCKET_TAG, EVENT_WORK_ALLOWED_TAG, EVENT_WORK_DUE_KEY_MAX_BYTES, EVENT_WORK_DUE_TAG,
    EVENT_WORK_REFUSED_KEY_MAX_BYTES, EVENT_WORK_REFUSED_TAG, EventBroadcast, EventDocumentChanged,
    EventDocumentClosed, EventHttpDone, EventHttpProgress, EventServedRequest, EventServer,
    EventSocket, EventWorkAllowed, EventWorkDue, EventWorkRefused,
    SERVICE_NET_ANSWER_CONTENT_TYPE_MAX_BYTES, SERVICE_NET_ANSWER_ETAG_MAX_BYTES,
    SERVICE_NET_ANSWER_FINAL_URL_MAX_BYTES, SERVICE_NET_ANSWER_TAG,
    SERVICE_NET_BODY_DATA_MAX_BYTES, SERVICE_NET_BODY_TAG, SERVICE_NET_BYTES_DATA_MAX_BYTES,
    SERVICE_NET_BYTES_TAG, SERVICE_NET_FILE_PATH_MAX_BYTES, SERVICE_NET_FILE_TAG,
    SERVICE_NET_HEADER_NAME_MAX_BYTES, SERVICE_NET_HEADER_TAG, SERVICE_NET_HEADER_VALUE_MAX_BYTES,
    SERVICE_NET_ID_TAG, SERVICE_NET_READ_TAG, SERVICE_NET_REQUEST_METHOD_MAX_BYTES,
    SERVICE_NET_REQUEST_TAG, SERVICE_NET_REQUEST_URL_MAX_BYTES,
    SERVICE_NET_RESPONSE_CONTENT_TYPE_MAX_BYTES, SERVICE_NET_RESPONSE_TAG, SERVICE_NET_SERVE_TAG,
    SERVICE_NET_STARTED_TAG, SERVICE_NET_TOKEN_TAG, SERVICE_SEC_BYTES_DATA_MAX_BYTES,
    SERVICE_SEC_BYTES_TAG, SERVICE_SEC_ENTRY_LABEL_MAX_BYTES, SERVICE_SEC_ENTRY_NAME_MAX_BYTES,
    SERVICE_SEC_ENTRY_PLUGIN_MAX_BYTES, SERVICE_SEC_ENTRY_TAG, SERVICE_SEC_KEEP_LABEL_MAX_BYTES,
    SERVICE_SEC_KEEP_NAME_MAX_BYTES, SERVICE_SEC_KEEP_PLUGIN_MAX_BYTES, SERVICE_SEC_KEEP_TAG,
    SERVICE_SEC_KEEP_VALUE_MAX_BYTES, SERVICE_SEC_LIST_TAG, SERVICE_SEC_NAME_NAME_MAX_BYTES,
    SERVICE_SEC_NAME_PLUGIN_MAX_BYTES, SERVICE_SEC_NAME_TAG, SERVICE_SEC_RANDOM_TAG,
    SERVICE_SEC_VALUE_LABEL_MAX_BYTES, SERVICE_SEC_VALUE_TAG, SERVICE_SEC_VALUE_VALUE_MAX_BYTES,
    SERVICE_SOCK_AT_TAG, SERVICE_SOCK_BROADCAST_DATA_MAX_BYTES, SERVICE_SOCK_BROADCAST_TAG,
    SERVICE_SOCK_BYTES_DATA_MAX_BYTES, SERVICE_SOCK_BYTES_TAG, SERVICE_SOCK_CONNECT_HOST_MAX_BYTES,
    SERVICE_SOCK_CONNECT_SERVER_NAME_MAX_BYTES, SERVICE_SOCK_CONNECT_TAG,
    SERVICE_SOCK_PEER_CERTIFICATE_MAX_BYTES, SERVICE_SOCK_PEER_KEY_MAX_BYTES,
    SERVICE_SOCK_PEER_TAG, SERVICE_SOCK_PIN_HASH_MAX_BYTES, SERVICE_SOCK_PIN_TAG,
    SERVICE_SOCK_PORT_TAG, SERVICE_SOCK_READ_PATH_MAX_BYTES, SERVICE_SOCK_READ_TAG,
    SERVICE_SOCK_REF_TAG, SERVICE_SOCK_SEND_DATA_MAX_BYTES, SERVICE_SOCK_SEND_TAG,
    SERVICE_SOCK_STARTED_TAG, SERVICE_SOCK_TLS_SERVER_NAME_MAX_BYTES, SERVICE_SOCK_TLS_TAG,
    SERVICE_WORK_KEY_KEY_MAX_BYTES, SERVICE_WORK_KEY_TAG, SERVICE_WORK_ROW_KEY_MAX_BYTES,
    SERVICE_WORK_ROW_TAG, SERVICE_WORK_SPEC_KEY_MAX_BYTES, SERVICE_WORK_SPEC_TAG, ServiceNetAnswer,
    ServiceNetBody, ServiceNetBytes, ServiceNetFile, ServiceNetHeader, ServiceNetId,
    ServiceNetRead, ServiceNetRequest, ServiceNetResponse, ServiceNetServe, ServiceNetStarted,
    ServiceNetToken, ServiceSecBytes, ServiceSecEntry, ServiceSecKeep, ServiceSecList,
    ServiceSecName, ServiceSecRandom, ServiceSecValue, ServiceSockAt, ServiceSockBroadcast,
    ServiceSockBytes, ServiceSockConnect, ServiceSockPeer, ServiceSockPin, ServiceSockPort,
    ServiceSockRead, ServiceSockRef, ServiceSockSend, ServiceSockStarted, ServiceSockTls,
    ServiceWorkKey, ServiceWorkRow, ServiceWorkSpec,
};
// Single-record and streaming decoders for plugin-side readers (lock format 8).
pub use wire::{RECORD_CARRY_MAX_BYTES, RecordStream, record_decode_prefix, records_decode_single};
// The host's decoders (lock format 10, #985): only records a plugin sends. The
// host decodes through these alone, so a device links no decoder for events,
// input, frames or service replies.
pub use wire::{
    host_arguments_decode_into, host_decode_one, host_decode_prefix, host_records_decode_into,
    host_records_visit,
};
// The typed codec (#364): decode and encode one record type, linking no other
// record's code. The host reads and writes records through `codec` (todo 129).
pub use wire::{
    RawRecord, WireEncode, WireRecord, record_encode_of, record_framed_len_of,
    records_encode_counted_of, records_encode_of,
};
// Counted service inputs above the registration limit (`ServiceOp::input_rows`).
pub use wire::{records_encode_counted, records_encode_counted_iter};

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
