//! The reader's bar (`menu.list` with `mode=bar`), as Beta 3 showed it: what
//! the reader hands over is read, `hide` leaves entries out, the title area
//! runs `action`, the buttons move the focus round the entries (mirrored right
//! to left) and choose, and the bar is laid out in even rows. The model is
//! tested alone; over the scripted host, the bar is opened with the reader's
//! hand-over and driven by buttons.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_plugin_menu::native_bar::{
    self as bar, Does, Handed, Hit, Key, Layout, Metrics, entry_width, layout,
};
use paperpaw_plugin_menu::native_model::{
    Act, Kind, MODE_BAR, Row, SETTING_SHOWN, SettingInfo, SettingKind, Value, flag,
};
use paperpaw_sdk::{Point, Rect};

/// What the reader hands over (`plugins/reader/src/native_view/menu_bar.rs`).
const READER: [(&str, &str); 7] = [
    ("menu", "core.reader-menu"),
    ("mode", "bar"),
    ("hide", "reader.search,reader.read-aloud"),
    ("title", "Moby-Dick"),
    ("text", "Chapter 1. Loomings."),
    ("page", "12 / 30"),
    ("action", "reader.book-info"),
];

fn row(item: &str, kind: Kind, flags: u32) -> Row {
    Row {
        item: item.to_string(),
        kind,
        flags,
        title: item.to_string(),
        icon: String::new(),
        badge: String::new(),
        describe: String::new(),
    }
}

fn action(item: &str) -> Row {
    row(item, Kind::Action, flag::ENABLED)
}

fn setting(key: &str, kind: SettingKind, value: Option<Value>, choices: u32) -> SettingInfo {
    SettingInfo {
        key: key.to_string(),
        kind,
        flags: SETTING_SHOWN,
        value,
        min: 0,
        max: 10,
        step: 1,
        unit: String::new(),
        editor_kind: 0,
        editor: String::new(),
        choices,
    }
}

fn metrics() -> Metrics {
    Metrics {
        width: 480,
        height: 800,
        line: 30.0,
        small: 20.0,
        icon: 32.0,
        gap: 8,
        margin: 16,
        border: 2,
        rtl: false,
    }
}

#[test]
fn the_readers_hand_over_is_read() {
    let mut handed = Handed::default();
    for (key, value) in READER {
        handed.take(key, value).unwrap();
    }
    handed.take("unknown", "left out").unwrap();
    assert!(handed.is_bar());
    assert_eq!(handed.menu, "core.reader-menu");
    assert_eq!(handed.mode, MODE_BAR);
    assert_eq!(handed.hide, "reader.search,reader.read-aloud");
    assert_eq!(handed.title, "Moby-Dick");
    assert_eq!(handed.text, "Chapter 1. Loomings.");
    assert_eq!(handed.page, "12 / 30");
    assert_eq!(handed.action, "reader.book-info");
    assert_eq!(handed.depth, None);
    // Without `mode=bar` the menu is a list.
    let mut list = Handed::default();
    list.take("menu", "core.main").unwrap();
    list.take("depth", "2").unwrap();
    assert!(!list.is_bar());
    assert_eq!(list.depth.as_deref(), Some("2"));
    // Texts are copied to their size, at most the 256 bytes a parameter holds.
    let long = "é".repeat(128);
    handed.take("title", &long).unwrap();
    assert_eq!(handed.title, long);
    assert_eq!(handed.title.capacity(), long.len());
}

#[test]
fn hide_headings_and_settings_not_shown_leave_entries_out() {
    let mut night = setting("core.night-mode", SettingKind::Bool, None, 0);
    night.flags = 0;
    let rows = vec![
        row("", Kind::Section, flag::ENABLED),
        action("reader.show-toc"),
        action("reader.search"),
        action("reader.read-aloud"),
        row("core.night-mode", Kind::Setting, flag::ENABLED),
        row("core.reader-more", Kind::Submenu, flag::ENABLED),
    ];
    let shown = bar::entries(rows, "reader.search, reader.read-aloud", &[night]).unwrap();
    let items: Vec<&str> = shown.iter().map(|r| r.item.as_str()).collect();
    assert_eq!(items, ["reader.show-toc", "core.reader-more"]);
}

#[test]
fn only_an_id_is_an_action_for_the_title_area() {
    assert!(bar::valid_action("reader.book-info").unwrap());
    assert!(!bar::valid_action("").unwrap());
    assert!(!bar::valid_action("Not an id").unwrap());
}

#[test]
fn a_tap_reaches_the_entries_then_the_title_area() {
    let laid = layout(&[60.0, 60.0, 60.0], true, &metrics()).unwrap();
    // The two lines: 30 + 20 + the gap, below a gap.
    assert_eq!(laid.info, Rect::new(0, 8, 480, 58));
    assert_eq!(laid.hit(Point::new(240, 20)), Hit::Info);
    let first = laid.cells[0];
    let middle = Point::new(i32::from(first.x) + 5, i32::from(first.y) + 5);
    assert_eq!(laid.hit(middle), Hit::Entry(0));
    // Halfway to the next entry still counts for this one.
    let beside = Point::new(i32::from(first.x + first.width) + 2, i32::from(first.y) + 5);
    assert_eq!(laid.hit(beside), Hit::Entry(0));
    // Below the bar: outside, which closes it.
    let below = Point::new(10, i32::from(laid.region.height) + 10);
    assert_eq!(laid.hit(below), Hit::Outside);
    // No lines: no title area.
    let bare = layout(&[60.0], false, &metrics()).unwrap();
    assert_eq!(bare.info.height, 0);
    assert_ne!(bare.hit(Point::new(240, 4)), Hit::Info);
}

#[test]
fn the_buttons_move_choose_and_close() {
    assert_eq!(bar::key("core.right", true, false), Key::Move(1));
    assert_eq!(bar::key("core.left", true, false), Key::Move(-1));
    // Right to left the entries start at the right: left goes on.
    assert_eq!(bar::key("core.left", true, true), Key::Move(1));
    assert_eq!(bar::key("core.right", true, true), Key::Move(-1));
    assert_eq!(bar::key("core.down", true, true), Key::Move(1));
    assert_eq!(bar::key("core.up", false, false), Key::Move(-1));
    assert_eq!(bar::key("core.next-page", true, false), Key::Move(1));
    // Turning pages does not show the focus on touch screens.
    assert_eq!(bar::key("core.next-page", false, false), Key::Swallow);
    assert_eq!(bar::key("core.prev-page", false, false), Key::Swallow);
    assert_eq!(bar::key("core.confirm", false, false), Key::Confirm);
    assert_eq!(bar::key("core.menu", true, false), Key::Close);
    assert_eq!(bar::key("core.back", true, false), Key::Close);
    assert_eq!(bar::key("core.power", true, false), Key::Other);
}

#[test]
fn the_focus_goes_round_past_faint_entries_and_stays_if_all_are() {
    let enabled = [true, false, true, true];
    let on = |i: usize| enabled.get(i).copied().unwrap_or(false);
    assert_eq!(bar::step_focus(4, 0, 1, on), 2);
    assert_eq!(bar::step_focus(4, 3, 1, on), 0);
    assert_eq!(bar::step_focus(4, 0, -1, on), 3);
    assert_eq!(bar::step_focus(4, 2, -1, on), 0);
    assert_eq!(bar::step_focus(3, 1, 1, |_| false), 1);
    assert_eq!(bar::step_focus(0, 0, 1, |_| true), 0);
}

#[test]
fn what_an_entry_does() {
    let toc = action("reader.show-toc");
    assert_eq!(
        bar::does(&toc, None).unwrap(),
        Does::Run("reader.show-toc".into())
    );
    let more = row("core.reader-more", Kind::Submenu, flag::ENABLED);
    assert_eq!(
        bar::does(&more, None).unwrap(),
        Does::OpenMenu("core.reader-more".into())
    );
    let faint = row("reader.search", Kind::Action, 0);
    assert_eq!(bar::does(&faint, None).unwrap(), Does::Nothing);
    let switch = row("core.night-mode", Kind::Setting, flag::ENABLED);
    let night = setting(
        "core.night-mode",
        SettingKind::Bool,
        Some(Value::Bool(true)),
        0,
    );
    assert_eq!(
        bar::does(&switch, Some(&night)).unwrap(),
        Does::Setting(Act::Step)
    );
    assert!(bar::on(&switch, Some(&night)));
    let number = setting("reader.margin", SettingKind::Int, Some(Value::Int(3)), 0);
    assert_eq!(
        bar::does(&switch, Some(&number)).unwrap(),
        Does::Setting(Act::Page)
    );
    let two = setting("reader.align", SettingKind::Choice, None, 2);
    assert_eq!(
        bar::does(&switch, Some(&two)).unwrap(),
        Does::Setting(Act::Step)
    );
    let three = setting("reader.font", SettingKind::Choice, None, 3);
    assert_eq!(
        bar::does(&switch, Some(&three)).unwrap(),
        Does::Setting(Act::Values)
    );
    // A setting the registry does not know runs as an action, as in Beta 3.
    assert_eq!(
        bar::does(&switch, None).unwrap(),
        Does::Run("core.night-mode".into())
    );
    // An entry its plugin shows as on.
    let checked = row(
        "reader.toggle-bookmark",
        Kind::Action,
        flag::ENABLED | flag::CHECKED,
    );
    assert!(bar::on(&checked, None));
    assert!(!bar::on(&toc, None));
}

#[test]
fn rows_are_filled_evenly_and_justified_between_the_margins() {
    // Four fit a row of 448 pixels (5 × 80 + 4 × 16 is too wide).
    let widths = [80.0; 7];
    let laid: Layout = layout(&widths, false, &metrics()).unwrap();
    let tops: Vec<u16> = laid.cells.iter().map(|c| c.y).collect();
    // Four and three rather than five and two.
    assert_eq!(tops.iter().filter(|y| **y == tops[0]).count(), 4);
    assert_eq!(tops.iter().filter(|y| **y != tops[0]).count(), 3);
    // The first starts at the margin, the last of a row ends at the other.
    assert_eq!(laid.cells[0].x, 16);
    assert!((463..=464).contains(&(laid.cells[3].x + laid.cells[3].width)));
    // Below a gap (and the two lines' place, empty), two rows of icon, label
    // and gaps, then the gap and the border.
    let cell = (32 + 20) + 2 * 8;
    assert_eq!(laid.region.height, 8 + 8 + 2 * cell + 8 + 2);
    // Right to left: the first entry at the right.
    let rtl = layout(
        &widths,
        false,
        &Metrics {
            rtl: true,
            ..metrics()
        },
    )
    .unwrap();
    assert_eq!(rtl.cells[0].x + rtl.cells[0].width, 464);
    assert!(rtl.rtl);
}

#[test]
fn an_entry_is_as_wide_as_its_label_or_icon_in_whole_pixels() {
    assert!((entry_width(40.2, 30.0) - 41.0).abs() < f32::EPSILON);
    assert!((entry_width(10.0, 30.0) - 30.0).abs() < f32::EPSILON);
    assert!(entry_width(f32::NAN, -5.0).abs() < f32::EPSILON);
}

#[test]
fn the_page_stays_whole_and_the_chapter_is_shortened() {
    // One pixel a byte; a fit keeps what fits and adds "…".
    let measure = |t: &str| Ok(t.len() as f32);
    let fit = |t: &str, w: f32| {
        if t.len() as f32 <= w {
            Ok(t.to_string())
        } else {
            let keep = (w as usize).saturating_sub(1);
            Ok(format!("{}…", &t[..keep]))
        }
    };
    let (chapter, page, wide) =
        bar::fit_pair(&"c".repeat(100), "12 / 30", (60.0, 10.0), fit, measure).unwrap();
    assert_eq!(page, "12 / 30");
    assert!((wide - 7.0).abs() < f32::EPSILON);
    assert!(chapter.ends_with('…'));
    assert!(chapter.chars().count() <= 50 - 7 + 1);
}

/// Over the scripted host: the reader's hand-over opens the bar on its menu.
#[cfg(feature = "linked")]
mod scripted {
    use super::READER;
    use paperpaw_core::native_host::environment::{SNAPSHOT_BYTES, SNAPSHOT_VERSION};
    use paperpaw_core::native_host::registrations::{
        EventScreenOpened, EventScreenShown, InputAction, Record, ServiceEnd, ServiceRegMenuRow,
        ServiceUiHandle, ServiceUiParam, record_encode_one,
    };
    use paperpaw_plugin_menu::NATIVE_ENTRY;
    use paperpaw_scripted_host::{Plugin, ScriptedHost, event_batch};
    use std::cell::RefCell;
    use std::rc::Rc;

    const LIST: u32 = 10;
    const INSTANCE: u32 = 7;
    const ENABLED: u32 = 2;

    fn stream(records: &[Record<'_>]) -> Vec<u8> {
        let mut out = Vec::new();
        for record in records {
            let mut one = vec![0; 4096];
            let used = record_encode_one(record, &mut one).unwrap();
            out.extend_from_slice(&one[..used]);
        }
        out
    }

    fn end(total: u32) -> Record<'static> {
        Record::ServiceEnd(ServiceEnd {
            flags: 1,
            total,
            generation: 1,
            next: total,
            cursor: None,
        })
    }

    /// What the test saw: the menus asked for, the hand-overs made.
    #[derive(Default)]
    struct Seen {
        menus: Vec<Vec<u8>>,
        handovers: Vec<Vec<u8>>,
    }

    fn contains(bytes: &[u8], needle: &str) -> bool {
        bytes
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    }

    /// The reader's menu: the table of contents, search (hidden by the
    /// hand-over) and More (a submenu).
    fn host(seen: &Rc<RefCell<Seen>>) -> ScriptedHost {
        let host = ScriptedHost::for_plugin("menu");
        let mut bytes = vec![0_u8; SNAPSHOT_BYTES];
        bytes[0..4].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
        bytes[4..8].copy_from_slice(&u32::try_from(SNAPSHOT_BYTES).unwrap().to_le_bytes());
        bytes[32..34].copy_from_slice(&480_u16.to_le_bytes());
        bytes[34..36].copy_from_slice(&800_u16.to_le_bytes());
        bytes[36..38].copy_from_slice(&220_u16.to_le_bytes());
        bytes[42] = 1;
        host.set_env(&bytes);
        let registry = host.registry();
        let menus = seen.clone();
        registry.on(12, move |input| {
            menus.borrow_mut().menus.push(input.to_vec());
            let row = |item, kind| {
                Record::ServiceRegMenuRow(ServiceRegMenuRow {
                    item,
                    kind,
                    flags: ENABLED,
                    order: 0,
                    by: "reader",
                    title: item,
                    icon: "",
                    badge: "",
                    describe: "",
                })
            };
            Ok(stream(&[
                row("reader.search", 0),
                row("reader.show-toc", 0),
                row("core.reader-more", 2),
                end(3),
            ]))
        });
        let handovers = seen.clone();
        host.service("core.ui", 1, 0, move |op, input| match op {
            16 => {
                handovers.borrow_mut().handovers.push(input.to_vec());
                Ok(stream(&[Record::ServiceUiHandle(ServiceUiHandle {
                    handle: 9,
                })]))
            }
            17 => {
                let mut out: Vec<Record<'_>> = READER
                    .iter()
                    .map(|(key, value)| Record::ServiceUiParam(ServiceUiParam { key, value }))
                    .collect();
                out.push(Record::ServiceEnd(ServiceEnd {
                    flags: 0,
                    total: u32::try_from(READER.len()).unwrap(),
                    generation: 0,
                    next: 0,
                    cursor: None,
                }));
                Ok(stream(&out))
            }
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

    #[test]
    fn the_bar_reads_the_menu_it_was_handed() {
        let seen = Rc::new(RefCell::new(Seen::default()));
        let host = host(&seen);
        let _plugin = open(&host);
        let seen = seen.borrow();
        assert!(!seen.menus.is_empty());
        assert!(contains(&seen.menus[0], "core.reader-menu"));
    }

    #[test]
    fn hidden_entries_are_passed_and_a_submenu_opens_over_the_bar() {
        let seen = Rc::new(RefCell::new(Seen::default()));
        let host = host(&seen);
        let plugin = open(&host);
        // Without a touch screen the focus shows at once: on the table of
        // contents (search is hidden); one on is More.
        let moved = plugin.input(LIST, &action("core.right")).unwrap();
        assert!(matches!(moved.status, 1 | -1), "{}", moved.status);
        let chosen = plugin.input(LIST, &action("core.confirm")).unwrap();
        // The list opens through a hand-over: its menu, one deep. (Opening a
        // screen is refused by this host, which has none.)
        let seen = seen.borrow();
        let handover = seen.handovers.last().unwrap();
        assert!(contains(handover, "core.reader-more"), "{chosen:?}");
        assert!(contains(handover, "depth"));
        assert!(chosen.batches_contain(b"menu.list"));
    }

    #[test]
    fn the_buttons_go_round_the_entries() {
        let seen = Rc::new(RefCell::new(Seen::default()));
        let host = host(&seen);
        let plugin = open(&host);
        // Back from the first entry is the last: More.
        plugin.input(LIST, &action("core.left")).unwrap();
        plugin.input(LIST, &action("core.confirm")).unwrap();
        assert!(contains(
            seen.borrow().handovers.last().unwrap(),
            "core.reader-more"
        ));
        // Round again to the table of contents: an action, no hand-over.
        let before = seen.borrow().handovers.len();
        plugin.input(LIST, &action("core.right")).unwrap();
        plugin.input(LIST, &action("core.confirm")).unwrap();
        assert_eq!(seen.borrow().handovers.len(), before);
    }

    #[test]
    fn keys_the_bar_does_not_know_are_left_to_the_host() {
        let seen = Rc::new(RefCell::new(Seen::default()));
        let host = host(&seen);
        let plugin = open(&host);
        assert_eq!(plugin.input(LIST, &action("core.power")).unwrap().status, 0);
    }
}
