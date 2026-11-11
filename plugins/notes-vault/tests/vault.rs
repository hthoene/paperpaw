//! The notes vault with the real kernel on a card in memory: its page
//! makes the index in the background, the lists show folders, the latest
//! changes, tags and search results, notes open in the reader and their
//! links lead on, and a vault of thousands of notes costs no more than a
//! window of rows.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError,
};
use std::rc::Rc;

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>, Rc<std::cell::Cell<usize>>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        // How often the index is read from its start.
        if offset == 0 && path.ends_with("/index.txt") {
            self.1.set(self.1.get() + 1);
        }
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

const X4: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const HOME: Id = Id::new_static("notes-vault.screen");
const LIST: Id = Id::new_static("notes-vault.list");

struct Vault {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

impl Vault {
    /// A card with `notes` (path, text), the vault folder set to `vault`.
    fn new(notes: &[(String, String)], vault: &str) -> Self {
        let storage = Shared::default();
        for (path, text) in notes {
            storage.write(path, text.as_bytes()).unwrap();
        }
        let config = format!("[core]\nlanguage = en\n[notes-vault]\nfolder = {vault}\n");
        let mut kernel = Kernel::builder(X4)
            .storage(storage.clone())
            .user_config(&config)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_epub_family::MarkdownFormat)
            .plugin(paperpaw_plugin_reader::Reader)
            .plugin(paperpaw_plugin_notes_vault::NotesVault)
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut vault = Self {
            kernel,
            storage,
            now: 0,
        };
        vault.settle(effects);
        vault
    }

    /// Lets timers run until nothing is due within a second.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..200_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
        panic!("the timers never settled");
    }

    fn open(&mut self, screen: Id, params: Params) {
        self.now += 100;
        let effects = self.kernel.open_screen(Instant(self.now), screen, params);
        self.settle(effects);
    }

    fn repeat(&mut self, action: Id, times: usize) {
        for _ in 0..times {
            self.run(action.clone());
        }
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn resize(&mut self, width: u16, height: u16) {
        self.now += 100;
        let effects = self.kernel.resize(Instant(self.now), width, height);
        self.settle(effects);
    }

    fn screen(&self) -> String {
        self.kernel
            .active_screen()
            .map(|id| id.as_str().to_string())
            .unwrap_or_default()
    }

    /// The labels of what the screen shows.
    fn labels(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }

    /// The titles of the rows shown (the first line of each).
    fn rows(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .filter(|p| p.role == Role::Row)
            .map(|p| p.label.split(", ").next().unwrap_or_default().to_string())
            .collect()
    }

    fn focused(&mut self) -> Option<String> {
        self.kernel.render(&mut |_, _| {});
        let parts = self.kernel.describe();
        paperpaw_sdk::semantics::focused(&parts).map(|p| p.label.clone())
    }

    fn says(&mut self, text: &str) -> bool {
        self.labels().iter().any(|l| l.contains(text))
    }

    /// Chooses the row that begins with `label`, by the buttons.
    fn choose(&mut self, label: &str) {
        for _ in 0..40 {
            if self.focused().is_some_and(|f| f.starts_with(label)) {
                self.run(actions::CONFIRM);
                return;
            }
            self.run(actions::DOWN);
        }
        panic!("no row {label}: {:?}", self.labels());
    }
}

/// Whether the list goes on past its window: its last row, reached by
/// whole pages, says *More notes*.
fn goes_on(vault: &mut Vault) -> bool {
    for _ in 0..40 {
        vault.run(actions::NEXT_PAGE);
    }
    vault.focused().is_some_and(|f| f.starts_with("More notes"))
}

fn note(path: &str, text: &str) -> (String, String) {
    (path.to_string(), text.to_string())
}

fn small_vault() -> Vec<(String, String)> {
    vec![
        note("vault/.obsidian/app.json", "{}"),
        note("vault/Inbox.md", "# Inbox\n\nSee [[Plan]] and #todo.\n"),
        note(
            "vault/Projects/Plan.md",
            "---\ntags: [work, plan]\n---\n# Plan\n\nThe plan, see [[Inbox]].\n",
        ),
        note("vault/Projects/Deep/Idea.md", "An idea. #work\n"),
        note("vault/Daily/2026-11-02.md", "Today. #todo #daily\n"),
        note("vault/Daily/readme.txt", "not a note"),
        note("vault/.trash/Gone.md", "hidden"),
    ]
}

#[test]
fn the_page_indexes_the_vault_in_the_background_and_says_how_many_notes() {
    let mut vault = Vault::new(&small_vault(), "vault");
    vault.open(HOME, Params::new());
    assert!(vault.says("4 notes"), "{:?}", vault.labels());
    // Kept in files on the card.
    assert!(
        vault
            .storage
            .exists(".paperpaw/plugins/notes-vault/index.txt")
    );
    let index = vault
        .storage
        .0
        .get(".paperpaw/plugins/notes-vault/index.txt")
        .unwrap();
    let index = String::from_utf8(index).unwrap();
    assert_eq!(index.lines().count(), 4, "{index}");
    assert!(
        !index.contains(".trash") && !index.contains("readme"),
        "{index}"
    );
    // The tags come after.
    let tags = vault
        .storage
        .0
        .get(".paperpaw/plugins/notes-vault/tags.txt")
        .map(|t| String::from_utf8(t).unwrap())
        .unwrap_or_default();
    assert!(tags.contains("Plan.md\twork plan"), "{tags}");
    assert!(tags.contains("Inbox.md\ttodo"), "{tags}");
}

#[test]
fn folders_tags_and_search_list_the_notes() {
    let mut vault = Vault::new(&small_vault(), "vault");
    vault.open(HOME, Params::new());
    // Folders: those first, then the notes, hidden ones left out.
    vault.choose("Folders");
    assert_eq!(vault.screen(), "notes-vault.list");
    assert_eq!(vault.rows(), ["Daily", "Projects", "Inbox"]);
    vault.run(actions::BACK);

    // Tags, with how many notes have each.
    vault.choose("Tags");
    assert_eq!(vault.screen(), "notes-vault.tags");
    let rows = vault.labels().join(" | ");
    assert!(rows.contains("#todo") && rows.contains("2 notes"), "{rows}");
    vault.choose("#work");
    assert_eq!(vault.screen(), "notes-vault.list");
    assert_eq!(vault.rows(), ["Idea", "Plan"]);
    vault.run(actions::BACK);
    vault.run(actions::BACK);

    // Search in the titles, by every word, whatever the case.
    vault.open(
        LIST,
        Params::new().with("mode", "search").with("text", "PLAN"),
    );
    assert_eq!(vault.rows(), ["Plan"]);
    vault.open(
        LIST,
        Params::new().with("mode", "search").with("text", "nothing"),
    );
    assert!(
        vault.says("No note has these words"),
        "{:?}",
        vault.labels()
    );
}

#[test]
fn a_note_opens_in_the_reader_and_its_links_lead_on_and_back() {
    let mut vault = Vault::new(&small_vault(), "vault");
    vault.open(HOME, Params::new());
    vault.choose("Folders");
    vault.choose("Inbox");
    assert_eq!(vault.screen(), "reader.screen");
    // The first link of the page: [[Plan]].
    vault.run(paperpaw_plugin_reader::actions::LINKS);
    vault.run(actions::CONFIRM);
    assert_eq!(vault.screen(), "reader.screen");
    // Back: the note it was opened from; back again: the vault's list.
    vault.run(actions::BACK);
    assert_eq!(vault.screen(), "reader.screen");
    vault.run(actions::BACK);
    assert_eq!(vault.screen(), "notes-vault.list");
}

#[test]
fn without_a_vault_the_page_asks_for_its_folder() {
    let mut vault = Vault::new(&small_vault(), "");
    vault.open(HOME, Params::new());
    assert!(
        vault.says("Choose the vault folder"),
        "{:?}",
        vault.labels()
    );
    // No index made for nothing.
    assert!(
        !vault
            .storage
            .exists(".paperpaw/plugins/notes-vault/index.txt")
    );
    // A vault that is not there: an empty index, no panic.
    let mut vault = Vault::new(&small_vault(), "nowhere");
    vault.open(HOME, Params::new());
    assert!(vault.says("0 notes"), "{:?}", vault.labels());
}

/// `count` notes in `folders` folders (`vault/F<n>/note-<k>.md`).
fn many(count: usize, folders: usize) -> Vec<(String, String)> {
    (0..count)
        .map(|k| {
            note(
                &format!("vault/F{:02}/note-{k:05}.md", k % folders),
                &format!("# Note {k}\n\n#t{} some text\n", k % 7),
            )
        })
        .collect()
}

/// The paths of the index made for `notes`.
fn indexed(notes: &[(String, String)]) -> Vec<String> {
    let mut vault = Vault::new(notes, "vault");
    vault.open(HOME, Params::new());
    let index = vault
        .storage
        .0
        .get(".paperpaw/plugins/notes-vault/index.txt")
        .unwrap();
    String::from_utf8(index)
        .unwrap()
        .lines()
        .filter_map(|l| l.split_once('\t').map(|(_, p)| p.to_string()))
        .collect()
}

#[test]
fn notes_in_nested_folders_are_all_indexed() {
    // 30 folders side by side, each with a folder holding a note: the
    // folders waiting must not count as depth.
    let notes: Vec<_> = (0..30)
        .map(|k| note(&format!("vault/T{k:02}/Sub/n{k}.md"), "# n\n"))
        .collect();
    assert_eq!(indexed(&notes).len(), 30);
    // Deep, but within the depth (the vault's folder and 22 below).
    let deep = (0..22)
        .map(|k| format!("d{k}"))
        .collect::<Vec<_>>()
        .join("/");
    let notes = [note(&format!("vault/{deep}/Deep.md"), "# d\n")];
    assert_eq!(indexed(&notes).len(), 1);
}

#[test]
fn a_folder_of_many_folders_is_indexed_whole() {
    // More folders than the walk once waited for (500): none left out.
    let notes: Vec<_> = (0..700)
        .map(|k| note(&format!("vault/G{k:03}/n{k}.md"), "# n\n"))
        .collect();
    assert_eq!(indexed(&notes).len(), 700);
}

#[test]
fn five_thousand_notes_are_listed_a_window_at_a_time() {
    let mut vault = Vault::new(&many(5000, 1), "vault");
    vault.open(HOME, Params::new());
    assert!(
        vault.says("5,000 notes") || vault.says("5000 notes"),
        "{:?}",
        vault.labels()
    );
    let index = vault
        .storage
        .0
        .get(".paperpaw/plugins/notes-vault/index.txt")
        .unwrap();
    assert_eq!(String::from_utf8(index).unwrap().lines().count(), 5000);
    // One folder of them all: windows of a hundred, with *More notes*.
    vault.choose("Folders");
    vault.choose("F00");
    let mut windows = 1;
    loop {
        // To the last row of the window by whole pages.
        for _ in 0..40 {
            if vault.focused().is_some_and(|f| f.starts_with("More notes")) {
                break;
            }
            vault.run(actions::NEXT_PAGE);
        }
        if !vault.focused().is_some_and(|f| f.starts_with("More notes")) {
            break;
        }
        vault.run(actions::CONFIRM);
        windows += 1;
        assert!(windows <= 60, "never ends");
    }
    assert_eq!(windows, 50);
    // The last window ends with the last note, and nothing more after it;
    // the first of its rows goes back to the window before.
    assert!(
        vault.rows().contains(&"note-04999".to_string()),
        "{:?}",
        vault.rows()
    );
    assert!(!vault.says("More notes"));
    for _ in 0..40 {
        vault.run(actions::PREV_PAGE);
    }
    assert!(vault.says("Earlier notes"), "{:?}", vault.labels());
}

#[test]
fn the_latest_changes_and_search_and_tags_work_on_a_big_vault() {
    let mut vault = Vault::new(&many(2000, 20), "vault");
    vault.open(HOME, Params::new());
    vault.open(
        LIST,
        Params::new()
            .with("mode", "search")
            .with("text", "note-0199"),
    );
    // 10 notes match 'note-0199x' (x = 0..9), in order of their titles.
    assert_eq!(vault.rows()[0], "note-01990", "{:?}", vault.rows());
    vault.open(LIST, Params::new().with("mode", "tag").with("text", "t3"));
    assert!(goes_on(&mut vault), "a window of a hundred, then more");
    vault.open(LIST, Params::new().with("mode", "recent"));
    assert!(goes_on(&mut vault));
}

#[test]
fn odd_vaults_are_read_without_a_panic() {
    // Deep folders, long and odd names, notes with nothing in them.
    let deep = (0..40)
        .map(|n| format!("d{n}"))
        .collect::<Vec<_>>()
        .join("/");
    let mut notes = vec![
        note(&format!("vault/{deep}/bottom.md"), "x"),
        note("vault/ünï cödé ☃.md", ""),
        note(&format!("vault/{}.md", "n".repeat(200)), "#a"),
        note("vault/.hidden.md", "x"),
        note("vault/UPPER.MD", "x"),
        note("vault/a b/c d.markdown", "x"),
    ];
    notes.extend(many(30, 3));
    let mut vault = Vault::new(&notes, "vault");
    vault.open(HOME, Params::new());
    assert!(!vault.says("Indexing"), "{:?}", vault.labels());
    for mode in ["folder", "recent", "tag", "search"] {
        vault.open(LIST, Params::new().with("mode", mode).with("text", "a"));
        vault.labels();
    }
    // Folders below the depth the walk goes to are left out; nothing fails.
    vault.open(HOME, Params::new());
    assert!(vault.says("notes"), "{:?}", vault.labels());
}

/// The vault with its widgets on the home screen's first page.
fn with_widgets(notes: &[(String, String)], extra: &str, page: &str) -> Vault {
    let storage = Shared::default();
    for (path, text) in notes {
        storage.write(path, text.as_bytes()).unwrap();
    }
    let config = format!(
        "[core]\nlanguage = en\n[notes-vault]\nfolder = vault\n{extra}\n\
         [home]\npage-1 = {page}\n"
    );
    let mut kernel = Kernel::builder(X4)
        .storage(storage.clone())
        .user_config(&config)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_epub_family::MarkdownFormat)
        .plugin(paperpaw_plugin_reader::Reader)
        .plugin(paperpaw_plugin_home::Home)
        .plugin(paperpaw_plugin_notes_vault::NotesVault)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut vault = Vault {
        kernel,
        storage,
        now: 0,
    };
    vault.settle(effects);
    vault
}

#[test]
fn the_widgets_say_what_is_missing_then_show_the_notes() {
    let page = "notes-vault.recent 0/0 4x2, notes-vault.tags 0/2 4x1, notes-vault.pinned 0/3 4x1, \
                notes-vault.day 0/4 4x1, notes-vault.random 0/5 4x1";
    let mut vault = with_widgets(&small_vault(), "pinned.title = plan\n", page);
    vault.open(Id::new_static("home.dashboard"), Params::new());
    // Not indexed yet: they say so, each a tap to the vault's page.
    assert!(
        vault.says("Open the vault to index it"),
        "{:?}",
        vault.labels()
    );
    // Indexed on the vault's page, then back to the home screen.
    vault.open(HOME, Params::new());
    vault.run(actions::BACK);
    vault.open(Id::new_static("home.dashboard"), Params::new());
    let shown = vault.labels().join(" | ");
    for expected in ["Inbox", "Plan", "#todo", "2 notes", "The plan, see Inbox."] {
        assert!(shown.contains(expected), "{expected} in {shown}");
    }
}

#[test]
fn the_widgets_do_not_read_the_index_again_while_it_is_the_same() {
    let page = "notes-vault.recent 0/0 4x2, notes-vault.tags 0/2 4x1, notes-vault.pinned 0/3 4x1";
    let mut vault = with_widgets(&small_vault(), "pinned.title = plan\n", page);
    vault.open(HOME, Params::new());
    vault.run(actions::BACK);
    vault.open(Id::new_static("home.dashboard"), Params::new());
    assert!(vault.says("Plan"), "{:?}", vault.labels());
    // The home screen shows again and again: the widgets' lines are as they
    // were, and the index (hundreds of KiB on a big vault) is not read
    // for them each time.
    let reads = vault.storage.1.get();
    for _ in 0..3 {
        vault.open(Id::new_static("home.add-widget"), Params::new());
        vault.run(actions::BACK);
        assert!(vault.says("Plan"), "{:?}", vault.labels());
    }
    assert_eq!(vault.storage.1.get(), reads, "read for the widgets");
}

#[test]
fn the_search_widget_asks_for_words_and_lists_the_notes() {
    let mut vault = with_widgets(&small_vault(), "", "notes-vault.search 0/0 4x1");
    vault.open(Id::new_static("home.dashboard"), Params::new());
    assert!(vault.says("Search…"), "{:?}", vault.labels());
}

#[test]
fn turned_the_pages_of_the_vault_keep_their_place() {
    let mut vault = Vault::new(&many(300, 3), "vault");
    vault.open(HOME, Params::new());
    vault.run(actions::DOWN);
    vault.run(actions::DOWN);
    let on_home = vault.focused();
    vault.resize(800, 480);
    assert_eq!(vault.screen(), "notes-vault.screen");
    assert_eq!(vault.focused(), on_home, "the vault's page");
    vault.choose("Folders");
    vault.choose("F01");
    vault.repeat(actions::NEXT_PAGE, 2);
    let on_list = vault.focused();
    assert!(on_list.is_some());
    vault.resize(480, 800);
    assert_eq!(vault.screen(), "notes-vault.list");
    assert_eq!(vault.focused(), on_list, "the list");
    vault.open(Id::new_static("notes-vault.tags"), Params::new());
    vault.run(actions::DOWN);
    let on_tags = vault.focused();
    vault.resize(800, 480);
    assert_eq!(vault.screen(), "notes-vault.tags");
    assert_eq!(vault.focused(), on_tags, "the tags");
}
