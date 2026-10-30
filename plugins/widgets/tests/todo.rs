//! The to-do list's own screen on the real kernel: items added, ticked
//! off and on, edited, moved up and removed are written to `todo.txt` as
//! they change, any sequence of them (property-based) leaves the file
//! as a list kept by hand would be, and the list is read back the same
//! by a screen opened anew, and by the widget on the home screen.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_widgets::Widgets;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError, SystemStatus, WallClock,
};
use proptest::prelude::*;

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Small,
);

const TODO: &str = ".paperpaw/plugins/widgets/todo.txt";
const SCREEN: Id = Id::new_static("widgets.todo.list");
/// Wednesday, 14 October 2026, 10:41 UTC.
const NOW: i64 = 1_791_974_460_000;
const TODAY: &str = "2026-10-14";

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

/// The kernel with the widget pack, on `storage`, its clock at [`NOW`].
struct List {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

impl List {
    fn new(storage: Shared) -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(Widgets)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.set_status(
            Instant(10),
            SystemStatus::new(Some(WallClock::new(Instant(0), NOW, Some(0))), None),
        );
        Self {
            kernel,
            storage,
            now: 100,
        }
    }

    fn tick(&mut self) -> Instant {
        self.now += 100;
        Instant(self.now)
    }

    /// The list's own screen opened.
    fn open(&mut self) {
        let now = self.tick();
        self.kernel.open_screen(now, SCREEN, Params::new());
        assert_eq!(self.kernel.active_screen(), Some(&SCREEN));
    }

    fn send(&mut self, event: &str, params: Params) {
        let now = self.tick();
        self.kernel
            .send_event(now, Id::parse(event).unwrap(), params);
    }

    /// An item typed on the screen, as the text input answers.
    fn add(&mut self, text: &str) {
        let typed = Typed::new(text.to_string(), Params::new());
        self.send("widgets.todo.added", typed.to_params());
    }

    /// Item `index`'s text typed anew.
    fn edit(&mut self, index: usize, text: &str) {
        let typed = Typed::new(
            text.to_string(),
            Params::new().with("index", index.to_string()),
        );
        self.send("widgets.todo.edited", typed.to_params());
    }

    /// `option` chosen from item `index`'s menu: tick, untick, up, remove.
    fn choose(&mut self, index: usize, option: &str) {
        let picked = Picked::new(index.to_string(), option.to_string());
        self.send("widgets.todo.chosen", picked.to_params());
    }

    /// The file as kept.
    fn file(&self) -> String {
        String::from_utf8(paperpaw_sdk::read_file(&self.storage, TODO, 64 * 1024).unwrap()).unwrap()
    }

    /// The texts the screen shows, in order.
    fn shown(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }
}

/// An item of the list as the test expects it.
#[derive(Debug, Clone, PartialEq)]
struct Item {
    text: String,
    done: bool,
}

/// The file the items should be kept as.
fn kept(items: &[Item]) -> String {
    items
        .iter()
        .map(|i| {
            if i.done {
                format!("x {TODAY} {}\n", i.text)
            } else {
                format!("{}\n", i.text)
            }
        })
        .collect()
}

/// A text as typed on the screen becomes an item's: one line, trimmed,
/// 200 characters at most.
fn cleaned(text: &str) -> String {
    let text = text.replace(['\n', '\r', '\t'], " ");
    text.trim().chars().take(200).collect()
}

#[derive(Debug, Clone)]
enum Op {
    Add(String),
    Tick(usize),
    Edit(usize, String),
    Up(usize),
    Remove(usize),
}

/// Texts typed: words, now and then a line break or tab inside, spaces
/// round them, nothing but spaces, or very long; never beginning with
/// "x " (kept with a date in front, which the unit tests check).
fn typed() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => "[A-Za-z0-9äöüß][A-Za-z0-9äöüß ,.!?]{0,30}",
        1 => "  [A-Za-z]{1,8}\n[A-Za-z]{1,8}\t[a-z]{0,5}  ",
        1 => "[ \t]{0,3}",
        1 => "[A-Za-z]{250,300}",
    ]
    .prop_filter("not read as done", |t| !cleaned(t).starts_with("x "))
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => typed().prop_map(Op::Add),
        2 => (0usize..8).prop_map(Op::Tick),
        1 => (0usize..8, typed()).prop_map(|(i, t)| Op::Edit(i, t)),
        1 => (0usize..8).prop_map(Op::Up),
        1 => (0usize..8).prop_map(Op::Remove),
    ]
}

/// What `op` does to `items`, as the list's screen should.
fn apply(items: &mut Vec<Item>, op: &Op) {
    match op {
        Op::Add(text) => {
            let text = cleaned(text);
            if !text.is_empty() && items.len() < 100 {
                items.push(Item { text, done: false });
            }
        }
        Op::Tick(i) => {
            if let Some(item) = items.get_mut(*i) {
                item.done = !item.done;
            }
        }
        Op::Edit(i, text) => {
            let text = cleaned(text);
            if let Some(item) = items.get_mut(*i)
                && !text.is_empty()
            {
                item.text = text;
            }
        }
        Op::Up(i) => {
            if *i > 0 && *i < items.len() {
                items.swap(*i, *i - 1);
            }
        }
        Op::Remove(i) => {
            if *i < items.len() {
                items.remove(*i);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48)))]

    /// Any sequence of changes on the list's screen leaves the file as
    /// the list should be, and a screen opened anew reads it back.
    #[test]
    fn changes_on_the_screen_are_kept_and_read_back(ops in proptest::collection::vec(op(), 1..25)) {
        let storage = Shared::default();
        storage.write(TODO, b"First thing\n").unwrap();
        let mut list = List::new(storage.clone());
        list.open();
        let mut items = vec![Item { text: "First thing".into(), done: false }];
        for op in &ops {
            match op {
                Op::Add(text) => list.add(text),
                Op::Tick(i) => {
                    let done = items.get(*i).is_some_and(|item| item.done);
                    list.choose(*i, if done { "untick" } else { "tick" });
                }
                Op::Edit(i, text) => list.edit(*i, text),
                Op::Up(i) => list.choose(*i, "up"),
                Op::Remove(i) => list.choose(*i, "remove"),
            }
            apply(&mut items, op);
            prop_assert_eq!(list.file(), kept(&items), "after {:?}", op);
        }
        // Read back by the screen opened again on another kernel.
        let mut again = List::new(storage);
        again.open();
        // The rows below "Add", as many as fit, each an item's text (or
        // its start, shortened with "…"), in order.
        let shown = again.shown();
        let add = shown.iter().position(|l| l == "Add something to do").unwrap();
        let rows = &shown[add + 1..];
        if !items.is_empty() {
            prop_assert!(!rows.is_empty(), "{:?}", shown);
        }
        for (row, item) in rows.iter().zip(&items) {
            let start = row.trim_end_matches('…');
            prop_assert!(
                !start.is_empty() && item.text.starts_with(start),
                "{:?} shown as {:?}", item.text, row
            );
        }
        // Ticked off once more: the file the same again.
        if !items.is_empty() {
            let done = items[0].done;
            again.choose(0, if done { "untick" } else { "tick" });
            items[0].done = !done;
            prop_assert_eq!(again.file(), kept(&items));
        }
    }
}

#[test]
fn choices_for_items_that_are_not_there_change_nothing() {
    let storage = Shared::default();
    storage.write(TODO, b"One\nx 2026-10-14 Two\n").unwrap();
    let mut list = List::new(storage);
    list.open();
    for (index, option) in [(2, "tick"), (99, "remove"), (0, "up"), (0, "dance")] {
        list.choose(index, option);
    }
    // Not a number at all.
    let picked = Picked::new("-1".into(), "remove".into());
    list.send("widgets.todo.chosen", picked.to_params());
    list.edit(7, "Seven");
    list.add("   ");
    assert_eq!(list.file(), "One\nx 2026-10-14 Two\n");
}

#[test]
fn a_hundred_items_at_most_are_kept() {
    let storage = Shared::default();
    let full: String = (0..100).map(|n| format!("Item {n}\n")).collect();
    storage.write(TODO, full.as_bytes()).unwrap();
    let mut list = List::new(storage);
    list.open();
    list.add("One too many");
    assert_eq!(list.file(), full);
    // One removed, one more fits.
    list.choose(0, "remove");
    list.add("Now it fits");
    let file = list.file();
    assert!(file.starts_with("Item 1\n"), "{file}");
    assert!(file.ends_with("Item 99\nNow it fits\n"), "{file}");
}

#[test]
fn the_home_widget_shows_what_the_screen_kept() {
    let storage = Shared::default();
    let mut list = List::new(storage.clone());
    list.open();
    list.add("Return the library book");
    list.add("Water the plants");
    list.choose(1, "tick");
    // On the home screen, the widget reads the same list: one open of two.
    let mut home = List::new(storage);
    let now = home.tick();
    home.kernel
        .set_setting(now, "home.page-1", "widgets.todo 0/0 4x3")
        .unwrap();
    let shown = home.shown();
    let has = |text: &str| shown.iter().any(|l| l.contains(text));
    assert!(has("Return the library"), "{shown:?}");
    assert!(has("To do · 1"), "{shown:?}");
}
