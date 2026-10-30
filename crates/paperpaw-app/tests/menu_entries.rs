//! Menu entries of plugins in the shipped menu: a menu the user
//! redefined shows the entries of plugins installed since (after the
//! entry before them), but not where it was redefined before that was
//! kept; entries placed beside others or taken out by a plugin; and
//! entries that show what they are now (a switch, faint, hidden) with a
//! line under them, said only by the plugin they belong to.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::borrow::Cow;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::semantics::Part;
use paperpaw_sdk::{
    ActionSpec, Command, Id, ItemState, Manifest, MemoryStorage, MenuEntry, MenuPlace, Params,
    Plugin, RegisterError, Registrar, SDK_VERSION,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
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

const MENU: Id = Id::new_static("early.menu");
const CUSTOMIZE: Id = Id::new_static("early.customize");

fn action(id: &'static str) -> ActionSpec {
    ActionSpec::new(Id::new_static(id), id)
}

/// A menu of two entries, A and B, and an action that redefines it as B,
/// A.
struct Early;

impl Plugin for Early {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("early"), "Early", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (id, order) in [("early.a", 10), ("early.b", 20)] {
            r.add_action(action(id).handler(|_, _, _| {}))?;
            r.add_menu_entry(MenuEntry::new(MENU, Id::new_static(id)).order(order))?;
        }
        r.add_action(action("early.customize").handler(|_, _, out| {
            let ids = vec![Id::new_static("early.b"), Id::new_static("early.a")];
            out.push(Command::SetMenu(MENU, Some(ids)));
        }))?;
        r.add_translations(Cow::Borrowed(
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\nearly.a = Early A\nearly.b = Early B\n",
        ))
    }
}

/// Installed later: C right after A; and entries that say what they are
/// now, one with a line under it.
struct Late;

impl Plugin for Late {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("late"), "Late", SDK_VERSION).uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // Something to open besides.
        r.add_action(action("late.app").handler(|_, _, _| {}))?;
        r.add_menu_entry(MenuEntry::new(
            paperpaw_sdk::builtin::menus::APPS,
            Id::new_static("late.app"),
        ))?;
        for id in ["late.c", "late.on", "late.faint", "late.gone"] {
            r.add_action(action(id).handler(|_, _, _| {}))?;
        }
        r.place_menu_entry(
            MENU,
            Id::new_static("late.c"),
            MenuPlace::After(Id::new_static("early.a")),
        )?;
        for id in ["late.on", "late.faint", "late.gone"] {
            r.place_menu_entry(MENU, Id::new_static(id), MenuPlace::Last)?;
        }
        r.set_item_state(Id::new_static("late.on"), |_| {
            ItemState::new().checked(true)
        })?;
        r.set_item_state(Id::new_static("late.faint"), |_| {
            ItemState::new().enabled(false).label("Not now")
        })?;
        r.set_item_state(Id::new_static("late.gone"), |_| {
            ItemState::new().shown(false)
        })?;
        r.describe_item(Id::new_static("late.on"), "late.on.help")?;
        r.add_translations(Cow::Borrowed(
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\nlate.c = Late C\nlate.on = Late on\nlate.faint = Late faint\nlate.gone = Late gone\nlate.on.help = Says what it does\n",
        ))
    }
}

/// Late after an update: C renamed to *See*, the entries saying what
/// they are now gone.
struct Renamed;

impl Plugin for Renamed {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("late"), "Late", SDK_VERSION).uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for id in ["late.see", "late.on"] {
            r.add_action(action(id).handler(|_, _, _| {}))?;
        }
        r.place_menu_entry(
            MENU,
            Id::new_static("late.see"),
            MenuPlace::After(Id::new_static("early.a")),
        )?;
        r.place_menu_entry(MENU, Id::new_static("late.on"), MenuPlace::Last)?;
        r.add_translations(Cow::Borrowed(
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\nlate.see = Late see\nlate.on = Late on\n",
        ))
    }
}

/// Tries to say what another plugin's entries show.
struct Meddler;

impl Plugin for Meddler {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("meddler"), "Meddler", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let state = r.set_item_state(Id::new_static("early.a"), |_| {
            ItemState::new().label("Meddled").enabled(false)
        });
        assert!(matches!(state, Err(RegisterError::Namespace { .. })));
        let line = r.describe_item(Id::new_static("early.a"), "meddler.line");
        assert!(matches!(line, Err(RegisterError::Namespace { .. })));
        Ok(())
    }
}

/// Takes B out of the menu.
struct Hider;

impl Plugin for Hider {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("hider"), "Hider", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.hide_menu_entry(MENU, Id::new_static("early.b"))
    }
}

struct App {
    kernel: Kernel,
    now: u64,
}

impl App {
    fn new(
        config: &str,
        plugins: impl FnOnce(paperpaw_core::KernelBuilder) -> paperpaw_core::KernelBuilder,
    ) -> Self {
        let storage = MemoryStorage::new()
            .with(
                "paperpaw.ini",
                format!("[help]\nshow = false\n[core]\nlanguage = en\n{config}"),
            )
            .with(".paperpaw/plugins/setup/done", "");
        let builder = paperpaw_app::install(
            Kernel::builder(DEVICE).storage(storage),
            paperpaw_app::Fonts::Packs,
        );
        let mut kernel = plugins(builder).build().unwrap();
        let effects = kernel.start(Instant(0));
        let mut app = Self { kernel, now: 0 };
        app.settle(effects);
        app
    }

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

    /// The menu opened, its rows as described.
    fn menu(&mut self) -> Vec<Part> {
        self.now += 100;
        let params = Params::new().with(params::MENU, MENU.as_str());
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::MENU, params);
        self.settle(effects);
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    /// The labels of the tools' menu.
    fn tools(&mut self) -> Vec<String> {
        self.now += 100;
        let params = Params::new().with(params::MENU, "core.tools");
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::MENU, params);
        self.settle(effects);
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }

    /// The labels of the menu's entries, in order.
    fn entries(&mut self) -> Vec<String> {
        let wanted = ["Early", "Late", "Not now"];
        self.menu()
            .into_iter()
            // Without the line under it.
            .map(|p| p.label.split(", ").next().unwrap_or_default().to_string())
            .filter(|l| wanted.iter().any(|w| l.starts_with(w)))
            .collect()
    }

    fn config(&self) -> String {
        let data =
            paperpaw_sdk::read_file(self.kernel.storage(), "paperpaw.ini", 64 * 1024).unwrap();
        String::from_utf8(data).unwrap()
    }
}

#[test]
fn a_redefined_menu_shows_the_entries_of_plugins_installed_since() {
    let mut app = App::new("", |b| b.plugin(Early));
    app.run(CUSTOMIZE);
    let config = app.config();
    assert!(
        config.contains("[menus]\nearly.menu = early.b, early.a"),
        "{config}"
    );
    // What was there and left out: nothing.
    assert!(
        config.contains("[menus-known]\nearly.menu = \"\"\n"),
        "{config}"
    );
    // The same storage after a plugin was installed: C after A.
    let kept = config.replace("[core]\nlanguage = en\n", "");
    let mut later = App::new(&kept, |b| b.plugin(Early).plugin(Late));
    let entries = later.entries();
    // The user's B and A, C after A, the plugin's last ones last.
    assert_eq!(
        entries,
        ["Early B", "Early A", "Late C", "Late on", "Not now"]
    );
    // A list kept before entries were known stays as it is.
    let mut old = App::new("[menus]\nearly.menu = early.b, early.a\n", |b| {
        b.plugin(Early).plugin(Late)
    });
    assert_eq!(old.entries(), ["Early B", "Early A"]);
}

#[test]
fn entries_go_where_plugins_place_them_and_are_taken_out() {
    let mut app = App::new("", |b| b.plugin(Early).plugin(Late).plugin(Hider));
    let entries = app.entries();
    // B taken out, C after A, the others last; the hidden one not shown.
    assert_eq!(
        entries,
        ["Early A", "Late C", "Late on", "Not now"],
        "{entries:?}"
    );
}

#[test]
fn entries_show_what_they_are_now_with_a_line_under_them() {
    let mut app = App::new("", |b| b.plugin(Early).plugin(Late));
    let parts = app.menu();
    let on = parts
        .iter()
        .find(|p| p.label.starts_with("Late on"))
        .unwrap_or_else(|| panic!("{parts:?}"));
    assert_eq!(on.on, Some(true));
    let labels: Vec<&str> = parts.iter().map(|p| p.label.as_str()).collect();
    assert!(
        labels.iter().any(|l| l.contains("Says what it does")),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.contains("Late gone")),
        "{labels:?}"
    );
}

#[test]
fn what_there_is_to_open_is_a_submenu_of_the_tools_where_there_is_some() {
    let mut without = App::new("", |b| b.plugin(Early));
    assert!(!without.tools().iter().any(|l| l.starts_with("Apps")));
    let mut with = App::new("", |b| b.plugin(Early).plugin(Late));
    let tools = with.tools();
    assert!(tools.iter().any(|l| l.starts_with("Apps")), "{tools:?}");
}

#[test]
fn entries_removed_or_renamed_since_a_menu_was_redefined_are_handled() {
    let mut app = App::new("", |b| b.plugin(Early).plugin(Late));
    app.run(CUSTOMIZE);
    let config = app.config();
    // Late's entries, left out, are known as such; the listed ones not.
    assert!(
        config.contains("[menus-known]\nearly.menu = late.c, late.on, late.faint, late.gone\n"),
        "{config}"
    );
    // An update of Late renames C and drops the rest; the user had also
    // listed C.
    let kept = config
        .replace("[core]\nlanguage = en\n", "")
        .replace("early.b, early.a", "early.b, late.c, early.a");
    let mut later = App::new(&kept, |b| b.plugin(Early).plugin(Renamed));
    // C is gone, its new name new: after A. *On*, left out, stays out.
    assert_eq!(later.entries(), ["Early B", "Early A", "Late see"]);
}

#[test]
fn only_its_own_plugin_says_what_an_entry_shows() {
    let mut app = App::new("", |b| b.plugin(Early).plugin(Meddler));
    let parts = app.menu();
    let a = parts
        .iter()
        .find(|p| p.label.starts_with("Early A"))
        .unwrap_or_else(|| panic!("{parts:?}"));
    assert_eq!(a.label, "Early A");
}

#[test]
fn the_menu_editor_tells_who_took_an_entry_out() {
    let mut app = App::new("[plugins]\nmenu-editor = on\n", |b| {
        b.plugin(Early).plugin(Hider)
    });
    app.now += 100;
    let params = Params::new().with("menu", MENU.as_str());
    let effects =
        app.kernel
            .open_screen(Instant(app.now), Id::new_static("menu-editor.add"), params);
    app.settle(effects);
    // Page by page to B (sorted by name).
    let mut labels = Vec::new();
    for _ in 0..50 {
        app.kernel.render(&mut |_, _| {});
        labels = app.kernel.describe().into_iter().map(|p| p.label).collect();
        if labels.iter().any(|l| l.contains("Early B")) {
            break;
        }
        app.run(paperpaw_sdk::builtin::actions::NEXT_PAGE);
    }
    assert!(
        labels
            .iter()
            .any(|l| l.contains("Early B") && l.contains("Taken out by Hider")),
        "{labels:?}"
    );
}
