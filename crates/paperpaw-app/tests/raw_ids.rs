//! No screen made from what plugins register shows an id where a name
//! belongs: in every language shipped, every row of the menu editor (its
//! list of menus, page by page, and each menu's entries), of every
//! settings group and of every choice is a name, never a raw id such as
//! `core.status-font` (a key no catalog has, or no key at all); and so is
//! every entry of every menu, as the menus show them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeSet;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::{Id, MemoryStorage, Params, SettingKind};

/// An X4 Pro as the emulator has it: buttons, no touch.
const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4-pro",
    "Test X4 Pro",
    DisplayInfo::new(528, 880, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

/// The most rows a list is walked through: more than any has.
const MOST_ROWS: usize = 120;

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

struct App {
    kernel: Kernel,
    now: u64,
    /// The first part of every id: the plugins' and the kernel's.
    namespaces: BTreeSet<String>,
}

impl App {
    /// The standard app in `language`, past its first-run setup.
    fn new(language: &str) -> Self {
        let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config(format!(
                "[core]\nlanguage = {language}\n[help]\nshow = false\n"
            ));
        let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
        let effects = kernel.start(Instant(0));
        let mut namespaces: BTreeSet<String> = kernel
            .plugins()
            .iter()
            .map(|p| p.manifest.id.to_string())
            .collect();
        namespaces.insert("core".to_string());
        let mut app = Self {
            kernel,
            now: 0,
            namespaces,
        };
        app.settle(effects);
        app
    }

    /// Lets timers run until nothing is due within a second.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn open(&mut self, screen: Id, params: Params) {
        self.now += 100;
        let effects = self.kernel.open_screen(Instant(self.now), screen, params);
        self.settle(effects);
    }

    /// The labels the screen shows, and which is focused.
    fn shown(&mut self) -> (Vec<String>, Option<String>) {
        self.kernel.render(&mut |_, _| {});
        let parts = self.kernel.describe();
        let focused = parts.iter().find(|p| p.focused).map(|p| p.label.clone());
        (parts.into_iter().map(|p| p.label).collect(), focused)
    }

    /// Every label of the screen's list, page by page: down until the
    /// focus comes back or stays.
    fn walk(&mut self) -> BTreeSet<String> {
        let (labels, first) = self.shown();
        let mut seen: BTreeSet<String> = labels.into_iter().collect();
        let mut last = first.clone();
        for _ in 0..MOST_ROWS {
            self.run(actions::DOWN);
            let (labels, focused) = self.shown();
            seen.extend(labels);
            if focused.is_none() || focused == first || focused == last {
                break;
            }
            last = focused;
        }
        seen
    }

    /// The ids among `labels`: a part of a label (between ` › `, `/` or
    /// on a line of its own) that is a dotted id in a plugin's or the
    /// kernel's namespace.
    fn raw_ids<'a>(&self, labels: impl IntoIterator<Item = &'a String>) -> Vec<String> {
        let mut found = Vec::new();
        for label in labels {
            for part in label.split(['\n', '/', '›', ',', ':']) {
                let part = part.trim();
                if self.is_id(part) {
                    found.push(format!("{part:?} in {label:?}"));
                }
            }
        }
        found
    }

    fn is_id(&self, text: &str) -> bool {
        let Some((namespace, rest)) = text.split_once('.') else {
            return false;
        };
        self.namespaces.contains(namespace)
            && !rest.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-._@".contains(c))
    }
}

/// The languages shipped, by tag.
fn languages() -> Vec<String> {
    let app = App::new("en");
    app.kernel
        .translator()
        .languages()
        .into_iter()
        .map(|(tag, _)| tag.to_string())
        .collect()
}

#[test]
fn ids_are_found_where_shown() {
    let app = App::new("en");
    assert!(app.is_id("core.status-font"));
    assert!(app.is_id("menu-editor.title"));
    assert!(!app.is_id("Status lines"));
    assert!(!app.is_id("paperpaw.org"));
    assert!(!app.is_id("1.5"));
    let shown = ["Book".to_string(), "Font › core.status-font".to_string()];
    assert_eq!(app.raw_ids(&shown).len(), 1);
}

/// The menu editor names every menu, on every page, and every entry of
/// each; a setting's chooser's menu (more fonts, say) by the setting.
#[cfg(feature = "store-plugins")]
#[test]
fn the_menu_editor_names_every_menu_and_entry() {
    for language in languages() {
        let mut app = App::new(&language);
        app.run(Id::new_static("menu-editor.open"));
        let mut shown = app.walk();
        let menus: Vec<String> = app
            .kernel
            .menus()
            .menu_ids()
            .into_iter()
            .map(ToString::to_string)
            .collect();
        for menu in menus {
            // The editor's own screen of a menu's entries.
            app.open(
                Id::new_static("menu-editor.entries"),
                Params::new().with("menu", menu.clone()),
            );
            shown.extend(app.walk());
            app.run(actions::BACK);
        }
        assert!(shown.len() > 50, "{language}: the walk saw the menus");
        let raw = app.raw_ids(&shown);
        assert!(
            raw.is_empty(),
            "{language}: the menu editor shows ids: {raw:#?}"
        );
    }
}

/// Every settings group names its settings and their values, and every
/// choice its options and more ways to choose.
#[test]
fn settings_pages_and_choosers_show_names() {
    for language in languages() {
        let mut app = App::new(&language);
        app.run(actions::SETTINGS);
        let mut shown = app.walk();
        let settings = app.kernel.settings();
        let groups: Vec<Id> = settings.groups().map(|g| g.id.clone()).collect();
        let choices: Vec<Id> = settings
            .specs()
            .filter(|s| matches!(s.kind, SettingKind::Choice(_)))
            .map(|s| s.id.clone())
            .collect();
        for group in groups {
            app.open(
                paperpaw_plugin_settings::GROUP_SCREEN,
                Params::new().with(params::ID, group.to_string()),
            );
            shown.extend(app.walk());
            app.run(actions::BACK);
        }
        for choice in choices {
            app.open(
                paperpaw_plugin_settings::CHOOSE_SCREEN,
                Params::new().with(params::ID, choice.to_string()),
            );
            shown.extend(app.walk());
            app.run(actions::BACK);
        }
        assert!(shown.len() > 200, "{language}: the walk saw the pages");
        let raw = app.raw_ids(&shown);
        assert!(raw.is_empty(), "{language}: settings show ids: {raw:#?}");
    }
}

/// Every entry of every menu, as the menus show it: its title.
#[test]
fn menu_entries_have_names() {
    for language in languages() {
        let app = App::new(&language);
        let menus = app.kernel.menus();
        let i18n = app.kernel.translator();
        let shown: Vec<String> = menus
            .menu_ids()
            .into_iter()
            .flat_map(|menu| menus.items(menu.as_str()))
            .map(|item| i18n.tr(item.title).to_string())
            .collect();
        assert!(shown.len() > 50, "{language}: the menus have entries");
        let raw = app.raw_ids(&shown);
        assert!(raw.is_empty(), "{language}: menus show ids: {raw:#?}");
    }
}
