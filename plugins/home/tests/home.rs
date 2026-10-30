//! The home screen on a real kernel: the grid stays on the screen with no
//! margin, pages are edited and added where their settings are, and
//! widgets are made anew only when what makes them changed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat, Point,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::builtin::{actions, menus, params};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::setting::Setting;
use paperpaw_sdk::widgets::{Widget, WidgetSpec};
use paperpaw_sdk::{
    ActionSpec, Canvas, Env, Id, Manifest, MemoryStorage, MenuEntry, Params, Plugin, Rect,
    RegisterError, Registrar, SettingSpec, Version,
};

/// A device with `touch` or buttons alone, `width` pixels wide.
const fn device(touch: bool, width: u16) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(width, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(true, touch, false, &[]),
        MemoryClass::Small,
    )
}

/// How often the probe widget was made.
static MADE: AtomicUsize = AtomicUsize::new(0);

/// A plugin with a widget that counts how often it is made, and a
/// setting of its own that its widget does not have.
/// How often the probe's status line was drawn in the home screen's
/// place.
static LINES: AtomicUsize = AtomicUsize::new(0);

struct Probe;

const PROBE: Id = Id::new_static("probe.widget");
const OTHER: Setting<bool> = Setting::new("probe.other");
const OWN: Setting<bool> = Setting::new("probe.widget.own");
const APP: Id = Id::new_static("probe.app");

struct ProbeWidget;

impl Widget for ProbeWidget {
    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        40
    }

    fn draw(&self, _: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}
}

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", Version::new(0, 1, 0))
            .description("Counts widgets made")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_widget(WidgetSpec::new(PROBE, "probe.widget", |_| {
            MADE.fetch_add(1, Ordering::SeqCst);
            Box::new(ProbeWidget)
        }))?;
        r.add_setting(SettingSpec::switch(
            OTHER.static_str(),
            "probe.other",
            false,
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        r.add_setting(SettingSpec::switch(
            OWN.static_str(),
            "probe.own",
            false,
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        // Something to open, listed with the tools.
        r.add_action(ActionSpec::new(APP, "probe.app").handler(|_, _, _| {}))?;
        r.add_menu_entry(MenuEntry::new(menus::APPS, APP))?;
        // Below the page, beside the pencil.
        r.add_menu_entry(MenuEntry::new(
            paperpaw_plugin_home::FOOTER,
            Id::new_static("probe.switch"),
        ))?;
        r.add_icon(APP, "puzzle")?;
        // A whole status line of its own, that counts its drawings.
        r.add_status_line(paperpaw_sdk::StatusLineSpec::new(
            Id::new_static("probe.line"),
            "probe.line",
            Box::new(|_, _, frame| {
                if frame.place == Some("home.status-line") {
                    LINES.fetch_add(1, Ordering::SeqCst);
                }
            }),
        ))?;
        // A status item that switches `OTHER` on when tapped.
        r.add_status_item(
            paperpaw_sdk::StatusItemSpec::new(
                Id::new_static("probe.status"),
                "probe.status",
                Box::new(|_, _| Some(paperpaw_sdk::StatusValue::Text("Probe".into()))),
            )
            .tap(Id::new_static("probe.switch")),
        )?;
        r.add_action(
            ActionSpec::new(Id::new_static("probe.switch"), "probe.switch").handler(|_, _, out| {
                out.push(paperpaw_sdk::Command::SetSetting(
                    Id::new_static("probe.other"),
                    paperpaw_sdk::SettingValue::Bool(true),
                ));
            }),
        )?;
        r.add_translations(
            "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\nprobe.app = Probe app\n",
        )
    }
}

struct Home {
    kernel: Kernel,
    now: u64,
}

impl Home {
    /// The home screen on a phone-sized device with `touch` or buttons
    /// alone, with `settings` in the user's config (under `[home]`, other
    /// sections after their names).
    fn new(touch: bool, settings: &str) -> Self {
        Self::on(device(touch, 480), settings)
    }

    /// The same on `device`.
    fn on(device: DeviceInfo, settings: &str) -> Self {
        let config = format!("[core]\nlanguage = en\n[help]\nshow = false\n[home]\n{settings}");
        let mut kernel = Kernel::builder(device)
            .storage(MemoryStorage::new())
            .user_config(&config)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(Probe)
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut home = Self { kernel, now: 0 };
        home.settle(effects);
        home
    }

    /// Lets timers run until nothing is due within a second.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..1000 {
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

    fn set(&mut self, id: &str, value: &str) {
        self.now += 100;
        let effects = self
            .kernel
            .set_setting(Instant(self.now), id, value)
            .unwrap();
        self.settle(effects);
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn open(&mut self, screen: &'static str, params: Params) {
        self.now += 100;
        let effects = self
            .kernel
            .open_screen(Instant(self.now), Id::new_static(screen), params);
        self.settle(effects);
    }

    /// What the screen shows: each part's label and place.
    fn shown(&mut self) -> Vec<(String, Rect)> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| (p.label, p.area))
            .collect()
    }

    /// The texts the screen shows, each with its place (not the buttons
    /// round them).
    fn texts(&mut self) -> Vec<(String, Rect)> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .filter(|p| p.role == Role::Text)
            .map(|p| (p.label, p.area))
            .collect()
    }

    /// The buttons the screen shows by what they say, the focused one
    /// marked.
    fn buttons(&mut self) -> Vec<(String, bool)> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .filter(|p| p.role == Role::Button)
            .map(|p| (p.label, p.focused))
            .collect()
    }

    /// A tap on the part of the screen that says `label`.
    fn tap_on(&mut self, label: &str) {
        let area = self
            .shown()
            .into_iter()
            .find(|(l, _)| l.contains(label))
            .unwrap_or_else(|| panic!("nothing says {label}"))
            .1;
        let at = Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        );
        self.tap(at);
    }

    /// A tap at `at`.
    fn tap(&mut self, at: Point) {
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 50;
            let event =
                InputEvent::Pointer(PointerEvent::new(0, phase, at, PointerTool::Finger, 1000));
            let effects = self.kernel.handle(Instant(self.now), event);
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
    }

    fn says(&mut self, text: &str) -> bool {
        self.shown().iter().any(|(label, _)| label.contains(text))
    }
}

/// Every part on the screen lies on it.
fn on_screen(home: &Home, shown: &[(String, Rect)]) {
    let display = home.kernel.device().display;
    for (label, area) in shown {
        assert!(
            area.x + area.width <= display.width && area.y + area.height <= display.height,
            "{label} at {area:?}"
        );
    }
}

#[test]
fn without_margins_the_grid_stays_on_the_screen() {
    let mut home = Home::new(
        true,
        "columns = 4\n\
         page-1 = tool:core.settings 0/0 1x1, tool:core.settings 3/0 1x1\n\
         [core]\nui-margin = 0\n",
    );
    let shown = home.shown();
    on_screen(&home, &shown);
    let texts = home.texts();
    let tools: Vec<_> = texts.iter().filter(|(l, _)| l == "Settings").collect();
    assert_eq!(tools.len(), 2, "{shown:?}");
    // In the first column and the last of four.
    let (left, right) = (tools[0].1, tools[1].1);
    assert!(left.x + left.width <= 120, "{left:?}");
    assert!(right.x >= 360, "{right:?}");
}

#[test]
fn a_page_is_edited_where_its_setting_is() {
    // The second page empty: the third shows second, the fourth third.
    let mut home = Home::new(
        true,
        "page-2 =\n\
         page-3 = tool:core.settings 0/0 1x1\n\
         page-4 = tool:core.library 0/0 1x1\n",
    );
    let page_3 = Params::new().with(params::ID, "home.page-3");
    home.open("home.dashboard", page_3);
    assert!(home.says("Settings"));
    assert!(!home.says("Library"));
}

#[test]
fn an_added_page_shows_until_it_is_removed() {
    let mut home = Home::new(true, "");
    home.open("home.add-widget", Params::new().with("page", "0"));
    home.tap_on("Add a page");
    let settings = home.kernel.settings();
    assert_eq!(settings.text("home.page-2"), "home.tools");
    assert_eq!(settings.text("home.page-3"), paperpaw_sdk::widgets::SPACE);
}

#[test]
fn widgets_are_made_anew_only_when_what_makes_them_changes() {
    let mut home = Home::new(true, "columns = 4\npage-1 = probe.widget 0/0 4x1\n");
    home.shown();
    let made = MADE.load(Ordering::SeqCst);
    assert_eq!(made, 1, "made once as the home screen showed");
    // Another plugin's setting: read anew, not made anew.
    home.set(OTHER.as_str(), "true");
    home.shown();
    assert_eq!(MADE.load(Ordering::SeqCst), made);
    // The widget's own setting, the look of all: made anew.
    home.set(OWN.as_str(), "true");
    home.shown();
    assert_eq!(MADE.load(Ordering::SeqCst), made + 1);
    home.set("core.ui-lines", "5");
    home.shown();
    assert_eq!(MADE.load(Ordering::SeqCst), made + 2);
}

impl Home {
    /// The gray of each pixel of the screen, drawn anew.
    fn pixels(&mut self) -> (u16, Vec<u8>) {
        let display = self.kernel.device().display;
        let (width, height) = (display.width, display.height);
        let mut gray = vec![255u8; usize::from(width) * usize::from(height)];
        self.kernel.render(&mut |top, view| {
            for y in 0..view.height {
                for x in 0..view.width.min(width) {
                    let row = usize::from(top + y);
                    if row < usize::from(height) {
                        gray[row * usize::from(width) + usize::from(x)] = view.pixel(x, y).luma();
                    }
                }
            }
        });
        (width, gray)
    }
}

/// The rows from `top` to `bottom` with ink between `left` and `right`.
fn inked_rows(
    width: u16,
    gray: &[u8],
    (left, right): (u16, u16),
    top: u16,
    bottom: u16,
) -> Vec<u16> {
    (top..bottom)
        .filter(|y| {
            (left..right).any(|x| gray[usize::from(*y) * usize::from(width) + usize::from(x)] < 128)
        })
        .collect()
}

/// A tool's name sits clearly nearer its own icon than the tool below
/// it, on a short screen of buttons alone and on a tall touch screen,
/// in cells as flat as the X4's and as tall as a large screen's.
#[test]
fn a_tools_name_belongs_to_its_own_icon() {
    for (touch, rows) in [(false, 0), (true, 0), (true, 8), (false, 12)] {
        let mut home = Home::new(
            touch,
            &format!(
                "page-1 = tool:core.settings 0/0 1x1, tool:core.settings 0/1 1x1\n\
                 rows = {rows}\nstatus-bar = false\n"
            ),
        );
        let (width, gray) = home.pixels();
        let shown = home.texts();
        let names: Vec<Rect> = shown
            .iter()
            .filter(|(l, _)| l == "Settings")
            .map(|(_, r)| *r)
            .collect();
        assert_eq!(names.len(), 2, "{shown:?}");
        let column = (names[0].x, names[0].x + names[0].width);
        // The first tool's icon: the ink above its name (whose round
        // letters reach a pixel or two above its capitals).
        let above = inked_rows(width, &gray, column, 0, names[0].y.saturating_sub(2));
        let icon_bottom = *above.last().unwrap();
        // Its name's ink, and the second tool's icon below it.
        let name = inked_rows(
            width,
            &gray,
            column,
            names[0].y,
            names[0].y + names[0].height,
        );
        let name_top = name[0];
        let below = inked_rows(
            width,
            &gray,
            column,
            names[0].y + names[0].height,
            names[1].y,
        );
        let (name_bottom, next_icon) = (*name.last().unwrap(), below[0]);
        let inside = name_top - icon_bottom;
        let between = next_icon - name_bottom;
        assert!(
            inside * 2 <= between,
            "rows {rows}, touch {touch}: name {inside} px below its icon, {between} px above the next"
        );
    }
}

/// With buttons alone, the focus goes from tool to tool on the home
/// screen, and the tool it is on is told by its name: what a screen
/// reader says, and what tools follow.
#[test]
fn the_focus_on_the_home_screen_is_told() {
    let mut home = Home::new(
        false,
        "page-1 = tool:core.settings 0/0 1x1, tool:core.library 1/0 1x1
",
    );
    let focused = |home: &mut Home| {
        home.buttons()
            .into_iter()
            .find(|(_, focused)| *focused)
            .map(|(name, _)| name)
    };
    // The page seen as it is first; the first press shows the focus.
    assert_eq!(focused(&mut home), None);
    home.run(actions::NEXT);
    assert_eq!(focused(&mut home).as_deref(), Some("Settings"));
    home.run(actions::NEXT);
    assert_eq!(focused(&mut home).as_deref(), Some("Library"));
}

/// Every tool is a button named after its action, its name shown or not,
/// so that each can be told apart and followed; the focus on one of them.
#[test]
fn every_tool_is_told_by_its_name() {
    for labels in ["on", "off"] {
        let mut home = Home::new(
            false,
            &format!(
                "tool-labels = {labels}\npage-1 = tool:core.settings 0/0 1x1, tool:core.library 1/0 1x1\n"
            ),
        );
        home.run(actions::NEXT);
        let buttons = home.buttons();
        for name in ["Settings", "Library"] {
            let found: Vec<_> = buttons.iter().filter(|(n, _)| n == name).collect();
            assert_eq!(found.len(), 1, "{labels}: {name} in {buttons:?}");
        }
        let focused: Vec<_> = buttons.iter().filter(|(_, f)| *f).collect();
        assert_eq!(focused, [&(String::from("Settings"), true)], "{labels}");
    }
}

#[test]
fn what_there_is_to_open_shows_with_the_tools() {
    let mut home = Home::new(true, "page-1 = home.tools 0/0 4x6\nstatus-bar = false\n");
    let shown = home.texts();
    assert!(shown.iter().any(|(l, _)| l == "Probe app"), "{shown:?}");
}

/// An item of the status line above the grid that does something when
/// tapped does it.
#[test]
fn a_tapped_status_item_above_the_grid_does_what_it_says() {
    let mut home = Home::new(
        true,
        "status-bar = true\nstatus.left = probe.status\nstatus.center =\nstatus.right =\n",
    );
    assert!(!home.kernel.settings().bool("probe.other"));
    home.tap(Point::new(36, 24));
    assert!(home.kernel.settings().bool("probe.other"));
}

/// An action a plugin adds below the page shows beside the pencil, and
/// runs when tapped.
#[test]
fn an_action_below_the_page_runs_when_tapped() {
    let mut home = Home::new(true, "");
    assert!(home.says("probe.switch"), "{:?}", home.shown());
    home.tap_on("probe.switch");
    assert!(home.kernel.settings().bool("probe.other"));
}

/// A whole status line chosen for the home screen's status line shows
/// there, told it is in the home screen's place; by default its items.
#[test]
fn a_status_line_chosen_for_home_shows_there() {
    let before = LINES.load(Ordering::SeqCst);
    let mut home = Home::new(
        true,
        "status-bar = true
status.line = probe.line
",
    );
    home.kernel.render(&mut |_, _| {});
    assert!(LINES.load(Ordering::SeqCst) > before);
    // Its items otherwise, a tap on one doing what it says.
    let mut home = Home::new(
        true,
        "status-bar = true
status.left = probe.status
status.center =
status.right =
",
    );
    home.tap(Point::new(36, 24));
    assert!(home.kernel.settings().bool("probe.other"));
}

/// The clock's sizes are told apart, and as large as fits fills its place:
/// in a cell of 4 by 2 its time is larger than at the largest size set,
/// drawn from the packs as the X4 draws (whose text sizes end at 48
/// pixels, the clock's digits having larger ones of their own).
#[test]
fn the_clocks_sizes_differ_and_as_large_as_fits_fills_its_place() {
    let height = |size: &str| {
        let settings = format!(
            "page-1 = home.clock 0/0 4x2, home.continue 0/2 4x2
clock.size = core.size.{size}
"
        );
        let mut home = Home::new(false, &settings);
        // 10:41 in the morning.
        let clock = paperpaw_sdk::WallClock::new(Instant(home.now), 1_760_438_460_000, Some(0));
        let status = paperpaw_sdk::SystemStatus::new(Some(clock), None);
        let effects = home.kernel.set_status(Instant(home.now), status);
        home.settle(effects);
        let texts = home.texts();
        let (_, at) = texts
            .iter()
            .find(|(text, _)| text.contains(':'))
            .unwrap_or_else(|| panic!("no time in {texts:?}"));
        at.height
    };
    let sizes = ["small", "medium", "large", "fit"].map(height);
    assert!(sizes.windows(2).all(|w| w[0] < w[1]), "{sizes:?}");
    // The time alone, as large as its cell's width allows: well over the
    // 48 pixels text has at most.
    assert!(sizes[3] > 60, "{sizes:?}");
}

/// Fewer columns than the page was arranged for: every item still shows,
/// moved inside the grid (narrower where it was wider than the grid),
/// none lost; what a page has no more room for goes to the next page.
#[test]
fn fewer_columns_keep_every_item_in_sight() {
    let mut home = Home::new(
        false,
        "columns = 4\nrows = 6\n\
         page-1 = tool:core.library 0/0 1x1, tool:core.settings 3/0 1x1, home.date 0/1 4x1\n",
    );
    home.set("home.columns", "3");
    let texts = home.texts();
    for name in ["Library", "Settings"] {
        assert!(texts.iter().any(|(t, _)| t == name), "{name}: {texts:?}");
    }
    let page = paperpaw_sdk::widgets::grid::parse(home.kernel.settings().text("home.page-1"));
    assert_eq!(page.len(), 3, "{page:?}");
    for item in &page {
        let at = item.at.expect("placed");
        assert!(at.fits(3, 6), "{item:?}");
    }
    // A full page: what has no room goes on to the next one.
    let full = (0..4)
        .flat_map(|row| (0..4).map(move |col| alloc_free(col, row)))
        .collect::<Vec<_>>()
        .join(", ");
    let mut home = Home::new(false, &format!("columns = 4\nrows = 4\npage-1 = {full}\n"));
    home.set("home.columns", "3");
    let settings = home.kernel.settings();
    let first = paperpaw_sdk::widgets::grid::parse(settings.text("home.page-1"));
    let second = paperpaw_sdk::widgets::grid::parse(settings.text("home.page-2"));
    let tools = |page: &[paperpaw_sdk::widgets::grid::Item]| {
        page.iter()
            .filter(|i| matches!(i.what, paperpaw_sdk::widgets::grid::What::Tool(_)))
            .count()
    };
    assert_eq!(tools(&first) + tools(&second), 16, "{first:?} {second:?}");
    assert_eq!(first.len(), 12);
}

/// A tool in cell `col`/`row`, one of 16 apart.
fn alloc_free(col: u8, row: u8) -> String {
    let tools = [
        "core.library",
        "core.settings",
        "core.home",
        "core.tools",
        "core.back",
        "core.menu",
        "core.sleep",
        "core.refresh-screen",
        "core.next-page",
        "core.prev-page",
        "core.night-mode",
        "core.screenshot",
        "core.quick-panel",
        "core.up",
        "core.down",
        "core.left",
    ];
    format!("tool:{} {col}/{row} 1x1", tools[usize::from(row * 4 + col)])
}
