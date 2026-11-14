//! No screen made from what plugins register shows an id where a name
//! belongs: in every language shipped, every title and note of every
//! settings group and every option of every choice is a name, never a raw id such as
//! `core.status-font` (a key no catalog has, or no key at all); and so is
//! every entry of every menu, as the menus show them. The same walk over
//! the menu editor is the pending case `menu-editor-names-every-menu-and-entry`
//! (`plugins/menu-editor/tests/acceptance_pending.rs`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;

use std::collections::BTreeSet;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::actions;
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
        let (builder, card) = native_activation::quiet_on(
            DEVICE,
            storage,
            &format!("[core]\nlanguage = {language}\n"),
        );
        let mut kernel = native_activation::build_on(paperpaw_app::install(builder, fonts()), card);
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
    assert!(app.is_id("core.settings-pages"));
    assert!(!app.is_id("Status lines"));
    assert!(!app.is_id("paperpaw.org"));
    assert!(!app.is_id("1.5"));
    let shown = ["Book".to_string(), "Font › core.status-font".to_string()];
    assert_eq!(app.raw_ids(&shown).len(), 1);
}

/// Every settings group names its settings and their values, and every
/// choice its options. The settings screen is native (interface 7) and
/// not run by this kernel, so this is what the screens are made of: the
/// titles and notes the registry holds, translated as the screens do
/// (`NativeSettings` reads the same registry and the same catalogs).
#[test]
#[cfg_attr(
    feature = "native-linked",
    ignore = "waits for the native settings, help, lock, files and file-manager plugins: core.secrets, document/library/files services (desktop) and event 87 (#925)"
)]
fn settings_pages_and_choosers_show_names() {
    for language in languages() {
        let app = App::new(&language);
        let settings = app.kernel.settings();
        let menus = app.kernel.menus();
        let i18n = app.kernel.translator();
        let mut shown: Vec<String> = Vec::new();
        for group in settings.groups() {
            shown.push(i18n.tr(&group.title).to_string());
        }
        for spec in settings.specs() {
            shown.push(i18n.tr(&spec.title).to_string());
            if let Some(note) = spec.description.as_deref() {
                shown.push(i18n.tr(note).to_string());
            }
            if matches!(spec.kind, SettingKind::Choice(_)) {
                for item in menus.items(spec.id.as_str()) {
                    shown.push(i18n.tr(item.title).to_string());
                }
            }
        }
        assert!(shown.len() > 100, "{language}: the registry was read");
        let raw = app.raw_ids(&shown);
        assert!(raw.is_empty(), "{language}: settings show ids: {raw:#?}");
    }
}

/// Every entry of every menu, as the menus show it: its title.
#[test]
fn menu_entries_have_names() {
    for language in languages() {
        let mut app = App::new(&language);
        let shown = if cfg!(feature = "native-linked") {
            app.menus_shown()
        } else {
            let menus = app.kernel.menus();
            let i18n = app.kernel.translator();
            menus
                .menu_ids()
                .into_iter()
                .flat_map(|menu| menus.items(menu.as_str()))
                .map(|item| i18n.tr(item.title).to_string())
                .collect()
        };
        assert!(
            shown.len() > 50,
            "{language}: the menus have entries: {shown:#?}"
        );
        let raw = app.raw_ids(&shown);
        assert!(raw.is_empty(), "{language}: menus show ids: {raw:#?}");
    }
}

/// The menus the linked native plugins declare entries in, the main menu first.
const NATIVE_MENUS: [&str; 8] = [
    "core.main",
    "core.tools",
    "core.settings-pages",
    "core.reader-more",
    "files.shelves",
    "files.menu",
    "home.actions",
    "plugin-manager.more",
];

impl App {
    /// With the native menu plugin the menus are its screens: every menu the
    /// kernel knows and those the native plugins add to ([`NATIVE_MENUS`]), opened
    /// on `menu.list` (its id as the hand-over's `menu`) and walked row by row;
    /// the entries each menu shows (an entry in two menus counts twice, as the
    /// interface-6 registry listed it), not the note of an empty menu.
    fn menus_shown(&mut self) -> Vec<String> {
        let mut ids: BTreeSet<String> = self
            .kernel
            .menus()
            .menu_ids()
            .into_iter()
            .map(ToString::to_string)
            .collect();
        // The menus the linked native plugins add entries to
        // (`MenuEntryDecl` in their `declarations.rs`): the kernel's own
        // registry does not know those it does not hold itself.
        ids.extend(NATIVE_MENUS.iter().map(ToString::to_string));
        let mut shown = Vec::new();
        for id in ids {
            self.open(
                Id::new_static("menu.list"),
                Params::new().with("menu", id.clone()),
            );
            assert_eq!(
                self.kernel.active_screen().map(Id::as_str),
                Some("menu.list"),
                "the menu {id} opens"
            );
            let empty = self.kernel.translator().tr("menu.empty").to_string();
            shown.extend(
                self.walk()
                    .into_iter()
                    .filter(|label| !label.is_empty() && *label != empty),
            );
            self.run(actions::BACK);
        }
        shown
    }
}
