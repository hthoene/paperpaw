//! Bounded copy-out queries over a scoped settings and translation view.

#![allow(clippy::unwrap_used, reason = "tests fail loudly on unexpected errors")]

#[path = "../src/abi7/context.rs"]
mod context;
#[path = "../src/abi7/queries.rs"]
mod queries;
#[path = "../src/abi7/settings_wire.rs"]
mod settings_wire;
#[path = "../src/abi7/snapshot_wire.rs"]
#[allow(
    dead_code,
    clippy::all,
    clippy::pedantic,
    reason = "complete generated codec is exercised by snapshot tests"
)]
mod snapshot_wire;

use context::{Frame, FrameSlot};
use paperpaw_i18n::{Catalog, Translator};
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_SPACE, ERR_STATE, ERR_UTF8, PpBuffer, PpBytes,
};
use paperpaw_sdk::{Id, SettingKind, SettingSpec, SettingValue, Settings, collections::VecMap};
use settings_wire::{SettingValueRef, settings_decode};
use std::{cell::Cell, pin::pin, ptr};

const SENTINEL: u8 = 0xa5;
const GROUP: Id = Id::new_static("test.group");
const SIZE: &str = "widgets.test.size";
const PLACED_SIZE: &str = "widgets.test@2.size";
const PLACEMENT: &str = "widgets.test@2";

type Query = unsafe fn(&FrameSlot, PpBytes, PpBuffer) -> i32;

struct View<'a> {
    settings: &'a Settings,
    translator: &'a Translator,
    settings_reads: Cell<usize>,
    translation_reads: Cell<usize>,
}

impl<'a> View<'a> {
    fn new(settings: &'a Settings, translator: &'a Translator) -> Self {
        Self {
            settings,
            translator,
            settings_reads: Cell::new(0),
            translation_reads: Cell::new(0),
        }
    }
}

impl Frame for View<'_> {
    fn settings(&self) -> &Settings {
        self.settings_reads.set(self.settings_reads.get() + 1);
        self.settings
    }

    fn translator(&self) -> &Translator {
        self.translation_reads.set(self.translation_reads.get() + 1);
        self.translator
    }
}

fn bytes(input: &[u8]) -> PpBytes {
    PpBytes {
        ptr: input.as_ptr(),
        len: input.len(),
    }
}

fn buffer(output: &mut [u8]) -> PpBuffer {
    PpBuffer {
        ptr: output.as_mut_ptr(),
        cap: output.len(),
    }
}

fn query(query: Query, slot: &FrameSlot, key: &str, output: &mut [u8]) -> i32 {
    if let Ok(result) = slot.with(|frame| frame.snapshot()) {
        assert!(matches!(result, Err(ERR_STATE)));
    }
    // SAFETY: both slices are live and disjoint throughout this synchronous call.
    unsafe { query(slot, bytes(key.as_bytes()), buffer(output)) }
}

fn read_setting<'a>(settings: &Settings, key: &str, output: &'a mut [u8]) -> SettingValueRef<'a> {
    let slot = pin!(FrameSlot::new());
    let translator = Translator::new();
    let view = View::new(settings, &translator);
    let needed = slot
        .as_ref()
        .scope(&view, || query(queries::settings_read, &slot, key, output))
        .unwrap();
    let needed = usize::try_from(needed).unwrap();
    assert!(needed <= output.len());
    assert!(output[needed..].iter().all(|byte| *byte == SENTINEL));
    settings_decode(&output[..needed]).unwrap()
}

fn read_int(settings: &Settings, key: &str) -> i64 {
    let mut output = [SENTINEL; 32];
    match read_setting(settings, key, &mut output) {
        SettingValueRef::Int(value) => value,
        other => panic!("expected integer, got {other:?}"),
    }
}

#[test]
fn queries_without_a_frame_leave_output_unchanged() {
    let slot = pin!(FrameSlot::new());
    let settings = Settings::new();
    let translator = Translator::new();
    let view = View::new(&settings, &translator);
    for call in [queries::settings_read as Query, queries::i18n_copy] {
        let mut output = [SENTINEL; 32];
        assert_eq!(query(call, &slot, "test.missing", &mut output), ERR_STATE);
        assert_eq!(output, [SENTINEL; 32]);
        slot.as_ref().scope(&view, || ()).unwrap();
        assert_eq!(query(call, &slot, "test.missing", &mut output), ERR_STATE);
        assert_eq!(output, [SENTINEL; 32]);
    }
    assert_eq!(view.settings_reads.get(), 0);
    assert_eq!(view.translation_reads.get(), 0);
}

#[test]
fn malformed_pairs_are_rejected_before_any_frame_lookup() {
    let slot = pin!(FrameSlot::new());
    let settings = Settings::new();
    let translator = Translator::new();
    let view = View::new(&settings, &translator);
    let oversized_key = [b'a'; 4097];
    slot.as_ref()
        .scope(&view, || {
            for call in [queries::settings_read as Query, queries::i18n_copy] {
                for (key, expected) in [
                    (bytes(&[0xff]), ERR_UTF8),
                    (
                        PpBytes {
                            ptr: ptr::null(),
                            len: 1,
                        },
                        ERR_INVALID,
                    ),
                    (bytes(&oversized_key), ERR_INVALID),
                ] {
                    let mut output = [SENTINEL; 32];
                    // SAFETY: non-null input is live for its declared length;
                    // null input is a rejected pair. Output is live and disjoint.
                    assert_eq!(unsafe { call(&slot, key, buffer(&mut output)) }, expected);
                    assert_eq!(output, [SENTINEL; 32]);
                }
                let mut output = [SENTINEL; 4097];
                for out in [
                    PpBuffer {
                        ptr: ptr::null_mut(),
                        cap: 1,
                    },
                    buffer(&mut output),
                ] {
                    // SAFETY: non-null output has its declared capacity; null
                    // output is a rejected pair. The input is live and disjoint.
                    assert_eq!(unsafe { call(&slot, bytes(b"test.key"), out) }, ERR_INVALID);
                    assert_eq!(output, [SENTINEL; 4097]);
                }
            }
        })
        .unwrap();
    assert_eq!(view.settings_reads.get(), 0);
    assert_eq!(view.translation_reads.get(), 0);
}

#[test]
fn null_empty_pairs_are_valid_size_queries() {
    let slot = pin!(FrameSlot::new());
    let settings = Settings::new();
    let translator = Translator::new();
    let view = View::new(&settings, &translator);
    slot.as_ref()
        .scope(&view, || {
            for (call, expected) in [
                (queries::settings_read as Query, 12),
                (queries::i18n_copy, 0),
            ] {
                // SAFETY: null pairs with zero length/capacity are permitted.
                let result = unsafe {
                    call(
                        &slot,
                        PpBytes {
                            ptr: ptr::null(),
                            len: 0,
                        },
                        PpBuffer {
                            ptr: ptr::null_mut(),
                            cap: 0,
                        },
                    )
                };
                assert_eq!(result, expected);
            }
        })
        .unwrap();
    assert_eq!(view.settings_reads.get(), 1);
    assert_eq!(view.translation_reads.get(), 1);
}

#[test]
fn false_zero_and_empty_text_remain_distinct_from_absence() {
    let mut settings = Settings::new();
    settings
        .define(SettingSpec::switch("test.false", "", false, GROUP))
        .unwrap();
    settings
        .define(SettingSpec::number("test.zero", "", -1, 1, 0, GROUP))
        .unwrap();
    settings
        .define(SettingSpec::text("test.empty", "", "", GROUP))
        .unwrap();
    let mut output = [SENTINEL; 32];
    assert!(matches!(
        read_setting(&settings, "test.false", &mut output),
        SettingValueRef::Bool(false)
    ));
    output.fill(SENTINEL);
    assert!(matches!(
        read_setting(&settings, "test.zero", &mut output),
        SettingValueRef::Int(0)
    ));
    output.fill(SENTINEL);
    assert!(matches!(
        read_setting(&settings, "test.empty", &mut output),
        SettingValueRef::Text("")
    ));
    output.fill(SENTINEL);
    assert!(matches!(
        read_setting(&settings, "test.absent", &mut output),
        SettingValueRef::Absent
    ));
}

#[test]
fn settings_short_copy_returns_full_wire_length_and_exact_prefix() {
    let mut settings = Settings::new();
    settings
        .define(SettingSpec::text("test.text", "", "Grüße", GROUP))
        .unwrap();
    let slot = pin!(FrameSlot::new());
    let translator = Translator::new();
    let view = View::new(&settings, &translator);
    slot.as_ref()
        .scope(&view, || {
            let needed = 12 + "Grüße".len();
            let mut full = [SENTINEL; 32];
            assert_eq!(
                query(queries::settings_read, &slot, "test.text", &mut full),
                needed as i32
            );
            assert!(matches!(
                settings_decode(&full[..needed]),
                Ok(SettingValueRef::Text("Grüße"))
            ));
            assert_eq!(&full[..12], &[1, 0, 0, 0, 3, 0, 0, 0, 7, 0, 0, 0]);
            assert_eq!(&full[12..needed], "Grüße".as_bytes());
            assert!(full[needed..].iter().all(|byte| *byte == SENTINEL));
            for cap in [0, 1, 11, 12, 14, 18, 19] {
                let mut output = [SENTINEL; 32];
                assert_eq!(
                    query(
                        queries::settings_read,
                        &slot,
                        "test.text",
                        &mut output[..cap]
                    ),
                    needed as i32
                );
                assert_eq!(&output[..cap], &full[..cap]);
                assert!(output[cap..].iter().all(|byte| *byte == SENTINEL));
            }
        })
        .unwrap();
}

#[test]
fn settings_values_over_the_wire_budget_leave_output_unchanged() {
    let mut settings = Settings::new();
    settings
        .define(SettingSpec::text("test.large", "", "", GROUP))
        .unwrap();
    settings
        .set("test.large", SettingValue::Text("a".repeat(4085).into()))
        .unwrap();
    let slot = pin!(FrameSlot::new());
    let translator = Translator::new();
    let view = View::new(&settings, &translator);
    let mut output = [SENTINEL; 4096];
    let result = slot
        .as_ref()
        .scope(&view, || {
            query(queries::settings_read, &slot, "test.large", &mut output)
        })
        .unwrap();
    assert_eq!(result, ERR_NO_SPACE);
    assert_eq!(output, [SENTINEL; 4096]);
}

#[test]
fn settings_queries_preserve_scope_preview_and_placement_precedence() {
    use paperpaw_sdk::builtin::settings::SCOPE_ONLY;
    let mut settings = Settings::new();
    settings
        .define(SettingSpec::number(SIZE, "", 0, 99, 3, GROUP).scoped())
        .unwrap();
    settings
        .define(
            SettingSpec::new(
                SCOPE_ONLY.into(),
                "",
                SettingKind::Bool,
                SettingValue::Bool(false),
                GROUP,
            )
            .scoped(),
        )
        .unwrap();
    assert_eq!(read_int(&settings, SIZE), 3);
    settings.set(SIZE, SettingValue::Int(7)).unwrap();
    assert_eq!(read_int(&settings, SIZE), 7);
    let mut scope = VecMap::new();
    scope.insert(Id::new_static(SIZE), SettingValue::Int(11));
    scope.insert(SCOPE_ONLY.into(), SettingValue::Bool(true));
    settings.set_scope(Some(scope));
    assert_eq!(read_int(&settings, SIZE), 11);
    settings.set_preview(vec![(Id::new_static(SIZE), Some(SettingValue::Int(13)))]);
    assert_eq!(read_int(&settings, SIZE), 13);
    settings.set(PLACED_SIZE, SettingValue::Int(17)).unwrap();
    assert_eq!(read_int(&settings, PLACED_SIZE), 17);
    assert_eq!(
        settings.with_placement(Some(PLACEMENT), || read_int(&settings, SIZE)),
        17
    );
    settings.set_preview(vec![
        (Id::new_static(SIZE), Some(SettingValue::Int(13))),
        (Id::new_static(PLACED_SIZE), Some(SettingValue::Int(19))),
    ]);
    assert_eq!(
        settings.with_placement(Some(PLACEMENT), || read_int(&settings, SIZE)),
        19
    );
    settings.set_preview(vec![
        (Id::new_static(SIZE), Some(SettingValue::Int(13))),
        (Id::new_static(PLACED_SIZE), None),
    ]);
    assert_eq!(
        settings.with_placement(Some(PLACEMENT), || read_int(&settings, SIZE)),
        13
    );
    settings.set_preview(Vec::new());
    assert_eq!(
        settings.with_placement(Some(PLACEMENT), || read_int(&settings, SIZE)),
        17
    );
    assert_eq!(read_int(&settings, SIZE), 11);
    settings.set_scope(None);
    assert_eq!(read_int(&settings, SIZE), 7);
}

#[test]
fn translations_and_missing_keys_copy_into_independent_live_outputs() {
    let mut first = [SENTINEL; 32];
    let mut second = [SENTINEL; 32];
    {
        let slot = pin!(FrameSlot::new());
        let settings = Settings::new();
        let mut translator = Translator::new();
        translator.add(Catalog::parse("[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n[messages]\ntest.hello = Grüß dich\n").unwrap());
        translator.set_language("de");
        let missing_key = String::from("test.missing");
        let view = View::new(&settings, &translator);
        slot.as_ref()
            .scope(&view, || {
                assert_eq!(
                    query(queries::i18n_copy, &slot, "test.hello", &mut first),
                    "Grüß dich".len() as i32
                );
                assert_eq!(
                    query(queries::i18n_copy, &slot, &missing_key, &mut second),
                    missing_key.len() as i32
                );
                assert_eq!(&first[.."Grüß dich".len()], "Grüß dich".as_bytes());
                let mut partial = [SENTINEL; 8];
                assert_eq!(
                    query(queries::i18n_copy, &slot, "test.hello", &mut partial[..3]),
                    "Grüß dich".len() as i32
                );
                assert_eq!(&partial[..3], &"Grüß dich".as_bytes()[..3]);
                assert!(partial[3..].iter().all(|byte| *byte == SENTINEL));
            })
            .unwrap();
        assert_eq!(view.translation_reads.get(), 3);
        assert_eq!(view.settings_reads.get(), 0);
    }
    assert_eq!(&first[.."Grüß dich".len()], "Grüß dich".as_bytes());
    assert_eq!(&second[.."test.missing".len()], b"test.missing");
    assert!(
        first["Grüß dich".len()..]
            .iter()
            .all(|byte| *byte == SENTINEL)
    );
    assert!(
        second["test.missing".len()..]
            .iter()
            .all(|byte| *byte == SENTINEL)
    );
}
