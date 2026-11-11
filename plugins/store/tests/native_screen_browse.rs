//! The start and the lists of the store, as plain functions (slice 3, host-free):
//! the rows made from the counts and the words, what choosing a row does, the
//! plugins a list picks from the index on the card and the rows of a page of
//! them, the marks, the letters, the sizes. The interface-6 screens
//! (`list.rs`, `view.rs`) are the references.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_index::{Index, Order, name_key, take};
use paperpaw_plugin_store::native_item::Device;
use paperpaw_plugin_store::native_list::Hooks;
use paperpaw_plugin_store::native_model::GROUPS;
use paperpaw_plugin_store::native_page::{Installed, Kind, Origin};
use paperpaw_plugin_store::native_screen_list::{
    Counts, Row, Say, choose, count, empty_words, group_name, items, retry, rows, searched,
    section_name,
};
use paperpaw_plugin_store::native_screen_view::{
    Act, Source, Top, Which, activation, first_from, hit_entry, mark, note, open_page, pick,
    size_text, tops,
};
use paperpaw_plugin_store::native_screens::{Ask, Installs, Next, Step, Trouble, ids};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::{Error, Fingerprint, INTERFACE};
use paperpaw_sdk::{MemoryStorage, Storage};
use paperpaw_ui_kit::Item;

/// The catalog's words as their keys, with the arguments after them.
struct Words;

impl Say for Words {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        // The decimal mark is the language's own, not a text to say.
        Ok(if key == "core.decimal" { "," } else { key }.to_string())
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        let mut text = key.to_string();
        for arg in args {
            match arg {
                Arg::Text { value, .. } => text.push_str(&format!(" {value}")),
                Arg::Int { value, .. } | Arg::Count(value) => {
                    text.push_str(&format!(" {value}"));
                }
                _ => {}
            }
        }
        Ok(text)
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        Ok(text.to_string())
    }
}

/// What the device has of some plugins.
#[derive(Default)]
struct Device1 {
    have: Vec<(String, Kind, bool, bool, String, Origin)>,
    steps: Vec<(String, Step)>,
    waiting: Vec<String>,
    actions: bool,
}

impl Installs for Device1 {
    fn step(&self, id: &str) -> Step {
        self.steps
            .iter()
            .find(|(i, _)| i == id)
            .map_or(Step::Idle, |(_, s)| s.clone())
    }
    fn queued(&self) -> bool {
        false
    }
    fn have(&self, id: &str) -> Option<Installed<'_>> {
        let (_, kind, enabled, restart, version, origin) =
            self.have.iter().find(|(i, ..)| i == id)?;
        Some(Installed {
            kind: *kind,
            enabled: *enabled,
            restart_needed: *restart,
            version,
            origin: *origin,
            release: None,
            trouble: None,
        })
    }
    fn takes_betas(&self) -> bool {
        false
    }
    fn has_action(&self, _: &str) -> bool {
        self.actions
    }
    fn waiting(&self, id: &str) -> bool {
        self.waiting.iter().any(|w| w == id)
    }
}

fn has(
    id: &str,
    kind: Kind,
    enabled: bool,
    restart: bool,
    version: &str,
    origin: Origin,
) -> (String, Kind, bool, bool, String, Origin) {
    (id.into(), kind, enabled, restart, version.into(), origin)
}

fn device() -> Device<'static> {
    Device {
        name: "x4",
        fingerprint: Some(Fingerprint::of(INTERFACE, 0x4567_89ab_cdef)),
        code_file: "code.ppn",
        have: None,
        sdk: paperpaw_sdk::SDK_VERSION,
        official: true,
        provides: None,
    }
}

fn nobody(_: &str) -> Option<[u8; 32]> {
    None
}
fn none() -> Vec<String> {
    Vec::new()
}

/// Twelve plugins: reading 0 to 5, look 6 to 8, tools 9 to 11.
fn list(count: usize, sections: &str) -> String {
    let groups = ["reading", "reading", "look", "tools"];
    let mut text = String::from(r#"{"format": 2, "plugins": ["#);
    for n in 0..count {
        if n > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            r#"{{"id": "plugin-{n:04}", "name": "Name {:04}", "summary": "Does {n} well.",
 "group": "{}", "kind": "data", "author": "A", "licence": "MIT",
 "added": "2020-{:02}-{:02}",
 "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30,
 "date": "2021-{:02}-{:02}"}}}}"#,
            (n * 7919) % 10_000,
            groups[(n / 3) % groups.len()],
            (n * 37) % 12 + 1,
            n % 28 + 1,
            (n * 101) % 12 + 1,
            n % 28 + 1,
        ));
    }
    text.push(']');
    text.push_str(sections);
    text.push('}');
    text
}

const SECTIONS: &str = r#", "sections": {"recommended": ["plugin-0003", "bad name", "plugin-0001"], "new": ["plugin-0002"]}"#;

fn kept(storage: &MemoryStorage, count: usize, sections: &str) -> Index {
    storage
        .write("list.json", list(count, sections).as_bytes())
        .unwrap();
    let hooks = Hooks {
        author_of: &nobody,
        stopped: &none,
    };
    take(storage, "list.json", device(), hooks)
        .unwrap()
        .expect("a list")
        .0
}

fn installed_device() -> Device1 {
    Device1 {
        have: vec![
            // Installed by the store, the same version.
            has(
                "plugin-0001",
                Kind::Data,
                true,
                false,
                "1.0.0",
                Origin::Store,
            ),
            // Installed by the store, older: an update waits.
            has(
                "plugin-0002",
                Kind::Data,
                true,
                false,
                "0.9.0",
                Origin::Store,
            ),
            // A built-in switched off is no plugin installed.
            has(
                "plugin-0007",
                Kind::BuiltIn,
                false,
                false,
                "1.0.0",
                Origin::Local,
            ),
            has(
                "plugin-0008",
                Kind::BuiltIn,
                true,
                false,
                "1.0.0",
                Origin::Local,
            ),
            // Put in place another way, older: no update is offered.
            has(
                "plugin-0009",
                Kind::Data,
                true,
                false,
                "0.1.0",
                Origin::Local,
            ),
        ],
        ..Device1::default()
    }
}

fn titles(items: &[Item]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|item| match item {
            Item::Row(row) => (row.title.clone(), row.value.clone()),
            _ => (String::new(), String::new()),
        })
        .collect()
}

#[test]
fn the_start_counts_groups_installed_updates_and_what_the_library_picked() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, SECTIONS);
    let counts = count(&index, &card, &installed_device()).unwrap();
    // Reading 6, look 3, tools 3.
    assert_eq!(counts.groups[0], 6);
    assert_eq!(counts.groups[5], 3);
    assert_eq!(counts.groups[9], 3);
    assert_eq!(counts.groups.iter().sum::<usize>(), 12);
    // 0001, 0002, 0008 and 0009 are installed, 0007 is off; one update.
    assert_eq!((counts.installed, counts.updates), (4, 1));
    // The two picked of `recommended` that this device lists, the one of `new`.
    assert_eq!(counts.sections, [2, 1, 0, 0, 0]);
}

#[test]
fn a_library_that_picks_nothing_leaves_the_sections_empty() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, "");
    let counts = count(&index, &card, &Device1::default()).unwrap();
    assert_eq!(counts.sections, [0; 5]);
    assert_eq!((counts.installed, counts.updates), (0, 0));
}

#[test]
fn the_rows_are_search_installed_what_was_picked_and_the_groups() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, SECTIONS);
    let counts = count(&index, &card, &installed_device()).unwrap();
    assert_eq!(
        rows(&counts, false).unwrap(),
        [
            Row::Search,
            Row::Installed,
            Row::Section(0),
            Row::Section(1),
            Row::Group(0),
            Row::Group(5),
            Row::Group(9)
        ]
    );
    // With nothing installed and nothing picked: the newest.
    let bare = Counts::default();
    assert_eq!(rows(&bare, false).unwrap(), [Row::Search, Row::New]);
}

#[test]
fn the_looks_are_listed_without_plugins_where_the_fonts_are_there() {
    let mut counts = Counts::default();
    counts.groups[0] = 2;
    assert_eq!(
        rows(&counts, true).unwrap(),
        [Row::Search, Row::New, Row::Group(0), Row::Group(5)]
    );
    assert_eq!(
        rows(&counts, false).unwrap(),
        [Row::Search, Row::New, Row::Group(0)]
    );
    // The row of the looks without plugins says `›`, not 0.
    let rows = rows(&counts, true).unwrap();
    let shown = titles(&items(&mut Words, &rows, &counts).unwrap());
    assert_eq!(shown[3], ("store.group.look".into(), "›".into()));
    assert_eq!(shown[2], ("store.group.reading".into(), "2".into()));
}

#[test]
fn each_row_says_its_name_and_its_number() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, SECTIONS);
    let counts = count(&index, &card, &installed_device()).unwrap();
    let list = rows(&counts, false).unwrap();
    let shown = titles(&items(&mut Words, &list, &counts).unwrap());
    // What has an update says how many, as the pill did.
    assert_eq!(
        shown,
        [
            ("store.search".into(), "›".into()),
            (
                "store.installed-list".into(),
                "store.updates-count 1".into()
            ),
            ("store.section.recommended".into(), "›".into()),
            ("store.section.new".into(), "›".into()),
            ("store.group.reading".into(), "6".into()),
            ("store.group.look".into(), "3".into()),
            ("store.group.tools".into(), "3".into()),
        ]
    );
    // Without an update, the number installed.
    let mut quiet = counts;
    quiet.updates = 0;
    let shown = titles(&items(&mut Words, &[Row::Installed], &quiet).unwrap());
    assert_eq!(shown[0].1, "4");
}

#[test]
fn a_group_past_the_known_ones_is_named_system() {
    assert_eq!(
        group_name(&mut Words, GROUPS.len()).unwrap(),
        "store.group.system"
    );
    assert_eq!(
        group_name(&mut Words, usize::MAX).unwrap(),
        "store.group.system"
    );
    assert_eq!(
        section_name(&mut Words, 4).unwrap(),
        "store.section.discover"
    );
    assert_eq!(
        section_name(&mut Words, 99).unwrap(),
        "store.section.recommended"
    );
}

#[test]
fn choosing_a_row_opens_its_list_or_asks_what_to_search() {
    let open = |row| choose(&mut Words, row).unwrap();
    let params = |view: &str| Next::Open {
        screen: ids::VIEW,
        params: vec![("view", view.to_string())],
    };
    assert_eq!(open(Row::Installed), params("installed"));
    assert_eq!(open(Row::New), params("new"));
    assert_eq!(open(Row::Section(3)), params("section:3"));
    assert_eq!(open(Row::Group(5)), params("group:5"));
    assert_eq!(
        open(Row::Search),
        Next::Ask {
            tag: 1,
            ask: Ask::Text {
                title: "store.search".into(),
                number: false
            }
        }
    );
    // The words typed open a list of what they find; none, nothing.
    assert_eq!(
        searched("  clock ").unwrap(),
        Next::Open {
            screen: ids::VIEW,
            params: vec![("view", "search".into()), ("text", "clock".into())]
        }
    );
    assert_eq!(searched("   ").unwrap(), Next::None);
}

#[test]
fn with_no_list_the_start_says_why_and_select_tries_again() {
    assert_eq!(
        empty_words(Trouble::Offline, true),
        ("store.offline", "store.offline.help")
    );
    assert_eq!(
        empty_words(Trouble::Offline, false),
        ("store.offline", "store.offline.help.buttons")
    );
    assert_eq!(
        empty_words(Trouble::Failed, false),
        ("store.unreachable", "store.unreachable.help.buttons")
    );
    assert_eq!(empty_words(Trouble::Loading, true), ("store.loading", ""));
    assert_eq!(empty_words(Trouble::None, true), ("store.loading", ""));
    assert!(retry(true, Trouble::Offline));
    assert!(retry(true, Trouble::Failed));
    assert!(!retry(true, Trouble::Loading));
    assert!(!retry(false, Trouble::Failed));
}

#[test]
fn the_lists_are_named_by_their_view_parameter() {
    let of = |view: &str, text: &str| {
        let params = [("view", view), ("text", text)];
        Which::of(&params).unwrap()
    };
    assert_eq!(of("group:5", ""), Which::Group(5));
    assert_eq!(of("installed", ""), Which::Installed);
    assert_eq!(of("new", ""), Which::New);
    assert_eq!(of("section:4", ""), Which::Section(4));
    assert_eq!(of("search", "clock"), Which::Search("clock".into()));
    // What is none of them is the first group.
    assert_eq!(of("", ""), Which::Group(0));
    assert_eq!(of("group:99", ""), Which::Group(0));
    assert_eq!(of("group:x", ""), Which::Group(0));
    assert_eq!(of("section:9", ""), Which::Section(0));
    assert_eq!(Which::of(&[]).unwrap(), Which::Group(0));
    // Each list opens back as the same list.
    for which in [
        Which::Group(12),
        Which::Installed,
        Which::New,
        Which::Section(2),
        Which::Search("a b".into()),
    ] {
        let params = which.params().unwrap();
        let borrowed: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(Which::of(&borrowed).unwrap(), which);
    }
}

#[test]
fn the_titles_are_the_list_names() {
    let title = |which: Which| which.title(&mut Words).unwrap();
    assert_eq!(title(Which::Group(5)), "store.group.look");
    assert_eq!(title(Which::Installed), "store.installed-list");
    assert_eq!(title(Which::New), "store.new");
    assert_eq!(title(Which::Section(1)), "store.section.new");
    assert_eq!(
        title(Which::Search("clock".into())),
        "store.search.title clock"
    );
}

#[test]
fn the_rows_above_the_plugins() {
    let top = |which: &Which, hits, per_page, fonts, updates| {
        tops(which, hits, per_page, fonts, updates).unwrap()
    };
    let look = Which::Group(5);
    assert_eq!(top(&look, 0, 6, false, false), []);
    assert_eq!(top(&look, 3, 6, false, false), [Top::Sort]);
    // More than a page: a letter is quicker than paging.
    assert_eq!(top(&look, 7, 6, false, false), [Top::Sort, Top::Letter]);
    assert_eq!(top(&look, 6, 6, false, false), [Top::Sort]);
    // The looks have the fonts above all.
    assert_eq!(top(&look, 3, 6, true, false), [Top::Fonts, Top::Sort]);
    assert_eq!(top(&look, 0, 6, true, false), [Top::Fonts]);
    // Other groups have no fonts; the first frame takes a page for six rows.
    assert_eq!(top(&Which::Group(0), 3, 6, true, false), [Top::Sort]);
    assert_eq!(top(&Which::Group(0), 7, 0, false, false).len(), 2);
    assert_eq!(top(&Which::Group(0), 6, 0, false, false).len(), 1);
    // Update all where one has an update, only in the installed list.
    assert_eq!(top(&Which::Installed, 4, 6, false, true), [Top::UpdateAll]);
    assert_eq!(top(&Which::Installed, 4, 6, false, false), []);
    for other in [Which::New, Which::Section(0), Which::Search("x".into())] {
        assert_eq!(top(&other, 40, 6, true, true), []);
    }
}

fn names(index: &Index, card: &MemoryStorage, hits: usize) -> Vec<String> {
    (0..hits)
        .map(|n| hit_entry(index, card, n).unwrap().unwrap().1.id)
        .collect()
}

#[test]
fn a_group_lists_its_plugins_from_a_to_z_or_newest_first() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, SECTIONS);
    let device = Device1::default();
    let look = Which::Group(5);
    // Names 3352 (8), 5433 (7), 7514 (6).
    let picked = pick(&look, Order::Name, &index, &card, &device).unwrap();
    assert_eq!(picked.hits, 3);
    assert_eq!(
        names(&index, &card, 3),
        ["plugin-0008", "plugin-0007", "plugin-0006"]
    );
    // Added: (n * 37) % 12 + 1 as the month: 6 -> 7, 7 -> 8, 8 -> 9.
    pick(&look, Order::Added, &index, &card, &device).unwrap();
    assert_eq!(
        names(&index, &card, 3),
        ["plugin-0008", "plugin-0007", "plugin-0006"]
    );
    // The library's order: the list's own.
    pick(&look, Order::Fair, &index, &card, &device).unwrap();
    assert_eq!(
        names(&index, &card, 3),
        ["plugin-0006", "plugin-0007", "plugin-0008"]
    );
}

#[test]
fn the_installed_list_has_those_with_an_update_first() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, SECTIONS);
    let picked = pick(
        &Which::Installed,
        Order::Rank,
        &index,
        &card,
        &installed_device(),
    )
    .unwrap();
    assert_eq!((picked.hits, picked.updates), (4, true));
    assert_eq!(
        names(&index, &card, 4),
        ["plugin-0002", "plugin-0001", "plugin-0008", "plugin-0009"]
    );
    let none = pick(
        &Which::Installed,
        Order::Rank,
        &index,
        &card,
        &Device1::default(),
    )
    .unwrap();
    assert_eq!((none.hits, none.updates), (0, false));
}

#[test]
fn a_section_keeps_the_libraries_order_and_the_newest_are_capped() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, SECTIONS);
    let device = Device1::default();
    let picked = pick(&Which::Section(0), Order::Name, &index, &card, &device).unwrap();
    assert_eq!(picked.hits, 2);
    assert_eq!(names(&index, &card, 2), ["plugin-0003", "plugin-0001"]);
    assert_eq!(
        pick(&Which::Section(2), Order::Name, &index, &card, &device)
            .unwrap()
            .hits,
        0
    );
    // The newest: thirty at most.
    let many = MemoryStorage::new();
    let big = kept(&many, 45, "");
    assert_eq!(
        pick(&Which::New, Order::Added, &big, &many, &device)
            .unwrap()
            .hits,
        30
    );
}

#[test]
fn a_search_finds_names_and_summaries_whatever_the_case() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, SECTIONS);
    let device = Device1::default();
    let found = |text: &str| {
        pick(
            &Which::Search(text.into()),
            Order::Fair,
            &index,
            &card,
            &device,
        )
        .unwrap()
        .hits
    };
    assert_eq!(found("NAME 3352"), 1);
    assert_eq!(found("does 1 well"), 1);
    assert_eq!(found("name"), 12);
    assert_eq!(found("nothing like it"), 0);
}

#[test]
fn the_rows_of_a_page_are_the_rows_above_and_the_plugins_in_place() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, SECTIONS);
    let device = Device1::default();
    let look = Which::Group(5);
    let picked = pick(&look, Order::Name, &index, &card, &device).unwrap();
    let top = tops(&look, picked.hits, 6, true, false).unwrap();
    let source = Source {
        index: &index,
        storage: &card,
        installs: &device,
        top: &top,
        hits: picked.hits,
        order: Order::Name,
    };
    // Fonts, the order, then three plugins.
    assert_eq!(source.len(), 5);
    let page = source.rows(&mut Words, 0, 10, "jump help").unwrap();
    let shown: Vec<(&str, &str, &str)> = page
        .iter()
        .map(|r| (r.title.as_str(), r.value.as_str(), r.note.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("store.fonts", "›", ""),
            ("store.sort", "store.order.name", "jump help"),
            ("Name 3352", "30 B", "Does 8 well."),
            ("Name 5433", "30 B", "Does 7 well."),
            ("Name 7514", "30 B", "Does 6 well."),
        ]
    );
    // A page of two, from the third row: only what is built is asked for.
    let page = source.rows(&mut Words, 2, 2, "").unwrap();
    assert_eq!(
        page.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
        ["Name 3352", "Name 5433"]
    );
    assert!(source.rows(&mut Words, 5, 2, "").unwrap().is_empty());
    assert!(
        source
            .rows(&mut Words, usize::MAX, 2, "")
            .unwrap()
            .is_empty()
    );
    assert!(source.rows(&mut Words, 3, usize::MAX, "").unwrap().len() == 2);
}

#[test]
fn a_row_chosen_is_one_above_the_plugins_or_a_plugin_by_its_place() {
    let top = [Top::Sort, Top::Letter];
    assert_eq!(activation(&top, 0), Act::Top(Top::Sort));
    assert_eq!(activation(&top, 1), Act::Top(Top::Letter));
    assert_eq!(activation(&top, 2), Act::Plugin(0));
    assert_eq!(activation(&top, 9), Act::Plugin(7));
    assert_eq!(activation(&[], 0), Act::Plugin(0));
    // A plugin's page is opened with its id and its place in the index.
    assert_eq!(
        open_page(7, "clock").unwrap(),
        Next::Open {
            screen: ids::PLUGIN,
            params: vec![("id", "clock".into()), ("entry", "7".into())]
        }
    );
}

#[test]
fn the_letters_are_found_by_halving() {
    let keys = [3u64, 5, 5, 9, 12, 12, 12, 40];
    for target in 0..45 {
        let by_halving = first_from(keys.len(), target, |n| Ok(keys[n])).unwrap();
        let by_counting = keys.iter().filter(|&&k| k < target).count();
        assert_eq!(by_halving, by_counting, "{target}");
    }
    assert_eq!(first_from(0, 7, |_| Err(Error::Io)).unwrap(), 0);
    assert_eq!(first_from(3, 7, |_| Err(Error::Io)), Err(Error::Io));
}

#[test]
fn a_letter_goes_to_the_first_name_at_or_after_it() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, SECTIONS);
    let device = Device1::default();
    let reading = Which::Group(0);
    let picked = pick(&reading, Order::Name, &index, &card, &device).unwrap();
    let key = |n: usize| name_key(&hit_entry(&index, &card, n).unwrap().unwrap().1.name);
    for letter in ["A", "M", "N", "O", "Z"] {
        let at = first_from(picked.hits, name_key(letter), |n| Ok(key(n))).unwrap();
        // All names are `Name ....`: before N none sorts before, after N all do.
        let want = usize::from(letter > "N") * picked.hits;
        assert_eq!(at, want, "{letter}");
    }
}

fn entry_of(
    card: &MemoryStorage,
    index: &Index,
    n: usize,
) -> paperpaw_plugin_store::native_model::Entry {
    let _ = card;
    index.entry(card, n).unwrap().unwrap()
}

#[test]
fn a_plugins_mark_says_where_it_stands() {
    let card = MemoryStorage::new();
    let index = kept(&card, 12, SECTIONS);
    let entry = |n| entry_of(&card, &index, n);
    let mut device = installed_device();
    let said = |device: &Device1, n| mark(&mut Words, device, &entry(n)).unwrap();
    assert_eq!(said(&device, 0), "30 B");
    assert_eq!(said(&device, 1), "store.state.installed");
    assert_eq!(said(&device, 2), "store.state.update");
    assert_eq!(said(&device, 7), "store.state.off");
    assert_eq!(said(&device, 8), "store.state.installed");
    // Put in place another way: installed, never an update.
    assert_eq!(said(&device, 9), "store.state.installed");
    // A restart waits.
    device.have.push(has(
        "plugin-0003",
        Kind::Code,
        true,
        true,
        "1.0.0",
        Origin::Store,
    ));
    assert_eq!(said(&device, 3), "store.state.restart");
    // A job beats all: the progress, then the reader's word.
    device.steps.push((
        "plugin-0000".into(),
        Step::Downloading { done: 3, total: 12 },
    ));
    device.steps.push((
        "plugin-0001".into(),
        Step::Downloading {
            done: 12,
            total: 12,
        },
    ));
    device.steps.push(("plugin-0002".into(), Step::Installing));
    device.steps.push(("plugin-0004".into(), Step::Installed));
    device.steps.push(("plugin-0005".into(), Step::Failed(13)));
    assert_eq!(said(&device, 0), "3 / 12");
    assert_eq!(said(&device, 1), "store.state.waiting");
    assert_eq!(said(&device, 2), "store.state.waiting");
    // A step that ended says nothing of its own: the plugin's state does.
    assert_eq!(said(&device, 4), "30 B");
    assert_eq!(said(&device, 5), "30 B");
    // The queue.
    device.waiting.push("plugin-0006".into());
    assert_eq!(said(&device, 6), "store.state.queued");
}

#[test]
fn a_plugin_not_built_for_this_paperpaw_says_so() {
    let mut entry = paperpaw_plugin_store::native_model::Entry {
        id: "x".into(),
        name: "X".into(),
        size: 2048,
        ..Default::default()
    };
    let device = Device1::default();
    assert_eq!(mark(&mut Words, &device, &entry).unwrap(), "2,0 KB");
    entry.needs_newer = true;
    assert_eq!(
        mark(&mut Words, &device, &entry).unwrap(),
        "store.needs-newer"
    );
    entry.waits_author = true;
    assert_eq!(mark(&mut Words, &device, &entry).unwrap(), "store.no-build");
    // Where it is installed the mark is the state it is in.
    let mut have = Device1::default();
    have.have
        .push(has("x", Kind::Data, true, false, "1.0.0", Origin::Store));
    assert_eq!(
        mark(&mut Words, &have, &entry).unwrap(),
        "store.state.installed"
    );
}

#[test]
fn what_a_plugin_does_comes_after_who_vouches_for_it() {
    let mut entry = paperpaw_plugin_store::native_model::Entry {
        summary: "Tells the time.".into(),
        ..Default::default()
    };
    assert_eq!(note(&mut Words, &entry).unwrap(), "Tells the time.");
    entry.unreviewed = true;
    assert_eq!(
        note(&mut Words, &entry).unwrap(),
        "store.unreviewed · Tells the time."
    );
    entry.official = true;
    assert_eq!(
        note(&mut Words, &entry).unwrap(),
        "store.official · Tells the time."
    );
}

#[test]
fn sizes_read_as_interface_6_wrote_them() {
    for bytes in [
        0u64,
        1,
        819,
        820,
        999,
        1000,
        1023,
        1024,
        1536,
        2457,
        10_239,
        10_240,
        14 * 1024,
        999_999,
        1_000_000,
        1_024_000,
        1_047_552,
        2_400_000,
        10 * 1024 * 1024,
        999 * 1024 * 1024,
        1_000 * 1024 * 1024,
        5 * 1024 * 1024 * 1024,
        u64::MAX / 4,
    ] {
        for mark in [".", ","] {
            assert_eq!(
                size_text(bytes, mark).unwrap(),
                paperpaw_sdk::files::size(bytes, mark),
                "{bytes} {mark}"
            );
        }
    }
}

proptest::proptest! {
    #[test]
    fn any_size_reads_as_interface_6_wrote_it(bytes in 0u64..(1 << 50)) {
        proptest::prop_assert_eq!(
            size_text(bytes, ",").unwrap(),
            paperpaw_sdk::files::size(bytes, ",")
        );
    }
}
