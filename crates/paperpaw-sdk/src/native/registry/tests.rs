#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::*;
use crate::native::declare::records_wire::Record;
use alloc::{string::String, vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};

#[derive(Default)]
struct Fixture {
    response: RefCell<Vec<u8>>,
    result: Cell<i32>,
    calls: Cell<u32>,
    queries: RefCell<Vec<(u32, u32, u32, u32)>>,
    scopes: RefCell<Vec<String>>,
}
unsafe extern "C" fn open(
    _: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: Service lends key bytes and aligned output for the call.
    assert_eq!(
        unsafe { core::slice::from_raw_parts(key.ptr, key.len) },
        schema::KEY.as_bytes()
    );
    assert_eq!((major, minor), (schema::MAJOR, schema::REVISION));
    // SAFETY: output is exclusive and aligned for this call.
    unsafe { out.write(7) };
    abi::OK
}
unsafe extern "C" fn call(
    ctx: *mut c_void,
    handle: u32,
    op: u32,
    input: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: fixture and input live for the call and output is disjoint.
    let (fixture, bytes) = unsafe {
        (
            &*ctx.cast::<Fixture>(),
            core::slice::from_raw_parts(input.ptr, input.len),
        )
    };
    assert_eq!(handle, 7);
    fixture.calls.set(fixture.calls.get() + 1);
    let record = wire::records_decode_single(bytes).unwrap();
    match record {
        Record::ServiceRegQuery(q) => {
            fixture.scopes.borrow_mut().push(q.scope.into());
            fixture
                .queries
                .borrow_mut()
                .push((op, q.cursor, q.generation, q.flags))
        }
        Record::ServiceRegKey(_) => {}
        _ => panic!("wrong query record"),
    }
    if fixture.result.get() < 0 {
        return fixture.result.get();
    }
    let response = fixture.response.borrow();
    assert!(response.len() <= out.cap);
    // SAFETY: caller lends at least response.len initialized output bytes.
    unsafe { core::slice::from_raw_parts_mut(out.ptr, response.len()) }.copy_from_slice(&response);
    i32::try_from(response.len()).unwrap()
}
unsafe extern "C" fn release(_: *mut c_void, handle: u32) -> i32 {
    assert_eq!(handle, 7);
    abi::OK
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.service_open = open;
    table.service_call = call;
    table.handle_release = release;
    table
}
fn host<'a>(table: &'a PpHost, fixture: &mut Fixture) -> Host<'a> {
    Host {
        table,
        ctx: ptr::from_mut(fixture).cast(),
    }
}
fn encode(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = vec![0; 4096];
    let mut length = 0;
    for record in records {
        length += wire::record_encode_one(record, &mut out[length..]).unwrap();
    }
    out.truncate(length);
    out
}
fn item(id: &str) -> Record<'_> {
    Record::ServiceRegItem(wire::ServiceRegItem {
        id,
        plugin: "fixture",
        flags: 0,
        title: id,
    })
}
fn end(total: u32, next: u32, generation: u32, last: bool) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        total,
        next,
        generation,
        flags: u32::from(last),
        cursor: None,
    })
}

#[test]
fn lossless_pages_preserve_order_and_generation_beyond_batch_limit() {
    let table = table();
    let mut fixture = Fixture::default();
    let mut first = vec![item("fixture.first"); schema::PAGE_MAX_ROWS];
    first.push(end(35, 32, 9, false));
    *fixture.response.borrow_mut() = encode(&first);
    let host = host(&table, &mut fixture);
    let registry = Registry::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    let page = registry
        .actions(&host, "", PageCursor::start(), &mut input, &mut out)
        .unwrap();
    assert_eq!(page.len(), 32);
    assert_eq!(
        page.rows().map(|row| row.id()).collect::<Vec<_>>(),
        vec!["fixture.first"; 32]
    );
    let cursor = page.end().next_cursor().unwrap();
    assert_eq!((cursor.offset(), cursor.generation()), (32, 9));
    *fixture.response.borrow_mut() = encode(&[
        item("fixture.next"),
        item("fixture.next2"),
        item("fixture.last"),
        end(35, 35, 9, true),
    ]);
    let page = registry
        .actions(&host, "", cursor, &mut input, &mut out)
        .unwrap();
    assert_eq!(
        page.rows().map(|row| row.id()).collect::<Vec<_>>(),
        ["fixture.next", "fixture.next2", "fixture.last"]
    );
    assert!(page.end().is_last());
    assert_eq!(page.end().next_cursor(), None);
    assert_eq!(
        &*fixture.queries.borrow(),
        &[
            (schema::OP_ACTIONS, 0, 0, 0),
            (schema::OP_ACTIONS, 32, 9, 0)
        ]
    );
    registry.release(&host).unwrap();
}

#[test]
fn generation_changes_are_not_restarted_or_partially_exposed() {
    let table = table();
    let mut fixture = Fixture::default();
    *fixture.response.borrow_mut() = encode(&[item("fixture.a"), end(2, 1, 7, false)]);
    let host = host(&table, &mut fixture);
    let registry = Registry::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    let cursor = registry
        .actions(&host, "", PageCursor::start(), &mut input, &mut out)
        .unwrap()
        .end()
        .next_cursor()
        .unwrap();
    fixture.result.set(abi::ERR_STATE);
    out.fill(123);
    assert!(matches!(
        registry.actions(&host, "", cursor, &mut input, &mut out),
        Err(Error::State)
    ));
    assert_eq!(out, [123; 4096]);
    assert_eq!(fixture.calls.get(), 2);
    fixture.result.set(abi::OK);
    *fixture.response.borrow_mut() = encode(&[item("fixture.b"), end(2, 2, 8, true)]);
    assert!(matches!(
        registry.actions(&host, "", cursor, &mut input, &mut out),
        Err(Error::State)
    ));
    assert_eq!(fixture.calls.get(), 3);
}

#[test]
fn malformed_pages_never_produce_a_page() {
    let cases = [
        encode(&[item("fixture.a")]),
        encode(&[end(0, 0, 1, true), item("fixture.a")]),
        encode(&[end(0, 0, 1, true), end(0, 0, 1, true)]),
        encode(&[item("fixture.a"), end(1, 1, 0, true)]),
        encode(&[item("fixture.a"), end(2, 2, 1, true)]),
        encode(&[item("fixture.a"), end(1, 1, 1, false)]),
        encode(&[end(1, 0, 1, false)]),
        encode(&[item(""), end(1, 1, 1, true)]),
        encode(&[
            Record::ServiceRegChoice(wire::ServiceRegChoice {
                value: "x",
                title: "X",
            }),
            end(1, 1, 1, true),
        ]),
    ];
    for bytes in cases {
        assert!(matches!(
            validate_page(&bytes, PageCursor::start(), RegistryItem::decode),
            Err(Error::Invalid)
        ));
    }
    let mut too_many = vec![item("fixture.a"); schema::PAGE_MAX_ROWS + 1];
    too_many.push(end(33, 33, 1, true));
    assert!(matches!(
        validate_page(
            &encode(&too_many),
            PageCursor::start(),
            RegistryItem::decode
        ),
        Err(Error::Invalid)
    ));
    let mut trailing = encode(&[end(0, 0, 1, true)]);
    trailing.push(0);
    assert!(matches!(
        validate_page(&trailing, PageCursor::start(), RegistryItem::decode),
        Err(Error::Invalid)
    ));
    let truncated = encode(&[item("fixture.a"), end(1, 1, 1, true)]);
    for len in 0..truncated.len() {
        assert!(
            validate_page(&truncated[..len], PageCursor::start(), RegistryItem::decode).is_err()
        );
    }
}

#[test]
fn typed_menu_and_values_options_reach_the_host_without_raw_flags() {
    let table = table();
    let mut fixture = Fixture::default();
    *fixture.response.borrow_mut() = encode(&[end(0, 0, 1, true)]);
    let host = host(&table, &mut fixture);
    let registry = Registry::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    registry
        .menu(
            &host,
            "core.main",
            MenuQuery::Declared,
            PageCursor::start(),
            &mut input,
            &mut out,
        )
        .unwrap();
    registry
        .values(
            &host,
            ValuesQuery::Placement("widgets.clock@2"),
            PageCursor::start(),
            &mut input,
            &mut out,
        )
        .unwrap();
    assert_eq!(
        &*fixture.queries.borrow(),
        &[(schema::OP_MENU, 0, 0, 2), (schema::OP_VALUES, 0, 0, 1)]
    );
}

fn setting<'a>(value: &'a [u8], flags: u32) -> Record<'a> {
    Record::ServiceRegSetting(wire::ServiceRegSetting {
        key: "fixture.value",
        group: "fixture",
        kind: 0,
        flags,
        source: 3,
        title: "Value",
        value,
    })
}
fn extra() -> Record<'static> {
    Record::ServiceRegSettingExtra(wire::ServiceRegSettingExtra {
        min: 0,
        max: 1,
        step: 1,
        unit: "",
        section: "",
        follows: "",
        shown_id: "",
        shown_value: "",
        editor_kind: 0,
        editor: "",
        choices: 0,
    })
}
fn plugin(id: &str) -> Record<'_> {
    Record::ServiceRegPlugin(wire::ServiceRegPlugin {
        id,
        name: "Fixture",
        version: "1.0.0",
        kind: 2,
        state: 0,
        flags: 1,
        problem: 0,
        problem_arg: 0,
        update: "",
    })
}

#[test]
fn details_are_exact_bare_sequences_without_paging_end() {
    let table = table();
    let mut fixture = Fixture::default();
    *fixture.response.borrow_mut() = encode(&[
        setting(&[], 1 << 5),
        extra(),
        Record::ServiceRegText(wire::ServiceRegText {
            text: "Complete help",
        }),
    ]);
    let host = host(&table, &mut fixture);
    let registry = Registry::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    let detail = registry
        .setting(&host, "fixture.value", &mut input, &mut out)
        .unwrap();
    assert_eq!(detail.help(), Some("Complete help"));
    assert_eq!(detail.setting().kind(), RegistrySettingKind::Bool);
    assert_eq!(detail.setting().value(), None);
    assert_eq!(detail.extra().step(), 1);
    *fixture.response.borrow_mut() = encode(&[plugin("fixture")]);
    let detail = registry
        .plugin(&host, "fixture", &mut input, &mut out)
        .unwrap();
    assert_eq!(detail.plugin().kind(), RegistryPluginKind::Code);
    assert_eq!(detail.description(), None);
    *fixture.response.borrow_mut() = encode(&[
        item("fixture.preset"),
        Record::ServiceRegText(wire::ServiceRegText {
            text: "Description",
        }),
    ]);
    let detail = registry
        .preset(&host, "fixture.preset", &mut input, &mut out)
        .unwrap();
    assert_eq!(detail.preset().id(), "fixture.preset");
    assert_eq!(detail.description(), Some("Description"));
    for response in [
        encode(&[extra(), setting(&[], 1 << 5)]),
        encode(&[setting(&[], 1 << 5)]),
        encode(&[setting(&[], 1 << 5), extra(), extra()]),
        encode(&[setting(&[], 1 << 5), extra(), end(1, 1, 1, true)]),
    ] {
        *fixture.response.borrow_mut() = response;
        assert!(matches!(
            registry.setting(&host, "fixture.value", &mut input, &mut out),
            Err(Error::Invalid)
        ));
    }
    *fixture.response.borrow_mut() = encode(&[plugin("other")]);
    assert!(matches!(
        registry.plugin(&host, "fixture", &mut input, &mut out),
        Err(Error::Invalid)
    ));
    fixture.result.set(abi::ERR_NOT_FOUND);
    assert!(matches!(
        registry.plugin(&host, "missing", &mut input, &mut out),
        Err(Error::NotFound)
    ));
}

#[test]
fn malformed_values_and_secret_material_are_refused_before_exposure() {
    for row in [
        setting(&[], 0),
        setting(&[0; 12], 1 << 1),
        setting(&[0; 12], 1 << 5),
        setting(&[0; 12], 1 << 7),
    ] {
        let bytes = encode(&[row, end(1, 1, 1, true)]);
        assert!(matches!(
            validate_page(&bytes, PageCursor::start(), RegistrySetting::decode),
            Err(Error::Invalid)
        ));
    }
    let value = Record::ServiceRegValue(wire::ServiceRegValue {
        key: "fixture.value",
        value: &[],
        flags: 1,
    });
    let bytes = encode(&[value, end(1, 1, 1, true)]);
    let page = validate_page(&bytes, PageCursor::start(), RegistryValue::decode).unwrap();
    assert_eq!(page.rows().next().unwrap().value(), None);
    let bad = Record::ServiceRegValue(wire::ServiceRegValue {
        key: "fixture.value",
        value: &[0; 12],
        flags: 2,
    });
    assert!(matches!(
        validate_page(
            &encode(&[bad, end(1, 1, 1, true)]),
            PageCursor::start(),
            RegistryValue::decode
        ),
        Err(Error::Invalid)
    ));
}

#[test]
fn every_list_operation_uses_the_declared_schema_and_accepts_empty_pages() {
    let table = table();
    let mut fixture = Fixture::default();
    *fixture.response.borrow_mut() = encode(&[end(0, 0, 1, true)]);
    let host = host(&table, &mut fixture);
    let registry = Registry::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    let cursor = PageCursor::start();
    assert!(
        registry
            .settings(&host, "", cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .choices(&host, "fixture.value", cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .groups(&host, "", cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .plugins(&host, cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .actions(&host, "", cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .screens(&host, "", cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .slots(&host, cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .slot_offers(&host, "core.home", cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .queries
            .borrow()
            .iter()
            .map(|q| q.0)
            .collect::<Vec<_>>(),
        [1, 3, 4, 5, 7, 8, 9, 10]
    );
}

#[test]
fn remaining_list_operations_use_the_declared_schema_and_accept_empty_pages() {
    let table = table();
    let mut fixture = Fixture::default();
    *fixture.response.borrow_mut() = encode(&[end(0, 0, 1, true)]);
    let host = host(&table, &mut fixture);
    let registry = Registry::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    let cursor = PageCursor::start();
    assert!(
        registry
            .menus(&host, cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .menu(
                &host,
                "core.main",
                MenuQuery::Current,
                cursor,
                &mut input,
                &mut out
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .bindings(&host, "", cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .inputs(&host, cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .setup(&host, cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .presets(&host, "", cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .values(
                &host,
                ValuesQuery::Preset("fixture.preset"),
                cursor,
                &mut input,
                &mut out
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .widgets(&host, "", cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .needs(&host, "fixture", cursor, &mut input, &mut out)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .queries
            .borrow()
            .iter()
            .map(|q| q.0)
            .collect::<Vec<_>>(),
        [11, 12, 13, 14, 15, 16, 18, 19, 20]
    );
}

/// The widget list leaves bit 0 of the flags unset (`registry.md`, op 19): the
/// host then leaves out the widgets that only preview a settings group, so a
/// widget picker (the home plugin's) offers no preview widget. Bit 0 lists
/// only those.
#[test]
fn the_widget_list_never_asks_for_preview_widgets() {
    let table = table();
    let mut fixture = Fixture::default();
    *fixture.response.borrow_mut() = encode(&[end(0, 0, 1, true)]);
    let host = host(&table, &mut fixture);
    let registry = Registry::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    for scope in ["", "home"] {
        registry
            .widgets(&host, scope, PageCursor::start(), &mut input, &mut out)
            .unwrap();
    }
    assert_eq!(
        &*fixture.queries.borrow(),
        &[(schema::OP_WIDGETS, 0, 0, 0), (schema::OP_WIDGETS, 0, 0, 0)]
    );
}

#[test]
fn host_id_caps_remain_independent_of_plugin_loader_validation() {
    let id = "p".repeat(wire::SERVICE_REG_PLUGIN_ID_MAX_BYTES);
    let bytes = encode(&[plugin(&id), end(1, 1, 1, true)]);
    let page = validate_page(&bytes, PageCursor::start(), RegistryPlugin::decode).unwrap();
    assert_eq!(page.rows().next().unwrap().id(), id);
}

#[test]
fn choices_and_encoded_setting_values_keep_their_distinct_types() {
    use crate::native::settings::SettingValue;
    use crate::native::wire::settings_wire::{SettingValueRef, settings_encode};
    use core::mem::MaybeUninit;
    let choices = encode(&[
        Record::ServiceRegChoice(wire::ServiceRegChoice {
            value: "night",
            title: "Night",
        }),
        end(1, 1, 1, true),
    ]);
    let page = validate_page(&choices, PageCursor::start(), RegistryChoice::decode).unwrap();
    let choice = page.rows().next().unwrap();
    assert_eq!(choice.value(), "night");
    assert_eq!(choice.title(), "Night");

    let mut scratch = [MaybeUninit::uninit(); 32];
    let length = settings_encode(SettingValueRef::Bool(true), &mut scratch).unwrap();
    // SAFETY: the generated encoder initializes exactly this prefix.
    let value = scratch[..length]
        .iter()
        .map(|byte| unsafe { byte.assume_init() })
        .collect::<Vec<_>>();
    let bytes = encode(&[setting(&value, 0), end(1, 1, 1, true)]);
    let page = validate_page(&bytes, PageCursor::start(), RegistrySetting::decode).unwrap();
    assert_eq!(
        page.rows().next().unwrap().value(),
        Some(SettingValue::Bool(true))
    );
    let bytes = encode(&[
        Record::ServiceRegValue(wire::ServiceRegValue {
            key: "fixture.value",
            value: &value,
            flags: 0,
        }),
        end(1, 1, 1, true),
    ]);
    let page = validate_page(&bytes, PageCursor::start(), RegistryValue::decode).unwrap();
    assert_eq!(
        page.rows().next().unwrap().value(),
        Some(SettingValue::Bool(true))
    );
}

#[test]
fn status_discovery_maps_scopes_and_borrows_named_rows() {
    let table = table();
    let mut fixture = Fixture::default();
    let host = host(&table, &mut fixture);
    let registry = Registry::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    for (scope, kind, expected) in [
        (StatusScope::All, 0, StatusKind::Item),
        (StatusScope::Items, 0, StatusKind::Item),
        (StatusScope::Lines, 1, StatusKind::Line),
    ] {
        *fixture.response.borrow_mut() = encode(&[
            Record::ServiceRegStatus(wire::ServiceRegStatus {
                id: "fixture.clock",
                plugin: "fixture",
                kind,
                flags: kind | 2,
                priority: 0,
                size: 1000,
                tap: "",
                title: "Clock",
            }),
            end(1, 1, 9, true),
        ]);
        let out_range = out.as_ptr_range();
        let page = registry
            .status(&host, scope, PageCursor::start(), &mut input, &mut out)
            .unwrap();
        let row = page.rows().next().unwrap();
        assert_eq!(
            (row.id(), row.plugin(), row.title()),
            ("fixture.clock", "fixture", "Clock")
        );
        assert_eq!(row.kind(), expected);
        assert_eq!(row.drawn_by_owner(), kind == 1);
        assert!(row.private());
        assert_eq!((row.priority(), row.size(), row.tap()), (0, 1000, ""));
        assert!(out_range.contains(&row.title().as_ptr()));
    }
    assert_eq!(&*fixture.scopes.borrow(), &["", "item", "line"]);
    assert!(
        fixture
            .queries
            .borrow()
            .iter()
            .all(|query| query.0 == schema::OP_STATUS)
    );
}

#[test]
fn status_discovery_checks_empty_pages_unknown_kinds_and_scope_mismatch() {
    let table = table();
    let mut fixture = Fixture::default();
    let host = host(&table, &mut fixture);
    let registry = Registry::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    *fixture.response.borrow_mut() = encode(&[end(0, 0, 1, true)]);
    assert_eq!(
        registry
            .status(
                &host,
                StatusScope::All,
                PageCursor::start(),
                &mut input,
                &mut out
            )
            .unwrap()
            .len(),
        0
    );
    for (scope, kind, flags) in [
        (StatusScope::All, 2, 0),
        (StatusScope::Items, 1, 1),
        (StatusScope::Lines, 0, 0),
        (StatusScope::All, 1, 0),
    ] {
        *fixture.response.borrow_mut() = encode(&[
            Record::ServiceRegStatus(wire::ServiceRegStatus {
                id: "fixture.clock",
                plugin: "fixture",
                kind,
                flags,
                priority: 0,
                size: 1000,
                tap: "",
                title: "Clock",
            }),
            end(1, 1, 1, true),
        ]);
        assert!(matches!(
            registry.status(&host, scope, PageCursor::start(), &mut input, &mut out),
            Err(Error::Invalid)
        ));
    }
}

#[test]
fn menu_user_definition_is_read_from_the_explicit_flag() {
    for flags in [0, 31, 32, 63, 1 << 31] {
        let row = wire::ServiceRegItem {
            id: "demo.menu",
            plugin: "demo",
            title: "Menu",
            flags,
        };
        let mut bytes = [0; 512];
        let n = wire::record_encode_of(&row, &mut bytes).unwrap();
        let item = RegistryItem::decode(wire::RawRecord::one(&bytes[..n]).unwrap()).unwrap();
        assert_eq!(item.user_defined(), flags & 32 != 0);
    }
}

#[test]
fn a_group_asks_for_the_widgets_that_preview_it_with_bit_zero_and_reads_what_they_say() {
    let table = table();
    let mut fixture = Fixture::default();
    let widget = |id, previews| {
        Record::ServiceRegWidget(wire::ServiceRegWidget {
            id,
            plugin: "widgets",
            kind: "clock",
            icon: "",
            flags: 0,
            title: id,
            cells: 0,
            previews,
        })
    };
    *fixture.response.borrow_mut() = encode(&[
        widget("widgets.look", "widgets.look"),
        widget("widgets.plain", ""),
        end(2, 2, 3, true),
    ]);
    let host = host(&table, &mut fixture);
    let registry = Registry::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    let page = registry
        .widgets_previewing(
            &host,
            "widgets.look",
            PageCursor::start(),
            &mut input,
            &mut out,
        )
        .unwrap();
    let rows: Vec<_> = page.rows().collect();
    assert_eq!(rows[0].previews(), "widgets.look");
    assert_eq!(
        rows[1].previews(),
        "",
        "none for a widget that previews nothing"
    );
    assert_eq!(&*fixture.queries.borrow(), &[(schema::OP_WIDGETS, 0, 0, 1)]);
    assert_eq!(&*fixture.scopes.borrow(), &[String::from("widgets.look")]);
    // An empty group is refused before the host is called; an empty page is no preview.
    let calls = fixture.calls.get();
    assert_eq!(
        registry
            .widgets_previewing(&host, "", PageCursor::start(), &mut input, &mut out)
            .map(|page| page.len()),
        Err(Error::Invalid)
    );
    assert_eq!(fixture.calls.get(), calls);
    *fixture.response.borrow_mut() = encode(&[end(0, 0, 3, true)]);
    assert!(
        registry
            .widgets_previewing(
                &host,
                "widgets.look",
                PageCursor::start(),
                &mut input,
                &mut out
            )
            .unwrap()
            .is_empty()
    );
}
