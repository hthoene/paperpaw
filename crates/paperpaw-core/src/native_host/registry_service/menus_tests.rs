//! `core.registry` ops 11, 12, 13, 15, 19 and 21 (`registry.md` 2.2, 2.4, 3, 6):
//! framing and paging, scope, flags, and what each row carries. Rows of code
//! plugins come from the environment, rows of native plugins from a
//! declaration store the fixtures fill.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
extern crate std;

use super::{
    schema,
    tests::{call_in, page, query_flags, read},
};
use crate::native_host::{
    declaration_store::{Allocations, DeclarationStore, ImageOwner, PendingDeclarations},
    native_declarations::NativeDeclarations,
    registrations::{self as wire, Record},
};
use alloc::{borrow::Cow, boxed::Box, format, rc::Rc, string::String, vec, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID};
use paperpaw_sdk::{
    ActionSpec, Canvas, Cx, Env, Handled, Id, ItemState, Manifest, MenuEntry, MenuNeed,
    MenuSection, Plugin, RegisterError, Registrar, Screen, ScreenSpec, SettingKind, SettingSpec,
    SettingValue, Version,
    setup::{SetupPage, SetupStepSpec},
    widgets::{CellSpan, Drawn, PlaceKind, WidgetPlace, WidgetSpec, categories},
};

fn id(text: &str) -> Id {
    Id::parse(text).unwrap()
}

struct Quiet;
impl Screen for Quiet {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

const LISTS: &[Id] = &[Id::new_static("demo.list")];
const OFF_LISTS: &[Id] = &[Id::new_static("demo.off-list")];

/// Everything the ops read from the environment.
struct Demo;
impl Plugin for Demo {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("demo"), "Demo", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            id("demo.main"),
            "demo.screen.main",
            Box::new(|_, _| Box::new(Quiet)),
        ))?;
        for name in [
            "open", "save", "hide", "late", "needy", "switchy", "extra", "sub", "long",
        ] {
            r.add_action(ActionSpec::new(
                id(&format!("demo.{name}")),
                match name {
                    "open" => "demo.action.open",
                    _ => "demo.action.other",
                },
            ))?;
        }
        let group = paperpaw_sdk::builtin::groups::OTHER;
        r.add_setting(SettingSpec::new(
            id("demo.flag"),
            "demo.setting.flag",
            SettingKind::Bool,
            SettingValue::Bool(false),
            group.clone(),
        ))?;
        for (name, value) in [
            ("list", "demo.note@2"),
            ("off-list", "demo.clock"),
            ("list2", "nat.none"),
            ("mode", "off"),
        ] {
            r.add_setting(SettingSpec::new(
                id(&format!("demo.{name}")),
                "demo.setting.list",
                SettingKind::Text,
                SettingValue::Text(Cow::Borrowed(value)),
                group.clone(),
            ))?;
        }
        register_menus(r)?;
        register_ui(r)?;
        Ok(())
    }
}

fn register_menus(r: &mut Registrar) -> Result<(), RegisterError> {
    let menu = id("demo.menu");
    for (item, order) in [
        ("demo.open", 10),
        ("demo.save", 20),
        ("demo.hide", 30),
        ("demo.switchy", 35),
        ("demo.late", 40),
        ("demo.needy", 50),
        ("demo.flag", 60),
        ("demo.sub", 70),
        ("demo.long", 80),
    ] {
        r.add_menu_entry(MenuEntry::new(menu.clone(), id(item)).order(order))?;
    }
    r.add_menu_entry(MenuEntry::new(id("demo.sub"), id("demo.extra")))?;
    r.add_menu_section(
        MenuSection::new(id("demo.part"), "demo.section.part", menu.clone()).order(25),
    )?;
    r.hide_menu_entry(menu, id("demo.hide"))?;
    r.set_menu_need(id("demo.needy"), MenuNeed::Speech)?;
    r.set_item_state(id("demo.switchy"), |_| {
        ItemState::new().checked(true).label("Switched").badge("3")
    })?;
    r.set_item_state(id("demo.late"), |_| ItemState::new().shown(false))?;
    r.set_item_state(id("demo.long"), |_| {
        ItemState::new().enabled(false).label("é".repeat(100))
    })?;
    r.describe_item(id("demo.open"), "demo.line.open")?;
    r.add_icon(id("demo.open"), "gear")?;
    r.add_keymap("[keymap.global]\ntap.center = demo.open\nswipe.left = none\n[keymap.demo.main]\ntap.center = demo.save\n[keymap.demo.custom]\nswipe.up = demo.open\n")?;
    Ok(())
}

fn register_ui(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_widget(
        WidgetSpec::drawn(
            id("demo.clock"),
            "demo.widget.clock",
            Drawn::new(|_, _| 10, |_, _, _| {}),
        )
        .category(categories::TIME)
        .cells(CellSpan::new(2, 1).min(1, 1)),
    )?;
    r.add_widget(
        WidgetSpec::drawn(
            id("demo.note"),
            "demo.widget.note",
            Drawn::new(|_, _| 10, |_, _, _| {}),
        )
        .cells(CellSpan::new(2, 2).square()),
    )?;
    r.add_widget_place(WidgetPlace::new(
        id("demo.home"),
        "demo.place.home",
        LISTS,
        PlaceKind::Grid,
    ))?;
    r.add_widget_place(
        WidgetPlace::new(
            id("demo.off"),
            "demo.place.off",
            OFF_LISTS,
            PlaceKind::Stack,
        )
        .shown_if(id("demo.mode"), "on"),
    )?;
    r.add_setup_step(
        SetupStepSpec::new(
            id("demo.two"),
            "demo.step.two",
            SetupPage::Setting(id("demo.flag")),
        )
        .order(20),
    )?;
    r.add_setup_step(
        SetupStepSpec::new(id("demo.one"), "", SetupPage::Screen(id("demo.main")))
            .order(10)
            .applies(Box::new(|_| true)),
    )?;
    r.add_setup_step(
        SetupStepSpec::new(
            id("demo.never"),
            "demo.step.never",
            SetupPage::Screen(id("demo.main")),
        )
        .order(5)
        .applies(Box::new(|_| false)),
    )?;
    Ok(())
}

/// The actions of the plugin the native rows belong to: its `action.spec`
/// rows are merged into the registries when it installs.
struct Nat;
impl Plugin for Nat {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("nat"), "Nat", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for name in ["first", "go", "last", "speaks", "future"] {
            r.add_action(ActionSpec::new(id(&format!("nat.{name}")), "nat.action"))?;
        }
        Ok(())
    }
}

fn kernel() -> crate::Kernel {
    crate::Kernel::builder(crate::tests::DEVICE)
        .device_keymap("[keymap.global]\nswipe.right = demo.save\n")
        .plugin(Demo)
        .plugin(Nat)
        .build()
        .unwrap()
}

struct Image(Vec<u8>);
impl ImageOwner for Image {
    fn plugin_id(&self) -> &str {
        "nat"
    }
    fn generation(&self) -> u32 {
        1
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(Some(&self.0));
    }
    fn invalid_declaration(&self, _: i32) {}
}

/// A declaration store holding `rows`, all of plugin `nat`.
pub(super) fn declarations(rows: &[Record<'_>]) -> NativeDeclarations {
    let mut allocations = Allocations::default();
    let mut pending = PendingDeclarations::new(
        Id::new_static("nat"),
        Rc::new(Image(Vec::new())) as Rc<dyn ImageOwner>,
    );
    // One batch holds a few records.
    for batch in rows.chunks(wire::REGISTRATION_MAX_RECORDS) {
        let mut bytes = vec![0; wire::REGISTRATION_MAX_BYTES];
        let used = wire::records_encode(batch, &mut bytes).unwrap();
        pending
            .stage_batch(&bytes[..used], &mut allocations)
            .unwrap();
    }
    let mut store = DeclarationStore::default();
    store.prepare(pending, &mut allocations).unwrap().commit();
    NativeDeclarations {
        store: RefCell::new(store),
        catalog: Default::default(),
        members: Default::default(),
        items: Default::default(),
        item_room: Default::default(),
        refusals: Default::default(),
    }
}

fn entry(item: &'static str, order: i32, place: u32) -> Record<'static> {
    Record::MenuEntry(wire::MenuEntry {
        menu: "demo.menu",
        item,
        order,
        place,
        anchor: "",
    })
}

/// What natively declared plugins put into `demo.menu`, and more.
fn native_menus() -> NativeDeclarations {
    declarations(&[
        entry("nat.first", 0, 4),
        entry("nat.go", 15, 0),
        entry("nat.last", 0, 5),
        entry("nat.speaks", 5, 0),
        entry("nat.future", 0, 99),
        Record::MenuNeed(wire::MenuNeed {
            item: "nat.speaks",
            need: 0,
            capability: 0,
        }),
        Record::MenuSection(wire::MenuSection {
            name: "head",
            title: "nat.heading",
            menu: "demo.menu",
            order: 5,
        }),
        Record::MenuHide(wire::MenuHide {
            menu: "demo.menu",
            item: "demo.save",
        }),
        Record::MenuDescribe(wire::MenuDescribe {
            item: "go",
            key: "nat.line.go",
        }),
        Record::IconSpec(wire::IconSpec {
            item: "nat.go",
            icon: "moon",
        }),
        Record::MenuEntry(wire::MenuEntry {
            menu: "nat.menu",
            item: "nat.go",
            order: 0,
            place: 0,
            anchor: "",
        }),
    ])
}

fn rows_of(
    kernel: &crate::Kernel,
    directory: Option<&NativeDeclarations>,
    id: u32,
    scope: &str,
    flags: u32,
) -> Vec<String> {
    let mut all = Vec::new();
    let (mut cursor, mut generation) = (0, 0);
    loop {
        let reply = call_in(
            kernel,
            directory,
            id,
            &query_flags(scope, cursor, generation, flags),
            4096,
        )
        .unwrap();
        let (rows, end) = read(&reply);
        let end = end.unwrap();
        all.extend(rows);
        if end.flags & 1 != 0 {
            assert_eq!(end.next, end.total);
            return all;
        }
        (cursor, generation) = (end.next, end.generation);
    }
}

/// The item ids of a menu's rows.
fn items(rows: &[String]) -> Vec<&str> {
    rows.iter()
        .map(|row| row.split(' ').nth(1).unwrap())
        .collect()
}

fn row<'a>(rows: &'a [String], item: &str) -> &'a str {
    rows.iter()
        .find(|row| row.split(' ').nth(1) == Some(item))
        .unwrap_or_else(|| panic!("{item} in {rows:?}"))
}

const ALL: u32 = 1;
const DECLARED: u32 = 2;
const USER: u32 = 1 << 4;

#[test]
fn op_11_lists_every_menu_once_and_flags_the_users_own() {
    let mut kernel = kernel();
    let directory = native_menus();
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENUS, "", 0);
    // The host's own menus, the plugins' (sorted), then the ones only
    // declarations name (`nat.menu`); each once.
    let ours = [
        "item demo.menu demo 0 ",
        "item demo.sub demo 0 demo.action.other",
        "item nat.menu nat 0 ",
    ];
    assert!(rows.contains(&"item core.main core 0 ".into()), "{rows:?}");
    assert_eq!(&rows[rows.len() - 3..], ours);
    let count = rows.len();
    let mut once = rows.clone();
    once.sort();
    once.dedup();
    assert_eq!(once.len(), count);
    kernel.shared.menus.customize(id("demo.sub"), vec![]);
    kernel.shared.menus.customize(id("user.empty"), vec![]);
    kernel
        .shared
        .menus
        .customize(id("nat.menu"), vec![id("demo.open")]);
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENUS, "", 0);
    // Bit 5 where the user has a definition, even an empty one no plugin
    // named; a declared menu is flagged too.
    assert_eq!(rows.len(), count + 1);
    assert_eq!(
        &rows[rows.len() - 4..],
        [
            "item demo.menu demo 0 ",
            "item demo.sub demo 32 demo.action.other",
            "item nat.menu nat 32 ",
            "item user.empty user 32 ",
        ]
    );
    // Without a store the environment's menus alone.
    let rows = rows_of(&kernel, None, schema::OP_MENUS, "", 0);
    assert_eq!(rows.len(), count + 1);
    let (rows, end) = page(&kernel, schema::OP_MENUS, "");
    assert_eq!(
        (end.flags, end.total as usize, rows.len()),
        (1, count + 1, count + 1)
    );
}

#[test]
fn op_12_default_view_is_the_declared_rows_the_device_shows() {
    let kernel = kernel();
    let directory = native_menus();
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", 0);
    // Placed first; the environment's in order (without the hidden save, the
    // faint-hidden late and the speech-only needy); the rest by order, a
    // place this host does not know left out, placed last last. The native
    // rows stand among the environment's by their order (Beta 3 sorted all
    // rows by order: the native Edit home screen at 40 sat before Settings).
    assert_eq!(
        items(&rows),
        [
            "nat.first",
            "nat.head",
            "demo.open",
            "nat.go",
            "demo.part",
            "demo.switchy",
            "demo.flag",
            "demo.sub",
            "demo.long",
            "nat.last",
        ]
    );
    // checked 1, enabled 2, declared 32, switch 64; the label and badge are
    // the pushed state, the line and the icon the declarations'.
    assert_eq!(
        row(&rows, "demo.open"),
        "row demo.open kind=0 flags=34 order=10 by=demo title=demo.action.open icon=gear badge= describe=demo.line.open"
    );
    assert_eq!(
        row(&rows, "demo.switchy"),
        "row demo.switchy kind=0 flags=99 order=35 by=demo title=Switched icon= badge=3 describe="
    );
    // A Boolean setting is a switch that is off; a heading and a submenu have their kinds.
    assert!(row(&rows, "demo.flag").starts_with("row demo.flag kind=1 flags=98 "));
    assert!(row(&rows, "demo.part").starts_with("row demo.part kind=3 flags=34 "));
    assert!(row(&rows, "demo.sub").starts_with("row demo.sub kind=2 flags=34 "));
    assert_eq!(
        row(&rows, "nat.head"),
        "row nat.head kind=3 flags=34 order=5 by=nat title=nat.heading icon= badge= describe="
    );
    assert_eq!(
        row(&rows, "nat.go"),
        "row nat.go kind=0 flags=34 order=15 by=nat title=nat.action icon=moon badge= describe=nat.line.go"
    );
    // Faint, and a label of 200 bytes cut at a character.
    let long = row(&rows, "demo.long");
    assert!(long.contains(&format!(
        "flags=32 order=80 by=demo title={} ",
        "é".repeat(64)
    )));
    // Without the store only the environment's rows.
    let rows = rows_of(&kernel, None, schema::OP_MENU, "demo.menu", 0);
    assert_eq!(items(&rows)[0], "demo.open");
    assert!(!items(&rows).contains(&"nat.go") && items(&rows).contains(&"demo.save"));
}

#[test]
fn op_12_negative_orders_keep_their_groups() {
    let kernel = kernel();
    // Placed first, by order and placed last, each with a negative order.
    let directory = declarations(&[
        entry("nat.last", -5, 5),
        entry("nat.go", -5, 0),
        entry("nat.first", -5, 4),
    ]);
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", 0);
    assert_eq!(
        items(&rows),
        [
            "nat.first",
            "nat.go",
            "demo.open",
            "demo.save",
            "demo.part",
            "demo.switchy",
            "demo.flag",
            "demo.sub",
            "demo.long",
            "nat.last",
        ]
    );
}

#[test]
fn op_12_all_adds_the_hidden_rows_and_flags_them() {
    let kernel = kernel();
    let directory = native_menus();
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", ALL);
    // The hidden ones keep their place; one a plugin of the environment hid
    // (`demo.hide`) comes after the others; one the device cannot offer
    // (`needy`, `speaks`) never.
    assert_eq!(
        items(&rows),
        [
            "nat.first",
            "nat.head",
            "demo.open",
            "nat.go",
            "demo.save",
            "demo.part",
            "demo.switchy",
            "demo.late",
            "demo.flag",
            "demo.sub",
            "demo.long",
            "nat.last",
            "demo.hide",
        ]
    );
    // Hidden 4, and `by` the plugin that hides it.
    assert!(row(&rows, "demo.save").starts_with("row demo.save kind=0 flags=6 order=20 by=nat "));
    assert!(row(&rows, "demo.late").starts_with("row demo.late kind=0 flags=6 order=40 by=demo "));
    assert!(row(&rows, "demo.hide").starts_with("row demo.hide kind=0 flags=6 order=30 by=demo "));
}

#[test]
fn op_12_a_users_definition_replaces_the_declared_rows() {
    let mut kernel = kernel();
    let directory = native_menus();
    kernel.shared.menus.customize(
        id("demo.menu"),
        vec![
            id("demo.save"),
            id("demo.late"),
            id("demo.extra"),
            id("nat.go"),
        ],
    );
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", 0);
    assert_eq!(
        items(&rows),
        ["demo.save", "demo.late", "demo.extra", "nat.go"]
    );
    // User menu 16; declared 32 only for what the plugins declare and show:
    // not the save that `menu.hide` takes out of the default set, nor an
    // action nobody put there. A state that hides does not hide it here.
    assert!(row(&rows, "demo.save").contains(&format!("flags={} ", USER | 2)));
    assert!(row(&rows, "demo.late").contains(&format!("flags={} ", USER | 2 | 32)));
    assert!(row(&rows, "demo.extra").contains(&format!("flags={} ", USER | 2)));
    assert!(row(&rows, "nat.go").contains(&format!("flags={} ", USER | 2 | 32)));
    // `all`: the user's rows, then the declared ones it leaves out (removed
    // 8), then the hidden ones; none twice.
    let all = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", ALL);
    assert_eq!(
        items(&all),
        [
            "demo.save",
            "demo.late",
            "demo.extra",
            "nat.go",
            "nat.first",
            "nat.head",
            "demo.open",
            "demo.part",
            "demo.switchy",
            "demo.flag",
            "demo.sub",
            "demo.long",
            "nat.last",
            "demo.hide",
        ]
    );
    assert!(row(&all, "demo.open").contains(&format!("flags={} ", USER | 2 | 8 | 32)));
    assert!(row(&all, "demo.hide").contains(&format!("flags={} ", USER | 2 | 4)));
    // `declared`: the declared order whatever the user chose, and no user flag.
    let declared = rows_of(
        &kernel,
        Some(&directory),
        schema::OP_MENU,
        "demo.menu",
        DECLARED,
    );
    let plain = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", 0);
    assert_ne!(declared, plain);
    assert_eq!(items(&declared)[0], "nat.first");
    assert!(
        declared
            .iter()
            .all(|row| !row.contains(&format!("flags={USER}")))
    );
    assert!(row(&declared, "demo.open").contains("flags=34 "));
    // An empty definition is empty (and op 11 says it is the user's).
    kernel.shared.menus.customize(id("demo.menu"), vec![]);
    let (rows, end) = page(&kernel, schema::OP_MENU, "demo.menu");
    assert!(rows.is_empty() && (end.flags, end.total) == (1, 0));
}

#[test]
fn op_12_unknown_or_empty_scope_is_an_empty_page() {
    let kernel = kernel();
    let directory = native_menus();
    for scope in ["", "nobody.menu", "demo.open"] {
        let reply = call_in(
            &kernel,
            Some(&directory),
            schema::OP_MENU,
            &query_flags(scope, 0, 0, ALL),
            4096,
        )
        .unwrap();
        let (rows, end) = read(&reply);
        let end = end.unwrap();
        assert!(rows.is_empty(), "{scope}");
        assert_eq!((end.flags, end.total, end.next), (1, 0, 0), "{scope}");
        assert_ne!(end.generation, 0);
    }
    // A menu only declarations name is known; its rows are the declared ones.
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "nat.menu", 0);
    assert_eq!(items(&rows), ["nat.go"]);
}

#[test]
fn op_12_pages_by_cursor_and_a_stale_generation_is_a_state_error() {
    struct Many;
    impl Plugin for Many {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static("many"), "Many", Version::new(1, 0, 0))
                .uses(paperpaw_sdk::Uses::NOTHING)
        }
        fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
            for at in 0..40 {
                let item = id(&format!("many.item{at:02}"));
                r.add_action(ActionSpec::new(item.clone(), "many.action"))?;
                r.add_menu_entry(MenuEntry::new(id("many.menu"), item).order(at))?;
            }
            Ok(())
        }
    }
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Many)
        .build()
        .unwrap();
    let first = call_in(
        &kernel,
        None,
        schema::OP_MENU,
        &query_flags("many.menu", 0, 0, 0),
        4096,
    )
    .unwrap();
    let (rows, end) = read(&first);
    let end = end.unwrap();
    assert_eq!(
        (rows.len(), end.flags, end.total, end.next),
        (32, 0, 40, 32)
    );
    let more = call_in(
        &kernel,
        None,
        schema::OP_MENU,
        &query_flags("many.menu", end.next, end.generation, 0),
        4096,
    )
    .unwrap();
    let (more, last) = read(&more);
    assert_eq!((more.len(), last.unwrap().flags), (8, 1));
    assert_eq!(items(&more)[0], "many.item32");
    assert_eq!(
        call_in(
            &kernel,
            None,
            schema::OP_MENU,
            &query_flags("many.menu", 32, end.generation + 1, 0),
            4096
        ),
        Err(paperpaw_native::host_glue::ERR_STATE)
    );
    // A page that holds whole rows: a small buffer gives fewer, never a cut one.
    let small = call_in(
        &kernel,
        None,
        schema::OP_MENU,
        &query_flags("many.menu", 0, 0, 0),
        900,
    )
    .unwrap();
    let (some, end) = read(&small);
    assert!(some.len() < 32 && !some.is_empty());
    assert_eq!(end.unwrap().next as usize, some.len());
}

#[test]
fn op_12_titles_follow_the_users_language() {
    let mut kernel = kernel();
    kernel.shared.translator.add(
        paperpaw_i18n::Catalog::parse(Cow::Borrowed(
            "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\n\
             demo.action.open = Oeffnen\ndemo.line.open = Eine Zeile\n",
        ))
        .unwrap(),
    );
    kernel.shared.translator.set_language("de");
    let rows = rows_of(&kernel, None, schema::OP_MENU, "demo.menu", 0);
    assert!(row(&rows, "demo.open").contains("title=Oeffnen icon=gear badge= describe=Eine Zeile"));
}

/// What `plugin` pushed for `item`, stored as the host does after a callback.
fn push(
    directory: &NativeDeclarations,
    plugin: &str,
    item: &str,
    flags: u32,
    label: &str,
    badge: &str,
) {
    let push =
        crate::native_host::item_state::ItemPush::copied(item, flags, (label, b""), (badge, b""))
            .unwrap();
    let mut states = directory.items.borrow_mut();
    states.reserve(plugin, 1).unwrap();
    states.apply(plugin, push).unwrap();
}

#[test]
fn op_12_shows_what_the_plugin_pushed_and_the_pushed_state_wins() {
    use crate::native_host::item_state::{FAINT, HIDDEN, ON, SWITCH};
    let mut kernel = kernel();
    kernel.shared.translator.add(
        paperpaw_i18n::Catalog::parse(Cow::Borrowed(
            "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\n\
             demo.action.open = Oeffnen\ndemo.line.open = Eine Zeile\n",
        ))
        .unwrap(),
    );
    kernel.shared.translator.set_language("de");
    let directory = native_menus();
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", 0);
    assert!(
        row(&rows, "demo.open").contains("flags=34 "),
        "before a push"
    );
    // Faint and a label and a badge: enabled goes, the texts are the catalog's.
    push(
        &directory,
        "demo",
        "open",
        FAINT,
        "demo.line.open",
        "demo.action.open",
    );
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", 0);
    assert_eq!(
        row(&rows, "demo.open"),
        "row demo.open kind=0 flags=32 order=10 by=demo title=Eine Zeile icon=gear badge=Oeffnen describe=Eine Zeile"
    );
    // A switch that is on: checked and switch.
    push(&directory, "demo", "open", SWITCH | ON, "", "");
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", 0);
    assert!(row(&rows, "demo.open").contains("flags=99 "), "{rows:?}");
    // A key the catalogs no longer have: the title, no badge, never a raw key.
    push(&directory, "demo", "open", FAINT, "gone.key", "gone.badge");
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", 0);
    assert!(
        row(&rows, "demo.open").contains("title=Oeffnen "),
        "{rows:?}"
    );
    assert!(row(&rows, "demo.open").contains("badge= "));
    // Hidden: out of the default view, listed hidden in the editor's view.
    push(&directory, "demo", "open", HIDDEN, "", "");
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", 0);
    assert!(!items(&rows).contains(&"demo.open"));
    let all = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", ALL);
    assert!(
        row(&all, "demo.open").contains("flags=") && row(&all, "demo.open").contains(" kind=0 ")
    );
    // The default state removes the push: the item shows as declared again.
    push(&directory, "demo", "open", 0, "", "");
    let rows = rows_of(&kernel, Some(&directory), schema::OP_MENU, "demo.menu", 0);
    assert!(row(&rows, "demo.open").contains("flags=34 "));
}

#[test]
fn op_13_lists_layers_with_origin_and_nothing_for_an_unknown_one() {
    let kernel = kernel();
    let global = rows_of(&kernel, None, schema::OP_BINDINGS, "global", 0);
    // A plugin's, one bound to nothing on purpose, and the device's.
    assert!(
        global.contains(&"bind global tap.center demo.open 0".into()),
        "{global:?}"
    );
    assert!(global.contains(&"bind global swipe.left  0".into()));
    assert!(global.contains(&"bind global swipe.right demo.save 1".into()));
    let screen = rows_of(&kernel, None, schema::OP_BINDINGS, "demo.main", 0);
    assert_eq!(screen, ["bind demo.main tap.center demo.save 0"]);
    // No scope: every layer with a binding, in the keymap's sorted order,
    // however its name came about: a screen's, `global`, and one that is
    // neither a screen nor a slot.
    let all = rows_of(&kernel, None, schema::OP_BINDINGS, "", 0);
    let layers: Vec<&str> = all.iter().filter_map(|row| row.split(' ').nth(1)).collect();
    assert!(layers.is_sorted(), "{layers:?}");
    assert!(all.contains(&"bind demo.custom swipe.up demo.open 0".into()));
    assert!(all.contains(&"bind demo.main tap.center demo.save 0".into()));
    assert!(all.iter().any(|row| row.starts_with("bind global ")));
    // Each layer is read once.
    assert_eq!(all.iter().filter(|row| **row == screen[0]).count(), 1);
    assert!(all.len() >= global.len() + screen.len());
    let (rows, end) = page(&kernel, schema::OP_BINDINGS, "nobody.layer");
    assert!(rows.is_empty() && (end.flags, end.total) == (1, 0));
}

#[test]
fn op_15_lists_every_step_in_order_with_what_applies_merged_with_the_declared_ones() {
    let kernel = kernel();
    let step = |name, order, flags, key, value, need, capability| {
        Record::SetupStep(wire::SetupStep {
            name,
            title: "nat.step",
            help: "",
            order,
            page_kind: 0,
            page: "demo.flag",
            applies_key: key,
            applies_value: value,
            need,
            capability,
            flags,
        })
    };
    let directory = declarations(&[
        // Applies: no condition, no need.
        step("plain", 15, 1, "", "", 0, 0),
        // A condition that does not hold (demo.flag is off).
        step("when", 12, 1, "demo.flag", "true", 0, 0),
        // The declaration's own bit says it does not apply before a push.
        step("later", 13, 0, "", "", 0, 0),
        // A need the device does not meet (speech), and one it does (touch).
        step("speaks", 14, 3, "", "", 0, 0),
        step("touch", 20, 3, "", "", 4, 0),
        // A condition that holds.
        step("off", 5, 1, "demo.flag", "false", 0, 0),
    ]);
    let rows = rows_of(&kernel, Some(&directory), schema::OP_SETUP, "", 0);
    // The environment's go first where the order is equal (`demo.two`, 20).
    assert_eq!(
        rows,
        [
            // A code plugin's step that does not apply is listed, bit 0 clear.
            "step demo.never 1 demo.main 5 0 demo.step.never",
            "step nat.off 0 demo.flag 5 1 nat.step",
            // The host's own steps are the environment's too.
            "step core.setup.language 0 core.language 10 1 Language",
            // The title of an empty key is the setting's own; here a screen's: none.
            "step demo.one 1 demo.main 10 1 ",
            "step nat.when 0 demo.flag 12 0 nat.step",
            "step nat.later 0 demo.flag 13 0 nat.step",
            "step nat.speaks 0 demo.flag 14 0 nat.step",
            "step nat.plain 0 demo.flag 15 1 nat.step",
            // The host's own step for a device with buttons only: not here.
            "step core.setup.buttons 0 core.keymap-preset 20 0 Button layout",
            "step demo.two 0 demo.flag 20 1 demo.step.two",
            "step nat.touch 0 demo.flag 20 1 nat.step",
            "step core.setup.time-zone 0 core.time-zone 40 1 Time zone",
        ]
    );
    // The environment's steps alone; the step that never applies is listed too.
    let rows = rows_of(&kernel, None, schema::OP_SETUP, "", 0);
    assert_eq!(rows.len(), 6);
    assert!(
        rows.iter()
            .any(|row| row.starts_with("step demo.never ") && row.contains(" 5 0 "))
    );
    let (rows, end) = page(&kernel, schema::OP_SETUP, "ignored");
    assert_eq!((rows.len(), end.total), (6, 6));
}

#[test]
fn op_15_applies_by_the_plugins_last_push_before_the_declaration() {
    use crate::native_host::item_state::{FAINT, HIDDEN};
    let kernel = kernel();
    let step = |name, flags| {
        Record::SetupStep(wire::SetupStep {
            name,
            title: "nat.step",
            help: "",
            order: 15,
            page_kind: 0,
            page: "demo.flag",
            applies_key: "",
            applies_value: "",
            need: 0,
            capability: 0,
            flags,
        })
    };
    // `plain` applies by its declaration, `later` does not.
    let directory = declarations(&[step("plain", 1), step("later", 0)]);
    let applies = |directory: &NativeDeclarations, name: &str| {
        let rows = rows_of(&kernel, Some(directory), schema::OP_SETUP, "", 0);
        rows.iter()
            .find(|row| row.starts_with(&format!("step nat.{name} ")))
            .map(|row| row.split(' ').nth(5).unwrap() == "1")
            .unwrap()
    };
    assert_eq!(
        (applies(&directory, "plain"), applies(&directory, "later")),
        (true, false)
    );
    // The push wins: shown (not hidden) applies, hidden does not.
    push(&directory, "nat", "later", FAINT, "", "");
    push(&directory, "nat", "plain", HIDDEN, "", "");
    assert_eq!(
        (applies(&directory, "plain"), applies(&directory, "later")),
        (false, true)
    );
    // Without a push, as declared again.
    push(&directory, "nat", "later", 0, "", "");
    push(&directory, "nat", "plain", 0, "", "");
    assert_eq!(
        (applies(&directory, "plain"), applies(&directory, "later")),
        (true, false)
    );
}

fn widget(
    name: &'static str,
    category: &'static str,
    cells: u32,
    flags: u32,
    previews: &'static str,
) -> Record<'static> {
    Record::WidgetSpec(wire::WidgetSpec {
        name,
        title: "nat.widget",
        category,
        previews,
        cells,
        token: 0,
        flags,
    })
}

#[test]
fn op_19_lists_widgets_with_kind_icon_cells_and_whether_a_place_shows_them() {
    let kernel = kernel();
    let directory = declarations(&[
        // 2 by 1, least 1 by 1; private.
        widget("quote", "core.widgets.words", 0x0101_0102, 2, ""),
        // A widget that previews settings is no widget to add.
        widget("preview", "core.widgets.words", 0, 0, "demo.top"),
        widget("round", "nat.own-kind", 0x0202_0202, 4, ""),
        // A place that lists `quote`, one that lists nothing here.
        Record::WidgetPlace(wire::WidgetPlace {
            name: "home",
            title: "nat.place",
            list_keys: "demo.list\ndemo.setting-nobody-has",
            layout: 0,
            shown_if_key: "",
            shown_if_value: "",
        }),
    ]);
    let rows = rows_of(&kernel, Some(&directory), schema::OP_WIDGETS, "", 0);
    // The environment's (the host's own first), then the declared ones in
    // the store's order; `demo.note@2` is listed by an environment place that
    // shows, `demo.clock` by one that does not.
    assert_eq!(
        rows,
        [
            "widget core.space core core.widgets.layout layout 0 cells=0 Space",
            "widget core.fill core core.widgets.layout layout 0 cells=0 Free space",
            "widget demo.clock demo core.widgets.time clock 0 cells=16843010 demo.widget.clock",
            "widget demo.note demo core.widgets.other puzzle 5 cells=16843266 demo.widget.note",
            // The place `nat.home` lists demo.list: `demo.note@2` only. The
            // declared ones in declaration order (#2390).
            "widget nat.quote nat core.widgets.words letter-case 2 cells=16843010 nat.widget",
            "widget nat.round nat nat.own-kind puzzle 4 cells=33686018 nat.widget",
        ]
    );
    // A kind as scope.
    let rows = rows_of(
        &kernel,
        Some(&directory),
        schema::OP_WIDGETS,
        "core.widgets.words",
        0,
    );
    assert_eq!(rows.len(), 1);
    assert!(rows[0].starts_with("widget nat.quote "));
    let rows = rows_of(&kernel, Some(&directory), schema::OP_WIDGETS, "nobody", 0);
    assert!(rows.is_empty());
}

#[test]
fn op_19_with_flags_bit_0_lists_the_widgets_that_preview_a_group() {
    let kernel = kernel();
    let directory = declarations(&[
        widget("quote", "core.widgets.words", 0, 0, ""),
        widget("sample", "core.widgets.words", 0, 0, "demo.top"),
        widget("other", "core.widgets.words", 0, 0, "demo.else"),
        widget("later", "core.widgets.words", 0, 0, "demo.top"),
    ]);
    // In op 19's order (declaration order for declarations, #2390); a page
    // shows the last.
    let rows = rows_of(&kernel, Some(&directory), schema::OP_WIDGETS, "demo.top", 1);
    assert_eq!(
        rows,
        [
            "widget nat.sample nat core.widgets.words letter-case 0 cells=0 previews=demo.top \
             nat.widget",
            "widget nat.later nat core.widgets.words letter-case 0 cells=0 previews=demo.top \
             nat.widget",
        ]
    );
    // Without the bit the group is a kind nobody has; previews never show.
    assert!(rows_of(&kernel, Some(&directory), schema::OP_WIDGETS, "demo.top", 0).is_empty());
    let rows = rows_of(&kernel, Some(&directory), schema::OP_WIDGETS, "", 0);
    assert!(rows.iter().all(|row| !row.contains("previews=")));
    assert!(rows_of(&kernel, Some(&directory), schema::OP_WIDGETS, "nobody", 1).is_empty());
    // A group is needed.
    assert_eq!(
        call_in(
            &kernel,
            Some(&directory),
            schema::OP_WIDGETS,
            &query_flags("", 0, 0, 1),
            4096
        ),
        Err(ERR_INVALID)
    );
}

#[test]
fn op_19_shown_follows_the_settings_a_place_lists() {
    let mut kernel = kernel();
    let directory = declarations(&[
        widget("quote", "core.widgets.words", 0, 0, ""),
        Record::WidgetPlace(wire::WidgetPlace {
            name: "home",
            title: "nat.place",
            list_keys: "demo.list2",
            layout: 0,
            shown_if_key: "demo.flag",
            shown_if_value: "true",
        }),
    ]);
    let flags = |kernel: &crate::Kernel| -> (String, String) {
        let rows = rows_of(kernel, Some(&directory), schema::OP_WIDGETS, "", 0);
        (
            String::from(row(&rows, "nat.quote")),
            String::from(row(&rows, "demo.clock")),
        )
    };
    let (quote, clock) = flags(&kernel);
    assert!(
        quote.contains(" core.widgets.words letter-case 0 "),
        "{quote}"
    );
    assert!(clock.contains(" clock 0 "), "{clock}");
    // The place lists `nat.quote` now, and shows while the flag is on.
    kernel
        .shared
        .settings
        .set(
            "demo.list2",
            SettingValue::Text(Cow::Borrowed("nat.quote 2x1")),
        )
        .unwrap();
    let (quote, clock) = flags(&kernel);
    assert!(quote.contains(" letter-case 0 ") && clock.contains(" clock 0 "));
    kernel
        .shared
        .settings
        .set("demo.flag", SettingValue::Bool(true))
        .unwrap();
    kernel
        .shared
        .settings
        .set("demo.mode", SettingValue::Text(Cow::Borrowed("on")))
        .unwrap();
    let (quote, clock) = flags(&kernel);
    assert!(quote.contains(" letter-case 1 "), "{quote}");
    assert!(clock.contains(" clock 1 "), "{clock}");
}

#[test]
fn op_21_is_empty_until_a_plugin_declares_a_status_item_or_line() {
    let kernel = kernel();
    for scope in ["", "item", "line", "other"] {
        let (rows, end) = page(&kernel, schema::OP_STATUS, scope);
        assert!(rows.is_empty(), "{scope}");
        assert_eq!((end.flags, end.total, end.next), (1, 0, 0));
        assert_ne!(end.generation, 0);
    }
    let directory = declarations(&[
        Record::StatusItem(wire::StatusItem {
            name: "battery",
            title: "nat.status.battery",
            tap: "go",
            priority: 200,
            min_width: 1500,
            token: 0,
            flags: 2,
        }),
        Record::StatusLine(wire::StatusLine {
            name: "reading",
            title: "nat.status.reading",
            height: 1000,
            token: 0,
            flags: 0,
        }),
    ]);
    let all = rows_of(&kernel, Some(&directory), schema::OP_STATUS, "", 0);
    assert_eq!(
        all,
        [
            "status nat.battery nat kind=0 flags=2 priority=200 size=1500 tap=nat.go nat.status.battery",
            "status nat.reading nat kind=1 flags=1 priority=0 size=1000 tap= nat.status.reading",
        ]
    );
    let items = rows_of(&kernel, Some(&directory), schema::OP_STATUS, "item", 0);
    assert_eq!(items, &all[..1]);
    let lines = rows_of(&kernel, Some(&directory), schema::OP_STATUS, "line", 0);
    assert_eq!(lines, &all[1..]);
    assert!(rows_of(&kernel, Some(&directory), schema::OP_STATUS, "other", 0).is_empty());
}

#[test]
fn op_14_lists_the_natively_declared_inputs_the_device_meets() {
    let kernel = kernel();
    let spec = |name, kind, need| {
        Record::InputSpec(wire::InputSpec {
            name,
            title: "nat.input.title",
            kind,
            need,
            capability: 0,
            flags: u32::from(need != 0),
        })
    };
    // Touch (4) and no touch (3): the device meets one of them, never both.
    let directory = declarations(&[
        spec("chord", 0, 0),
        spec("shake", 1, 0),
        spec("with-touch", 0, 4),
        spec("without-touch", 0, 3),
    ]);
    let all = rows_of(&kernel, Some(&directory), schema::OP_INPUTS, "", 0);
    // Declared (bit 4); a gesture also bit 2.
    assert!(all.contains(&"item input.nat.chord nat 16 nat.input.title".into()));
    assert!(all.contains(&"item gesture.nat.shake nat 20 nat.input.title".into()));
    let touch = all.iter().any(|row| row.contains("input.nat.with-touch"));
    let no_touch = all
        .iter()
        .any(|row| row.contains("input.nat.without-touch"));
    assert_ne!(touch, no_touch, "{all:?}");
    // The device's own inputs come first.
    assert!(all[0].starts_with("item ") && !all[0].contains(".nat."));
}

#[test]
fn op_14_pages_over_the_device_inputs_and_forty_declared_ones() {
    let kernel = kernel();
    let names: Vec<String> = (0..40).map(|n| format!("in{n}")).collect();
    let records: Vec<Record<'_>> = names
        .iter()
        .map(|name| {
            Record::InputSpec(wire::InputSpec {
                name,
                title: "nat.input.title",
                kind: 0,
                need: 0,
                capability: 0,
                flags: 0,
            })
        })
        .collect();
    let directory = declarations(&records);
    // The first page is full and says more follow, under one generation.
    let first = call_in(
        &kernel,
        Some(&directory),
        schema::OP_INPUTS,
        &query_flags("", 0, 0, 0),
        4096,
    )
    .unwrap();
    let (rows, end) = read(&first);
    let end = end.unwrap();
    assert_eq!((end.flags & 1, rows.len()), (0, 32));
    assert!(
        end.total > 40,
        "the device's inputs come too: {}",
        end.total
    );
    // Every page together is every row once, the declared ones in order.
    let all = rows_of(&kernel, Some(&directory), schema::OP_INPUTS, "", 0);
    assert_eq!(all.len(), end.total as usize);
    let declared: Vec<&String> = all.iter().filter(|row| row.contains(".nat.")).collect();
    assert_eq!(declared.len(), 40);
    for name in &names {
        let id = format!("input.nat.{name} ");
        assert_eq!(
            all.iter().filter(|row| row.contains(&id)).count(),
            1,
            "{name}"
        );
    }
}

#[test]
fn a_borrowed_declaration_store_is_busy_not_a_panic() {
    let kernel = kernel();
    let directory = native_menus();
    let _held = directory.store.borrow_mut();
    for (id, scope) in [
        (schema::OP_MENUS, ""),
        (schema::OP_MENU, "demo.menu"),
        (schema::OP_SETUP, ""),
        (schema::OP_WIDGETS, ""),
        (schema::OP_STATUS, ""),
    ] {
        assert_eq!(
            call_in(
                &kernel,
                Some(&directory),
                id,
                &query_flags(scope, 0, 0, 0),
                4096
            ),
            Err(ERR_BUSY),
            "op {id}"
        );
    }
}

/// Presets a native plugin declared are listed by ops 16 to 18, a key the
/// preset sets back to default with `reg_value` bit 0.
#[test]
fn native_presets_are_listed_with_their_values_and_resets() {
    use crate::abi7::settings_wire::{SettingValueRef, settings_encode};
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let mut specs = kernel.settings().specs();
    let on = String::from(
        specs
            .find(|spec| matches!(spec.kind, SettingKind::Bool))
            .unwrap()
            .id
            .as_str(),
    );
    let other = String::from(
        specs
            .find(|spec| matches!(spec.kind, SettingKind::Bool))
            .unwrap()
            .id
            .as_str(),
    );
    let mut encoded = [core::mem::MaybeUninit::<u8>::uninit(); 32];
    let used = settings_encode(SettingValueRef::Bool(true), &mut encoded).unwrap();
    // SAFETY: the encoder initialised the first `used` bytes.
    let bytes: Vec<u8> = encoded[..used]
        .iter()
        .map(|b| unsafe { b.assume_init() })
        .collect();
    let directory = declarations(&[
        Record::PresetSpec(wire::PresetSpec {
            name: "night",
            kind: "nat.mode",
            title: "Night",
            help: "Dark and quiet.",
        }),
        Record::PresetValue(wire::PresetValue {
            preset: "nat.night",
            key: &on,
            value: &bytes,
            flags: 0,
        }),
        Record::PresetValue(wire::PresetValue {
            preset: "nat.night",
            key: &other,
            value: &[],
            flags: 1,
        }),
        Record::PresetValue(wire::PresetValue {
            preset: "nat.night",
            key: "nat.unknown",
            value: &bytes,
            flags: 0,
        }),
    ]);
    let rows = rows_of(&kernel, Some(&directory), schema::OP_PRESETS, "nat.mode", 0);
    assert_eq!(rows, vec!["item nat.night nat 0 Night"]);
    assert!(
        rows_of(
            &kernel,
            Some(&directory),
            schema::OP_PRESETS,
            "core.other",
            0
        )
        .is_empty()
    );
    let reply = super::tests::call_in(
        &kernel,
        Some(&directory),
        schema::OP_PRESET,
        &super::tests::key("nat.night"),
        4096,
    )
    .unwrap();
    assert_eq!(
        read(&reply).0,
        vec!["item nat.night nat 0 Night", "text Dark and quiet."]
    );
    let values = rows_of(&kernel, Some(&directory), schema::OP_VALUES, "nat.night", 0);
    // The unknown setting is left out; the reset carries bit 0 and no value.
    assert_eq!(values.len(), 2, "{values:?}");
    assert!(
        values
            .iter()
            .any(|v| v.starts_with(&format!("value {on} [")) && v.ends_with(" 0"))
    );
    assert!(
        values.contains(&format!("value {other} [] 1")),
        "{values:?}"
    );
}
