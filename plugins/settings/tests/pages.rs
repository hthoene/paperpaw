//! The settings screens with the real kernel and the standard app, on a
//! buttons-only X4 and a touch-only device: searching by words of the
//! language shown (English and German, accents or not), a hit opening its
//! page at the setting; a choice's list, the value chosen kept in
//! `paperpaw.ini` and read by a new kernel; text typed for a setting;
//! advanced settings; resetting a page and everything.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const LIST: &str = "settings.list";
const GROUP: &str = "settings.group";
const RESULTS: &str = "settings.search";
const CHOOSER: &str = "settings.choose";

fn touch(app: &App) -> bool {
    app.kernel.device().input.touch
}

/// The rows shown, whole labels.
fn rows(app: &mut App) -> Vec<String> {
    app.parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.label)
        .collect()
}

/// The first part of each row's label.
fn titles(app: &mut App) -> Vec<String> {
    rows(app)
        .into_iter()
        .map(|r| r.split(", ").next().unwrap_or_default().to_string())
        .collect()
}

/// Whether a row shown as `shown` is the one called `wanted`: the same,
/// or shortened to fit (ending in "…"), or its first part.
fn same(shown: &str, wanted: &str) -> bool {
    let first = shown.split(", ").next().unwrap_or_default();
    shown == wanted
        || first == wanted
        || shown.starts_with(&format!("{wanted}, "))
        || first
            .strip_suffix('…')
            .is_some_and(|short| wanted.starts_with(short))
}

/// Chooses the row called `label`: down to it (the first press may only
/// show the focus) and confirm, or a tap (on a later page if needed).
fn choose(app: &mut App, label: &str) {
    if touch(app) {
        for _ in 0..10 {
            // The last one drawn: a question's choices lie over the rows
            // of the page below, which are described as well.
            let part = app
                .parts()
                .into_iter()
                .rev()
                .find(|p| p.role == Role::Row && same(&p.label, label));
            if let Some(part) = part {
                app.tap(center(part.area));
                return;
            }
            app.tap_next_page();
        }
    } else {
        for _ in 0..200 {
            if app.focused().is_some_and(|f| same(&f, label)) {
                app.run(actions::CONFIRM);
                return;
            }
            app.run(actions::DOWN);
        }
    }
    panic!("no row {label}:\n{}", app.described());
}

fn settings(device: DeviceInfo, config: &str) -> App {
    let mut app = App::new(device, Shared::memory(config, &[]));
    app.run(actions::SETTINGS);
    assert_eq!(app.screen(), LIST);
    app
}

/// Searches the settings for `words`, as typed on the keyboard: the
/// first row of the main page, in either language.
fn search(app: &mut App, words: &str) {
    app.will_type(words);
    if !titles(app).iter().any(|t| t == "Search" || t == "Suchen") {
        // On another page: the main page anew, from its top.
        app.run(actions::BACK);
        app.run(actions::SETTINGS);
    }
    let title = titles(app)
        .into_iter()
        .find(|t| t == "Search" || t == "Suchen")
        .unwrap_or_else(|| panic!("no search:\n{}", app.described()));
    choose(app, &title);
}

#[test]
fn search_finds_settings_by_words_in_english() {
    for device in DEVICES {
        let mut app = settings(device, "");
        search(&mut app, "  SIZE   font ");
        assert_eq!(app.screen(), RESULTS, "{}", app.described());
        let hits = titles(&mut app);
        assert!(hits.iter().any(|h| h == "Font size"), "{hits:?}");
        // Every word counts: "size" alone finds more.
        app.run(actions::BACK);
        search(&mut app, "size");
        let more = titles(&mut app);
        assert!(more.len() > hits.len(), "{more:?}");
    }
}

#[test]
fn search_finds_settings_by_words_in_german_with_or_without_accents() {
    for device in DEVICES {
        let mut app = settings(device, "[core]\nlanguage = de\n");
        search(&mut app, "schriftgröße");
        assert_eq!(app.screen(), RESULTS, "{}", app.described());
        let hits = titles(&mut app);
        assert!(hits.iter().any(|h| h == "Schriftgröße"), "{hits:?}");
        app.run(actions::BACK);
        search(&mut app, "SCHRIFTGROSSE");
        let folded = titles(&mut app);
        assert!(folded.iter().any(|h| h == "Schriftgröße"), "{folded:?}");
    }
}

#[test]
fn a_hit_opens_its_page_with_the_setting_focused() {
    let mut app = settings(X4, "");
    search(&mut app, "font size");
    choose(&mut app, "Font size");
    assert_eq!(app.screen(), GROUP);
    assert!(
        app.focused().unwrap().starts_with("Font size"),
        "{}",
        app.described()
    );
    // Back to the results, then the main page.
    app.run(actions::BACK);
    assert_eq!(app.screen(), RESULTS);
    app.run(actions::BACK);
    assert_eq!(app.screen(), LIST);

    let mut app = settings(TOUCH, "");
    search(&mut app, "font size");
    choose(&mut app, "Font size");
    assert_eq!(app.screen(), GROUP);
    assert!(app.shows("Font size"), "{}", app.described());
}

#[test]
fn search_finds_the_pages_of_other_plugins_and_opens_them() {
    for device in DEVICES {
        // A plugin's own page, as any plugin registers it.
        let mut app = with_account(device, "");
        search(&mut app, "password");
        let hits = titles(&mut app);
        assert!(hits.iter().any(|h| h == "Password"), "{hits:?}");
        choose(&mut app, "Password");
        assert_eq!(app.screen(), GROUP);
        assert!(app.shows("Server"), "{}", app.described());
    }
}

#[test]
fn a_search_without_hits_says_so_and_an_empty_one_opens_nothing() {
    for device in DEVICES {
        let mut app = settings(device, "");
        search(&mut app, "zzqx flibbertigibbet");
        assert_eq!(app.screen(), RESULTS);
        assert!(
            app.shows("No setting has these words."),
            "{}",
            app.described()
        );
        assert!(rows(&mut app).is_empty());
        app.run(actions::CONFIRM);
        assert_eq!(app.screen(), RESULTS);
        app.run(actions::BACK);
        search(&mut app, "   ");
        assert_eq!(app.screen(), LIST);
    }
}

#[test]
fn advanced_settings_are_found_only_once_shown() {
    for device in DEVICES {
        let mut app = settings(device, "");
        // The address of Calibre (or of Joplin) is an advanced setting.
        search(&mut app, "address");
        let before = titles(&mut app);
        app.run(actions::BACK);
        choose(&mut app, "Advanced settings");
        assert!(app.kernel.settings().bool("settings.advanced"));
        search(&mut app, "address");
        let after = titles(&mut app);
        assert!(after.len() > before.len(), "{before:?} {after:?}");
    }
}

#[test]
fn a_choice_of_more_than_two_opens_its_list_and_the_value_is_kept() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = App::new(device, storage.clone());
        app.run(actions::SETTINGS);
        search(&mut app, "week starts");
        let hit = titles(&mut app).remove(0);
        choose(&mut app, &hit);
        assert_eq!(app.screen(), GROUP);
        if device.input.touch {
            choose(&mut app, &hit);
        } else {
            app.run(actions::CONFIRM);
        }
        assert_eq!(app.screen(), CHOOSER, "{}", app.described());
        let options = titles(&mut app);
        assert_eq!(options, ["As the language has it", "Monday", "Sunday"]);
        // With buttons, the focus on the value set.
        if !device.input.touch {
            assert_eq!(app.focused().as_deref(), Some("As the language has it"));
        }
        choose(&mut app, "Sunday");
        assert_eq!(app.screen(), GROUP);
        assert_eq!(
            app.kernel.settings().text("core.week-start"),
            "core.week.sunday"
        );
        assert!(
            storage.config().contains("week-start = core.week.sunday"),
            "{}",
            storage.config()
        );
        let app = app.restart();
        assert_eq!(
            app.kernel.settings().text("core.week-start"),
            "core.week.sunday"
        );
    }
}

#[test]
fn the_language_chosen_shows_at_once_and_after_a_restart() {
    for device in DEVICES {
        let storage = Shared::memory("", &[]);
        let mut app = App::new(device, storage.clone());
        app.open_screen(
            Id::new_static("settings.choose"),
            Params::new().with("id", "core.language"),
        );
        assert_eq!(app.screen(), CHOOSER);
        choose(&mut app, "Deutsch");
        assert_eq!(app.kernel.settings().text("core.language"), "de");
        app.run(actions::SETTINGS);
        assert!(app.shows("Einstellungen"), "{}", app.described());
        let mut app = app.restart();
        app.run(actions::SETTINGS);
        assert!(app.shows("Einstellungen"), "{}", app.described());
    }
}

#[test]
fn a_choice_opened_for_no_setting_shows_nothing_to_choose() {
    for id in [
        "",
        "no.such-setting",
        "core.font-size",
        "..",
        "core.language.x",
    ] {
        for device in DEVICES {
            let storage = Shared::memory("", &[]);
            let mut app = App::new(device, storage.clone());
            let before = storage.config();
            app.open_screen(
                Id::new_static("settings.choose"),
                Params::new().with("id", id),
            );
            assert_eq!(app.screen(), CHOOSER, "{id:?}");
            assert!(rows(&mut app).is_empty(), "{id:?}");
            app.run(actions::CONFIRM);
            app.tap(Point::new(240, 300));
            assert_eq!(storage.config(), before, "{id:?}");
        }
    }
}

/// Typed on the keyboard the settings page opens for a text setting
/// (Calibre's address: the progress sync's server, checked here before, is
/// set on its account screen now, as another server signs out).
#[test]
fn text_typed_for_a_setting_is_kept_trimmed() {
    for device in DEVICES {
        let storage = Shared::memory(
            "[settings]
advanced = on
[calibre]
address = 192.168.1.5
",
            &[],
        );
        let mut app = App::new(device, storage.clone());
        app.run(actions::SETTINGS);
        search(&mut app, "calibre address");
        let hit = rows(&mut app)
            .into_iter()
            .find(|r| r.starts_with("Address"))
            .unwrap_or_else(|| panic!("{}", app.described()));
        let hit = hit.split(", ").next().unwrap().to_string();
        choose(&mut app, &hit);
        assert_eq!(app.screen(), GROUP);
        app.will_type("  192.168.1.20:9090  ");
        if device.input.touch {
            choose(&mut app, &hit);
        } else {
            app.run(actions::CONFIRM);
        }
        assert_eq!(app.screen(), GROUP, "{}", app.described());
        // The keyboard started from the value it had.
        let question = app.asked().pop().unwrap();
        assert!(
            question
                .get(params::TEXT)
                .is_some_and(|t| t == "192.168.1.5")
        );
        assert_eq!(
            app.kernel.settings().text("calibre.address"),
            "192.168.1.20:9090"
        );
        assert!(storage.config().contains("address = 192.168.1.20:9090"));
    }
}

#[test]
fn a_page_reset_asks_first_and_sets_its_settings_back() {
    for device in DEVICES {
        let storage = Shared::memory("[core]\nfont-size = 20\n", &[]);
        let mut app = App::new(device, storage.clone());
        app.run(actions::SETTINGS);
        search(&mut app, "font size");
        choose(&mut app, "Font size");
        choose(&mut app, "Reset to defaults");
        assert!(app.shows("and passwords stay."), "{}", app.described());
        choose(&mut app, "Cancel");
        assert!(app.kernel.settings().is_set("core.font-size"));
        choose(&mut app, "Reset to defaults");
        choose(&mut app, "Yes, reset");
        assert!(!app.kernel.settings().is_set("core.font-size"));
        assert!(
            !storage.config().contains("font-size = 20"),
            "{}",
            storage.config()
        );
    }
}

#[test]
fn everything_reset_keeps_the_language() {
    for device in DEVICES {
        let config = "[core]\nlanguage = de\nfont-size = 20\n[files]\nsort = files.sort.name\n";
        let storage = Shared::memory(config, &[]);
        let mut app = App::new(device, storage.clone());
        app.run(actions::SETTINGS);
        choose(&mut app, "Alles zurücksetzen");
        assert!(app.shows("bleiben."), "{}", app.described());
        choose(&mut app, "Ja, zurücksetzen");
        let settings = app.kernel.settings();
        assert!(!settings.is_set("core.font-size"));
        assert!(!settings.is_set("files.sort"));
        assert_eq!(settings.text("core.language"), "de");
    }
}

/// A plugin with an account: a server typed in (kept on a reset), a
/// password (a secret) and a number (reset), on a page of its own.
struct Account;

const ACCOUNT_PAGE: Id = Id::new_static("acct.page");
static ACCOUNT_SECRET: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().secret();
static ACCOUNT_SETTINGS: [paperpaw_sdk::SettingSpec; 3] = [
    paperpaw_sdk::SettingSpec::text("acct.server", "Server", "", ACCOUNT_PAGE).kept_on_reset(),
    paperpaw_sdk::SettingSpec::text("acct.password", "Password", "", ACCOUNT_PAGE)
        .with_extra(&ACCOUNT_SECRET),
    paperpaw_sdk::SettingSpec::number("acct.count", "Count", 1, 50, 10, ACCOUNT_PAGE),
];

impl paperpaw_sdk::Plugin for Account {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("acct"),
            "Account",
            paperpaw_sdk::Version::new(0, 1, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        r.add_setting_group(paperpaw_sdk::SettingGroupSpec::new(ACCOUNT_PAGE, "Account"))?;
        r.add_settings(&ACCOUNT_SETTINGS)
    }
}

/// The app with an account set up: its server, its password and a count.
fn with_account(device: DeviceInfo, config: &str) -> App {
    let config = format!(
        "{config}[acct]
server = https://example.org
count = 20
"
    );
    let mut app = App::with_plugin(device, Shared::memory(&config, &[]), Account);
    app.kernel
        .set_secret(paperpaw_sdk::secrets::SecretChange::keep(
            "acct.password",
            "Password",
            "hunter2",
        ));
    app
}

#[test]
fn a_page_reset_keeps_the_account_and_its_password() {
    for device in DEVICES {
        let mut app = with_account(device, "");
        app.open_screen(
            Id::new_static(GROUP),
            Params::new().with(params::ID, ACCOUNT_PAGE.as_str()),
        );
        choose(&mut app, "Reset to defaults");
        choose(&mut app, "Yes, reset");
        let settings = app.kernel.settings();
        assert!(!settings.is_set("acct.count"));
        assert_eq!(settings.text("acct.server"), "https://example.org");
        assert_eq!(app.kernel.secrets().get("acct.password"), Some("hunter2"));
        assert!(
            app.storage
                .config()
                .contains("server = https://example.org")
        );
    }
}

#[test]
fn everything_reset_keeps_accounts_folders_and_passwords() {
    for device in DEVICES {
        let mut app = with_account(
            device,
            "[core]
library-folder = Books/Mine
",
        );
        app.run(actions::SETTINGS);
        choose(&mut app, "Reset all");
        assert!(app.shows("stay."), "{}", app.described());
        choose(&mut app, "Yes, reset");
        let settings = app.kernel.settings();
        assert!(!settings.is_set("acct.count"));
        assert_eq!(settings.text("acct.server"), "https://example.org");
        assert_eq!(settings.text("core.library-folder"), "Books/Mine");
        assert_eq!(app.kernel.secrets().get("acct.password"), Some("hunter2"));
    }
}

/// Counts the settings work that blocks a firmware loop while reset is
/// confirmed. Memory storage is fast; a card pays for every read.
struct CountConfig {
    inner: MemoryStorage,
    reads: Rc<Cell<usize>>,
    writes: Rc<Cell<usize>>,
}

impl Storage for CountConfig {
    fn read_at(&self, path: &str, offset: u64, bytes: &mut [u8]) -> Result<usize, StorageError> {
        if path == paperpaw_sdk::builtin::CONFIG_FILE && offset == 0 {
            self.reads.set(self.reads.get() + 1);
        }
        self.inner.read_at(path, offset, bytes)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StorageError> {
        if path == paperpaw_sdk::builtin::CONFIG_FILE {
            self.writes.set(self.writes.get() + 1);
        }
        self.inner.write(path, bytes)
    }

    fn append(&self, path: &str, bytes: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, bytes)
    }

    fn list(&self, path: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(path, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
}

struct ResetEvents(Rc<RefCell<Vec<Id>>>);

const TEST_SCOPE: Id = Id::new_static("reset-events.scope");
static TEST_SCOPE_ACTION: [paperpaw_sdk::FixedAction; 1] =
    [
        paperpaw_sdk::FixedAction::new(TEST_SCOPE, "Test scope").handler(|_, _, out| {
            out.push(paperpaw_sdk::Command::SetScope(Some(
                "book-settings.ini".into(),
            )));
        }),
    ];

impl paperpaw_sdk::Plugin for ResetEvents {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("reset-events"),
            "Reset events",
            paperpaw_sdk::Version::new(0, 1, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        r.add_setting(
            paperpaw_sdk::SettingSpec::number(
                "reset-events.count",
                "Count",
                1,
                20,
                2,
                paperpaw_sdk::builtin::groups::OTHER,
            )
            .scoped(),
        )?;
        r.add_actions(&TEST_SCOPE_ACTION)?;
        let events = self.0.clone();
        r.on(
            paperpaw_sdk::builtin::events::SETTING_CHANGED,
            move |_, changed, _| {
                events.borrow_mut().push(changed.id.clone());
            },
        )
    }
}

#[test]
fn resetting_a_books_values_does_not_fall_through_to_global_values() {
    for device in DEVICES {
        let storage = Shared::memory(
            "[reset-events]\ncount = 9\n",
            &[(
                "book-settings.ini",
                b"[core]\nscope-only = on\n[reset-events]\ncount = 2\n",
            )],
        );
        let mut app = App::with_plugin(device, storage, ResetEvents(Rc::default()));
        app.run(actions::SETTINGS);
        app.run(TEST_SCOPE);
        assert_eq!(app.kernel.settings().int("reset-events.count"), 2);
        choose(&mut app, "Reset all");
        choose(&mut app, "Yes, reset");
        assert_eq!(app.kernel.settings().int("reset-events.count"), 9);
        assert!(app.storage.config().contains("count = 9"));
        assert!(!app.kernel.settings().bool("core.scope-only"));
    }
}

#[test]
fn reset_all_does_not_reload_unchanged_defaults_or_reread_the_card_for_them() {
    for device in DEVICES {
        let reads = Rc::new(Cell::new(0));
        let writes = Rc::new(Cell::new(0));
        let changed = Rc::new(RefCell::new(Vec::new()));
        let storage = Shared(Rc::new(CountConfig {
            inner: MemoryStorage::new(),
            reads: reads.clone(),
            writes: writes.clone(),
        }));
        storage.set_config("[core]\nnight-mode = on\n");
        storage.write(SETUP_DONE, b"").unwrap();
        let mut app = App::with_plugin(device, storage, ResetEvents(changed.clone()));
        app.run(actions::SETTINGS);
        choose(&mut app, "Reset all");
        reads.set(0);
        writes.set(0);
        changed.borrow_mut().clear();
        let started = std::time::Instant::now();
        choose(&mut app, "Yes, reset");
        let elapsed = started.elapsed();
        let changed = changed.borrow();
        println!(
            "{} reset: {} events, {} config reads, {} config writes, {elapsed:?}",
            device.id,
            changed.len(),
            reads.get(),
            writes.get()
        );
        for unchanged in [
            "core.reading-font",
            "core.status-font",
            "core.keymap-preset",
        ] {
            assert!(
                !changed.iter().any(|id| id.as_str() == unchanged),
                "an unset default was reapplied: {unchanged}"
            );
        }
        assert!(changed.iter().any(|id| id.as_str() == "core.night-mode"));
        assert!(!app.kernel.settings().bool("core.night-mode"));
        assert!(
            reads.get() <= changed.len() + 1,
            "unneeded settings-file reads"
        );
        assert!(
            changed.len() < 10,
            "a small saved configuration caused {} resets",
            changed.len()
        );
    }
}

/// *Restart now*, last in the settings, asks first: *Cancel* leaves
/// things as they are, *Restart* restarts.
#[test]
fn restart_now_asks_then_restarts() {
    for device in DEVICES {
        let mut app = settings(device, "");
        choose(&mut app, "Restart");
        assert!(app.shows("Restart PaperPaw now?"), "{}", app.described());
        choose(&mut app, "Cancel");
        assert!(!app.restarted);
        choose(&mut app, "Restart");
        choose(&mut app, "Restart");
        assert!(app.restarted, "{}", app.described());
    }
    // The last of the settings' rows: the one before the focus goes round
    // to the first.
    let mut app = settings(X4, "");
    let mut last = None;
    for _ in 0..200 {
        app.run(actions::DOWN);
        let focused = app.focused();
        if focused.as_deref() == Some("Search") && last.is_some() {
            break;
        }
        last = focused;
    }
    assert_eq!(last.as_deref(), Some("Restart"));
}

#[test]
fn the_settings_open_on_a_setting_focused() {
    let mut app = settings(X4, "");
    app.open_slot(
        slots::SETTINGS,
        Params::new().with(params::FOCUS, "core.font-size"),
    );
    assert!(
        app.focused().unwrap().starts_with("Font size"),
        "{}",
        app.described()
    );
}

/// The installed-plugin manager and the group of plugin preferences are
/// different destinations, so their settings rows must say which opens.
#[test]
#[ignore = "BLOCKED (#228): the interface-7 plugin manager's menu place waits for the host (#944, H1/H3/H4); its row is not in this app"]
fn plugin_management_and_plugin_settings_have_distinct_names_and_destinations() {
    for device in DEVICES {
        for (language, manager, preferences, screens, network) in [
            (
                "en",
                "Manage plugins",
                "Plugin settings",
                "Screens",
                "Network per plugin",
            ),
            (
                "de",
                "Plugins verwalten",
                "Plugin-Einstellungen",
                "Bildschirme",
                "Netz je Plugin",
            ),
        ] {
            let mut app = settings(device, &format!("[core]\nlanguage = {language}\n"));
            let mut pages = Vec::new();
            let mut all_rows = Vec::new();
            for _ in 0..12 {
                let page = titles(&mut app);
                if pages.contains(&page) {
                    break;
                }
                pages.push(page.clone());
                all_rows.extend(page);
                if touch(&app) {
                    app.tap_next_page();
                } else {
                    app.run(actions::NEXT_PAGE);
                }
            }
            for title in [manager, preferences] {
                assert_eq!(
                    all_rows.iter().filter(|row| row.as_str() == title).count(),
                    1,
                    "{language} on {}: {all_rows:?}",
                    device.id
                );
            }
            assert!(!all_rows.iter().any(|row| row == "Plugins"), "{all_rows:?}");
            choose(&mut app, manager);
            assert_eq!(app.screen(), "plugin-manager.list");
            assert!(app.shows(screens), "{}", app.described());
            app.run(actions::BACK);
            choose(&mut app, preferences);
            assert_eq!(app.screen(), GROUP);
            assert!(app.shows(network), "{}", app.described());
        }
    }
}

/// A data plugin with two themes: presets of the kind `core.theme`.
const THEMES: &[(&str, &[u8])] = &[
    (
        "plugins/themes/plugin.ini",
        b"[plugin]\nid = themes\nname = Themes\ndescription = Two themes.\nversion = 1.0.0\nformats = 1\n",
    ),
    (
        "plugins/themes/presets.ini",
        b"[tall]\nkind = core.theme\nname = Tall\ndescription = Taller rows.\ncore.ui-row = 180\ncore.ui-size = 20\n\n[framed]\nkind = core.theme\nname = Framed\ndescription = A frame for the focus.\ncore.ui-focus = core.focus.ring\n",
    ),
];

/// Themes are chosen as a whole at the top of the appearance settings;
/// choosing one gives the others' values back their defaults.
#[test]
fn a_theme_is_chosen_at_the_top_of_the_appearance_settings() {
    for device in DEVICES {
        // The reader's own text size, which no theme touches.
        let mut app = App::new(
            device,
            Shared::memory(
                "[core]
ui-size = 14
",
                THEMES,
            ),
        );
        let look = || {
            Params::new().with(
                params::ID,
                paperpaw_sdk::builtin::groups::LOOK.as_str().to_string(),
            )
        };
        app.open_slot(slots::SETTINGS, look());
        // The text size is not a theme's: the appearance is as it comes.
        let first = rows(&mut app)[0].clone();
        assert!(
            first.starts_with("Theme") && first.contains("None"),
            "{first}"
        );
        choose(&mut app, "Theme");
        assert_eq!(app.screen(), "settings.themes", "{}", app.described());
        choose(&mut app, "Tall");
        assert_ne!(app.screen(), "settings.themes");
        assert_eq!(app.kernel.settings().int("core.ui-row"), 180);
        // It names the text size too: left as the reader has it.
        assert_eq!(app.kernel.settings().int("core.ui-size"), 14);
        assert!(rows(&mut app)[0].contains("Tall"), "{:?}", rows(&mut app));
        // The other theme: its value set, the first one's at its default.
        choose(&mut app, "Theme");
        choose(&mut app, "Framed");
        let settings = app.kernel.settings();
        assert_eq!(settings.text("core.ui-focus"), "core.focus.ring");
        assert!(!settings.is_set("core.ui-row"));
        assert_eq!(settings.int("core.ui-size"), 14);
        // None: the appearance as it comes, the text size still the reader's.
        choose(&mut app, "Theme");
        choose(&mut app, "None");
        let settings = app.kernel.settings();
        assert!(!settings.is_set("core.ui-focus"));
        assert_eq!(settings.int("core.ui-size"), 14);
        assert!(rows(&mut app)[0].contains("None"), "{:?}", rows(&mut app));
    }
    // Without themes, no such row.
    let mut app = App::new(X4, Shared::memory("", &[]));
    app.open_slot(
        slots::SETTINGS,
        Params::new().with(
            params::ID,
            paperpaw_sdk::builtin::groups::LOOK.as_str().to_string(),
        ),
    );
    assert!(!rows(&mut app)[0].starts_with("Theme"));
}

#[test]
fn a_choice_with_nothing_to_choose_is_not_listed() {
    for device in DEVICES {
        let mut app = settings(device, "");
        // The page frame offers one column alone until a plugin adds a
        // frame: nothing to choose.
        search(&mut app, "page frame");
        let hits = titles(&mut app);
        assert!(!hits.iter().any(|h| h == "Page frame"), "{hits:?}");
        // A choice of two is (the clock's 24 or 12 hours).
        app.run(actions::BACK);
        search(&mut app, "clock");
        let hits = titles(&mut app);
        assert!(hits.iter().any(|h| h == "Clock"), "{hits:?}");
    }
}

/// A choice's list (here the week's first day) on a screen much wider than
/// a line of text: its rows stand in the middle, as those of every page do.
#[test]
fn the_rows_of_a_choice_stand_in_the_readable_column_on_a_wide_screen() {
    let wide = TOUCH.with_display(DisplayInfo::new(
        1920,
        1200,
        220,
        PixelFormat::Mono1,
        Rotation::Deg0,
        None,
    ));
    let mut app = App::new(wide, Shared::memory("", &[]));
    app.run(actions::SETTINGS);
    search(&mut app, "week starts");
    let hit = titles(&mut app).remove(0);
    choose(&mut app, &hit);
    choose(&mut app, &hit);
    assert_eq!(app.screen(), CHOOSER, "{}", app.described());
    let rows: Vec<_> = app
        .parts()
        .into_iter()
        .filter(|p| p.role == Role::Row)
        .map(|p| p.area)
        .collect();
    assert_eq!(rows.len(), 3);
    for area in &rows {
        // In the middle, not from edge to edge.
        assert!(area.x > 100, "{area:?}");
        assert!(
            u32::from(area.x) + u32::from(area.width) < 1920 - 100,
            "{area:?}"
        );
        assert_eq!(area.x, rows[0].x);
    }
}
