//! Tiles (`menu.style` Tiles), as Beta 3 showed them: a menu of its own whose
//! every entry has an icon opens as the reader's bar without its two lines
//! (Beta 3's `BarScreen`); a submenu, a menu with an entry without an icon or
//! another style opens as the list. The rule and the layout are tested alone;
//! over the scripted host, `menu.list` opened as the main menu is driven by
//! buttons.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_menu::native_bar::{Handed, Hit, Metrics, layout, shows_tiles};
use paperpaw_plugin_menu::native_model::{Kind, Row, STYLE_LIST, STYLE_TILES, flag};
use paperpaw_sdk::Point;

fn row(item: &str, kind: Kind, icon: &str) -> Row {
    Row {
        item: item.to_string(),
        kind,
        flags: flag::ENABLED,
        title: item.to_string(),
        icon: icon.to_string(),
        badge: String::new(),
        describe: String::new(),
    }
}

fn main_menu() -> Handed {
    let mut handed = Handed::default();
    handed.take("menu", "core.main").unwrap();
    handed
}

fn with_icons() -> Vec<Row> {
    vec![
        row("", Kind::Section, ""),
        row("core.library", Kind::Action, "library"),
        row("core.tools", Kind::Submenu, "tools"),
    ]
}

#[test]
fn tiles_need_the_style_a_menu_of_its_own_and_an_icon_on_every_entry() {
    let handed = main_menu();
    let rows = with_icons();
    assert!(shows_tiles(Some(STYLE_TILES), &handed, &rows));
    // Another style, or none that could be read: the list.
    assert!(!shows_tiles(Some(STYLE_LIST), &handed, &rows));
    assert!(!shows_tiles(None, &handed, &rows));
    // An entry without an icon (headings need none): the list.
    let mut plain = with_icons();
    plain.push(row("core.sleep", Kind::Action, ""));
    assert!(!shows_tiles(Some(STYLE_TILES), &handed, &plain));
    // Nothing in it: the list says so.
    assert!(!shows_tiles(Some(STYLE_TILES), &handed, &[]));
}

#[test]
fn submenus_and_the_readers_bar_are_no_tiles() {
    let rows = with_icons();
    let mut submenu = main_menu();
    submenu.take("depth", "1").unwrap();
    assert!(!submenu.may_be_tiles());
    assert!(!shows_tiles(Some(STYLE_TILES), &submenu, &rows));
    // Even a depth of 0 given names a menu opened over another (Beta 3).
    let mut zero = main_menu();
    zero.take("depth", "0").unwrap();
    assert!(!shows_tiles(Some(STYLE_TILES), &zero, &rows));
    let mut reader = main_menu();
    reader.take("mode", "bar").unwrap();
    assert!(!reader.may_be_tiles());
}

#[test]
fn tiles_have_no_title_chapter_page_or_action() {
    let mut handed = main_menu();
    for (key, value) in [
        ("hide", "core.sleep"),
        ("title", "Moby-Dick"),
        ("text", "Chapter 1"),
        ("page", "12 / 30"),
        ("action", "reader.book-info"),
    ] {
        handed.take(key, value).unwrap();
    }
    let tiles = handed.tiles().unwrap();
    assert_eq!(tiles.menu, "core.main");
    assert_eq!(tiles.hide, "core.sleep");
    assert!(tiles.title.is_empty() && tiles.text.is_empty() && tiles.page.is_empty());
    assert!(tiles.action.is_empty());
    assert!(!tiles.is_bar());
}

#[test]
fn tiles_start_at_the_top_with_the_entries() {
    let m = Metrics {
        width: 480,
        height: 800,
        line: 30.0,
        small: 20.0,
        icon: 32.0,
        gap: 8,
        margin: 16,
        border: 2,
        rtl: false,
    };
    let laid = layout(&[60.0, 60.0, 60.0], false, &m).unwrap();
    assert_eq!(laid.info.height, 0);
    // A gap above the two lines that are not there, a gap below them.
    assert!(laid.cells.iter().all(|c| c.y == 16));
    let cell = 32 + 20 + 2 * 8;
    assert_eq!(laid.region.y, 0);
    assert_eq!(laid.region.height, 16 + cell + 8 + 2);
    // Nothing above the entries runs an action.
    assert_eq!(laid.hit(Point::new(240, 4)), Hit::Inside);
    assert_eq!(laid.hit(Point::new(240, 700)), Hit::Outside);
}

/// Over the scripted host: the main menu opened with style Tiles.
#[cfg(feature = "linked")]
mod scripted {
    use paperpaw_core::native_host::environment::{SNAPSHOT_BYTES, SNAPSHOT_VERSION};
    use paperpaw_core::native_host::registrations::{
        EventScreenOpened, EventScreenShown, InputAction, Record, ServiceEnd, ServiceRegMenuRow,
        ServiceRegSetting, ServiceRegSettingExtra, ServiceUiHandle, ServiceUiParam,
        record_encode_one,
    };
    use paperpaw_plugin_menu::NATIVE_ENTRY;
    use paperpaw_scripted_host::{Plugin, ScriptedHost, event_batch};
    use std::cell::RefCell;
    use std::rc::Rc;

    const LIST: u32 = 10;
    const INSTANCE: u32 = 7;
    const ENABLED: u32 = 2;
    /// `reg_setting.flags`: has a value, shown.
    const SHOWN: u32 = (1 << 6) | (1 << 9);

    fn stream(records: &[Record<'_>]) -> Vec<u8> {
        let mut out = Vec::new();
        for record in records {
            let mut one = vec![0; 4096];
            let used = record_encode_one(record, &mut one).unwrap();
            out.extend_from_slice(&one[..used]);
        }
        out
    }

    /// A text as `settings_value` (layout 1, kind 3).
    fn text_value(text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1_u32.to_le_bytes());
        out.extend_from_slice(&3_u32.to_le_bytes());
        out.extend_from_slice(&u32::try_from(text.len()).unwrap().to_le_bytes());
        out.extend_from_slice(text.as_bytes());
        out
    }

    #[derive(Default)]
    struct Seen {
        handovers: Vec<Vec<u8>>,
    }

    fn contains(bytes: &[u8], needle: &str) -> bool {
        bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    }

    /// A device of 480 × 800 (as `bar_mode.rs`), with the plugin's texts
    /// and the core's (`core.on`), which the list needs.
    fn device() -> ScriptedHost {
        let host = ScriptedHost::for_plugin("menu");
        for folder in [
            concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../crates/paperpaw-core/i18n"
            ),
        ] {
            host.add_catalogs_in(folder).unwrap();
        }
        let mut bytes = vec![0_u8; SNAPSHOT_BYTES];
        bytes[0..4].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
        bytes[4..8].copy_from_slice(&u32::try_from(SNAPSHOT_BYTES).unwrap().to_le_bytes());
        bytes[32..34].copy_from_slice(&480_u16.to_le_bytes());
        bytes[34..36].copy_from_slice(&800_u16.to_le_bytes());
        bytes[36..38].copy_from_slice(&220_u16.to_le_bytes());
        bytes[42] = 1;
        host.set_env(&bytes);
        host
    }

    /// The main menu (the library, an action, and the tools, a submenu; the
    /// sleep entry's icon as `sleep_icon`), with `menu.style` as `style`
    /// (`None`: the registry cannot say).
    fn host(
        seen: &Rc<RefCell<Seen>>,
        style: Option<&'static str>,
        sleep_icon: &'static str,
    ) -> ScriptedHost {
        let host = device();
        let registry = host.registry();
        registry.on(12, move |_| {
            let row = |item, kind, icon| {
                Record::ServiceRegMenuRow(ServiceRegMenuRow {
                    item,
                    kind,
                    flags: ENABLED,
                    order: 0,
                    by: "core",
                    title: item,
                    icon,
                    badge: "",
                    describe: "",
                })
            };
            Ok(stream(&[
                row("core.library", 0, "library"),
                row("core.sleep", 0, sleep_icon),
                row("core.tools", 2, "tools"),
                Record::ServiceEnd(ServiceEnd {
                    flags: 1,
                    total: 3,
                    generation: 1,
                    next: 3,
                    cursor: None,
                }),
            ]))
        });
        if let Some(style) = style {
            registry.on(2, move |input| {
                if !contains(input, "menu.style") {
                    return Err(-1);
                }
                let value = text_value(style);
                Ok(stream(&[
                    Record::ServiceRegSetting(ServiceRegSetting {
                        key: "menu.style",
                        group: "core.look",
                        kind: 2,
                        flags: SHOWN,
                        source: 3,
                        title: "Menus",
                        value: &value,
                    }),
                    Record::ServiceRegSettingExtra(ServiceRegSettingExtra {
                        min: 0,
                        max: 0,
                        step: 0,
                        unit: "",
                        section: "",
                        follows: "",
                        shown_id: "",
                        shown_value: "",
                        editor_kind: 0,
                        editor: "",
                        choices: 2,
                    }),
                ]))
            });
        }
        let handovers = seen.clone();
        host.service("core.ui", 1, 0, move |op, input| match op {
            16 => {
                handovers.borrow_mut().handovers.push(input.to_vec());
                Ok(stream(&[Record::ServiceUiHandle(ServiceUiHandle {
                    handle: 9,
                })]))
            }
            17 => Ok(stream(&[
                Record::ServiceUiParam(ServiceUiParam {
                    key: "menu",
                    value: "core.main",
                }),
                Record::ServiceEnd(ServiceEnd {
                    flags: 0,
                    total: 1,
                    generation: 0,
                    next: 0,
                    cursor: None,
                }),
            ])),
            _ => Err(-1),
        });
        host
    }

    fn open(host: &ScriptedHost) -> Plugin<'_> {
        let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
        let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
            now_ms: 0,
            screen: "menu.list",
            instance: INSTANCE,
            slot: "core.menu",
            flags: 0,
            handover: 7,
            params: 0,
            text_bytes: 0,
        }))
        .unwrap();
        assert_eq!(plugin.event(LIST, &opened).unwrap().status, 0);
        let shown = event_batch(&Record::EventScreenShown(EventScreenShown {
            now_ms: 0,
            screen: "menu.list",
            width: 480,
            height: 800,
            instance: Some(INSTANCE),
            last_input: 0,
        }))
        .unwrap();
        assert_eq!(plugin.event(LIST, &shown).unwrap().status, 0);
        plugin
    }

    fn action(name: &str) -> Vec<u8> {
        event_batch(&Record::InputAction(InputAction {
            now_ms: 0,
            action: name,
        }))
        .unwrap()
    }

    /// Back from the first entry, then *Select*: the tiles go round to the
    /// tools (a submenu opened through a hand-over), a list stays on the
    /// library (an action, no hand-over).
    fn back_goes_round(style: Option<&'static str>, sleep_icon: &'static str) -> bool {
        let seen = Rc::new(RefCell::new(Seen::default()));
        let host = host(&seen, style, sleep_icon);
        let plugin = open(&host);
        plugin.input(LIST, &action("core.left")).unwrap();
        plugin.input(LIST, &action("core.confirm")).unwrap();
        let seen = seen.borrow();
        seen.handovers
            .last()
            .is_some_and(|h| contains(h, "core.tools"))
    }

    #[test]
    fn the_main_menu_opens_as_tiles_and_goes_round() {
        assert!(back_goes_round(Some("menu.style.tiles"), "sleep"));
    }

    #[test]
    fn a_submenu_of_the_tiles_opens_as_a_list_one_deep() {
        let seen = Rc::new(RefCell::new(Seen::default()));
        let host = host(&seen, Some("menu.style.tiles"), "sleep");
        let plugin = open(&host);
        plugin.input(LIST, &action("core.left")).unwrap();
        let chosen = plugin.input(LIST, &action("core.confirm")).unwrap();
        let seen = seen.borrow();
        let handover = seen.handovers.last().unwrap();
        assert!(contains(handover, "depth"), "{chosen:?}");
        assert!(chosen.batches_contain(b"menu.list"));
    }

    #[test]
    fn the_list_style_an_entry_without_an_icon_or_no_style_is_the_list() {
        assert!(!back_goes_round(Some("menu.style.list"), "sleep"));
        assert!(!back_goes_round(Some("menu.style.tiles"), ""));
        assert!(!back_goes_round(None, "sleep"));
    }
}
