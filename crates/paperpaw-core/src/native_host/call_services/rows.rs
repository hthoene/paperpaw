//! The rows of a service reply, each read by its record type (todo 129): the
//! generated op tables name 93 reply tags, and a row is checked by the one
//! decoder of its own tag rather than by the enum-wide decoder.
use crate::native_host::registrations::{self as wire, RawRecord, WireRecord};
use paperpaw_native::host_glue::ERR_INVALID;

/// `raw` decodes as `R`; any failure is `ERR_INVALID`.
fn row<'a, R: WireRecord<'a>>(raw: &RawRecord<'a>) -> Result<(), i32> {
    raw.decode::<R>().map(|_| ()).map_err(|_| ERR_INVALID)
}

/// The row `raw` as the record type of its tag. A tag no op replies with is
/// `ERR_INVALID`; the caller has already checked the tag against the op's list.
#[allow(
    clippy::too_many_lines,
    reason = "one arm per reply tag of the generated op tables: a flat table"
)]
#[cfg(any(test, debug_assertions))]
pub(super) fn check(raw: &RawRecord<'_>) -> Result<(), i32> {
    match raw.tag() {
        wire::SERVICE_END_TAG => row::<wire::ServiceEnd>(raw),
        wire::SERVICE_ECHO_TAG => row::<wire::ServiceEcho<'_>>(raw),
        wire::SERVICE_REG_SETTING_TAG => row::<wire::ServiceRegSetting<'_>>(raw),
        wire::SERVICE_REG_SETTING_EXTRA_TAG => row::<wire::ServiceRegSettingExtra<'_>>(raw),
        wire::SERVICE_REG_CHOICE_TAG => row::<wire::ServiceRegChoice<'_>>(raw),
        wire::SERVICE_REG_TEXT_TAG => row::<wire::ServiceRegText<'_>>(raw),
        wire::SERVICE_REG_GROUP_TAG => row::<wire::ServiceRegGroup<'_>>(raw),
        wire::SERVICE_REG_PLUGIN_TAG => row::<wire::ServiceRegPlugin<'_>>(raw),
        wire::SERVICE_REG_NEED_TAG => row::<wire::ServiceRegNeed<'_>>(raw),
        wire::SERVICE_REG_ITEM_TAG => row::<wire::ServiceRegItem<'_>>(raw),
        wire::SERVICE_REG_SLOT_TAG => row::<wire::ServiceRegSlot<'_>>(raw),
        wire::SERVICE_REG_MENU_ROW_TAG => row::<wire::ServiceRegMenuRow<'_>>(raw),
        wire::SERVICE_REG_BINDING_TAG => row::<wire::ServiceRegBinding<'_>>(raw),
        wire::SERVICE_REG_SETUP_STEP_TAG => row::<wire::ServiceRegSetupStep<'_>>(raw),
        wire::SERVICE_REG_VALUE_TAG => row::<wire::ServiceRegValue<'_>>(raw),
        wire::SERVICE_REG_WIDGET_TAG => row::<wire::ServiceRegWidget<'_>>(raw),
        wire::SERVICE_UI_SCREEN_TAG => row::<wire::ServiceUiScreen<'_>>(raw),
        wire::SERVICE_UI_REQUEST_TAG => row::<wire::ServiceUiRequest>(raw),
        wire::SERVICE_UI_QUESTION_TAG => row::<wire::ServiceUiQuestion<'_>>(raw),
        wire::SERVICE_UI_HINT_TAG => row::<wire::ServiceUiHint<'_>>(raw),
        wire::SERVICE_UI_INSTANCE_TAG => row::<wire::ServiceUiInstance>(raw),
        wire::SERVICE_UI_TARGET_TAG => row::<wire::ServiceUiTarget>(raw),
        wire::SERVICE_UI_TEXT_TAG => row::<wire::ServiceUiText<'_>>(raw),
        wire::SERVICE_UI_SIZE_TAG => row::<wire::ServiceUiSize>(raw),
        wire::SERVICE_UI_STATUS_VALUE_TAG => row::<wire::ServiceUiStatusValue<'_>>(raw),
        wire::SERVICE_UI_PARAM_TAG => row::<wire::ServiceUiParam<'_>>(raw),
        wire::SERVICE_UI_HANDLE_TAG => row::<wire::ServiceUiHandle>(raw),
        wire::SERVICE_REG_STATUS_TAG => row::<wire::ServiceRegStatus<'_>>(raw),
        wire::SERVICE_DOC_STATE_TAG => row::<wire::ServiceDocState<'_>>(raw),
        wire::SERVICE_DOC_META_TAG => row::<wire::ServiceDocMeta<'_>>(raw),
        wire::SERVICE_DOC_TEXT_TAG => row::<wire::ServiceDocText<'_>>(raw),
        wire::SERVICE_DOC_SECTION_TAG => row::<wire::ServiceDocSection>(raw),
        wire::SERVICE_DOC_AT_TAG => row::<wire::ServiceDocAt>(raw),
        wire::SERVICE_DOC_TOC_TAG => row::<wire::ServiceDocToc<'_>>(raw),
        wire::SERVICE_DOC_LABEL_TAG => row::<wire::ServiceDocLabel<'_>>(raw),
        wire::SERVICE_DOC_XPOINTER_TAG => row::<wire::ServiceDocXpointer<'_>>(raw),
        wire::SERVICE_DOC_LINK_TAG => row::<wire::ServiceDocLink<'_>>(raw),
        wire::SERVICE_DOC_FONT_TAG => row::<wire::ServiceDocFont>(raw),
        wire::SERVICE_DOC_FORMAT_TAG => row::<wire::ServiceDocFormat<'_>>(raw),
        wire::SERVICE_DOC_READER_TAG => row::<wire::ServiceDocReader<'_>>(raw),
        wire::SERVICE_DOC_PROGRESS_TAG => row::<wire::ServiceDocProgress>(raw),
        wire::SERVICE_DOC_BLOCK_TAG => row::<wire::ServiceDocBlock<'_>>(raw),
        wire::SERVICE_DOC_RUN_TAG => row::<wire::ServiceDocRun>(raw),
        wire::SERVICE_DOC_MAP_TAG => row::<wire::ServiceDocMap>(raw),
        wire::SERVICE_DOC_CELL_TAG => row::<wire::ServiceDocCell>(raw),
        wire::SERVICE_DOC_RUBY_TAG => row::<wire::ServiceDocRuby<'_>>(raw),
        wire::SERVICE_DOC_INLINE_LANG_TAG => row::<wire::ServiceDocInlineLang<'_>>(raw),
        wire::SERVICE_DOC_IMAGE_TAG => row::<wire::ServiceDocImage>(raw),
        wire::SERVICE_DOC_TEXT_PART_TAG => row::<wire::ServiceDocTextPart<'_>>(raw),
        wire::SERVICE_DOC_COLUMNS_TAG => row::<wire::ServiceDocColumns<'_>>(raw),
        wire::SERVICE_LOOKUP_ENTRY_TAG => row::<wire::ServiceLookupEntry<'_>>(raw),
        wire::SERVICE_LIB_STATE_TAG => row::<wire::ServiceLibState>(raw),
        wire::SERVICE_LIB_ENTRY_TAG => row::<wire::ServiceLibEntry<'_>>(raw),
        wire::SERVICE_LIB_BOOK_TAG => row::<wire::ServiceLibBook<'_>>(raw),
        wire::SERVICE_LIB_TEXT_TAG => row::<wire::ServiceLibText<'_>>(raw),
        wire::SERVICE_LIB_FACET_TAG => row::<wire::ServiceLibFacet<'_>>(raw),
        wire::SERVICE_LIB_RESULT_TAG => row::<wire::ServiceLibResult<'_>>(raw),
        wire::SERVICE_LIB_POSITION_TAG => row::<wire::ServiceLibPosition<'_>>(raw),
        wire::SERVICE_LIB_ANNOTATION_TAG => row::<wire::ServiceLibAnnotation<'_>>(raw),
        wire::SERVICE_LIB_ANCHOR_TAG => row::<wire::ServiceLibAnchor<'_>>(raw),
        wire::SERVICE_LIB_TRANSACTION_TAG => row::<wire::ServiceLibTransaction>(raw),
        wire::SERVICE_LIB_COMMITTED_TAG => row::<wire::ServiceLibCommitted>(raw),
        wire::SERVICE_LIB_FIELD_TAG => row::<wire::ServiceLibField<'_>>(raw),
        wire::SERVICE_LIB_KEY_TAG => row::<wire::ServiceLibKey<'_>>(raw),
        wire::SERVICE_LIB_RECEIPT_TAG => row::<wire::ServiceLibReceipt>(raw),
        wire::SERVICE_DEV_DEVICE_TAG => row::<wire::ServiceDevDevice<'_>>(raw),
        wire::SERVICE_DEV_VALUE_TAG => row::<wire::ServiceDevValue<'_>>(raw),
        wire::SERVICE_DEV_FIRMWARE_TAG => row::<wire::ServiceDevFirmware<'_>>(raw),
        wire::SERVICE_DEV_BUILTIN_TAG => row::<wire::ServiceDevBuiltin<'_>>(raw),
        wire::SERVICE_DEV_NATIVE_TAG => row::<wire::ServiceDevNative<'_>>(raw),
        wire::SERVICE_DEV_CHECK_TAG => row::<wire::ServiceDevCheck<'_>>(raw),
        wire::SERVICE_DEV_MEMORY_TAG => row::<wire::ServiceDevMemory>(raw),
        wire::SERVICE_DEV_WIFI_TAG => row::<wire::ServiceDevWifi<'_>>(raw),
        wire::SERVICE_DEV_NETWORK_TAG => row::<wire::ServiceDevNetwork<'_>>(raw),
        wire::SERVICE_DEV_BLUETOOTH_TAG => row::<wire::ServiceDevBluetooth>(raw),
        wire::SERVICE_DEV_BONDED_TAG => row::<wire::ServiceDevBonded<'_>>(raw),
        wire::SERVICE_DEV_POWER_TAG => row::<wire::ServiceDevPower<'_>>(raw),
        wire::SERVICE_DEV_INSTALL_TAG => row::<wire::ServiceDevInstall<'_>>(raw),
        wire::SERVICE_DEV_NOT_INSTALLED_TAG => row::<wire::ServiceDevNotInstalled<'_>>(raw),
        wire::SERVICE_DEV_KEY_TAG => row::<wire::ServiceDevKey<'_>>(raw),
        wire::SERVICE_FS_ENTRY_TAG => row::<wire::ServiceFsEntry<'_>>(raw),
        wire::SERVICE_FS_SPACE_TAG => row::<wire::ServiceFsSpace>(raw),
        wire::SERVICE_FS_PROGRESS_TAG => row::<wire::ServiceFsProgress>(raw),
        wire::SERVICE_NET_STARTED_TAG => row::<wire::ServiceNetStarted>(raw),
        wire::SERVICE_NET_ANSWER_TAG => row::<wire::ServiceNetAnswer<'_>>(raw),
        wire::SERVICE_NET_BYTES_TAG => row::<wire::ServiceNetBytes<'_>>(raw),
        wire::SERVICE_SEC_VALUE_TAG => row::<wire::ServiceSecValue<'_>>(raw),
        wire::SERVICE_SEC_ENTRY_TAG => row::<wire::ServiceSecEntry<'_>>(raw),
        wire::SERVICE_SEC_BYTES_TAG => row::<wire::ServiceSecBytes<'_>>(raw),
        wire::SERVICE_WORK_ROW_TAG => row::<wire::ServiceWorkRow<'_>>(raw),
        wire::SERVICE_SOCK_STARTED_TAG => row::<wire::ServiceSockStarted>(raw),
        wire::SERVICE_SOCK_BYTES_TAG => row::<wire::ServiceSockBytes<'_>>(raw),
        wire::SERVICE_SOCK_PEER_TAG => row::<wire::ServiceSockPeer<'_>>(raw),
        // The host's own usage producers (`data_services`) answer outside the
        // generated op tables, through the same validator.
        wire::SERVICE_USAGE_TAG => row::<wire::ServiceUsage>(raw),
        wire::SERVICE_PLUGIN_USAGE_TAG => row::<wire::ServicePluginUsage<'_>>(raw),
        _ => Err(ERR_INVALID),
    }
}

/// The rows of `core.lookup`, whose provider is a plugin: its entry row and
/// the end row, each by its type. Any other tag is `ERR_INVALID`.
pub(super) fn check_lookup(raw: &RawRecord<'_>) -> Result<(), i32> {
    match raw.tag() {
        wire::SERVICE_LOOKUP_ENTRY_TAG => row::<wire::ServiceLookupEntry<'_>>(raw),
        wire::SERVICE_END_TAG => row::<wire::ServiceEnd>(raw),
        _ => Err(ERR_INVALID),
    }
}
