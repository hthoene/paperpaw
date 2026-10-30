//! Every widget of the pack drawn where the home screen can place it:
//! at every size from the smallest grid cell to the whole screen, on a
//! panel of grays and on one of black and white, at awkward times (new
//! year, the last minute of the year, a leap day, before 1970, no clock
//! at all), with every value of each of its settings, with hostile texts
//! in its settings and hostile files where it keeps its own. Whatever it
//! draws stays inside its place (the canvas round it is untouched), the
//! places a tap acts on lie inside it, and nothing panics.
//!
//! The widgets draw with the default fonts as bitmap packs, the same
//! whichever features the fonts plugin is built with; problems that show
//! only with its scalable fonts have tests of their own.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};

use paperpaw_graphics::Framebuffer;
use paperpaw_i18n::{Catalog, Translator};
use paperpaw_plugin_widgets::Widgets as WidgetPack;
use paperpaw_sdk::builtin::settings as core_settings;
use paperpaw_sdk::widgets::{Widgets, categories};
use paperpaw_sdk::{
    Actions, Battery, Capabilities, Color, Contribution, DeviceInfo, DisplayInfo, Env, EnvParts,
    Fonts, Formats, Hyphenation, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Menus,
    Metrics, PixelFormat, Plugin, Plugins, Rect, Registrar, SettingKind, SettingSpec, SettingValue,
    Settings, StatusItems, Storage, SystemStatus, WallClock,
};

const fn device(format: PixelFormat, touch: bool) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(
            480,
            800,
            220,
            format,
            paperpaw_platform::Rotation::Deg0,
            None,
        ),
        InputCaps::new(true, touch, false, &[]),
        MemoryClass::Small,
    )
}

/// A panel of sixteen grays with touch (a Boox), and one of black and
/// white with buttons only (the X4).
const GRAY: DeviceInfo = device(PixelFormat::Gray4, true);
const MONO: DeviceInfo = device(PixelFormat::Mono1, false);

/// The page drawn on: a little larger than the largest place, so that
/// there is canvas round every place.
const PAGE: (u16, u16) = (492, 800);
/// Where places begin on the page.
const AT: (u16, u16) = (11, 13);

/// The places widgets get on a home screen of 480 × 800: from a cell of
/// the densest grid (six columns, twelve rows) to the whole screen.
const SIZES: [(u16, u16); 10] = [
    (74, 56),
    (110, 95),
    (74, 400),
    (230, 95),
    (110, 200),
    (230, 200),
    (470, 56),
    (470, 95),
    (470, 420),
    (470, 760),
];

/// The smallest of all: a cell of six columns and twelve rows on a
/// screen turned on its side.
const SMALLEST: (u16, u16) = (74, 34);

/// Pixels a widget may draw beyond its place: the edge of a glyph or of
/// a mark, within the gap the grid leaves round each place.
const TOLERANCE: u16 = 2;

/// Seconds since 1970 (UTC) of a date and time.
fn unix(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> i64 {
    // Days from civil (proleptic Gregorian), as the widgets reckon.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + hour * 3600 + minute * 60
}

/// Awkward times: UTC seconds and the offset of the place, in minutes.
fn times() -> Vec<(&'static str, Option<(i64, i16)>)> {
    vec![
        ("an ordinary morning", Some((unix(2026, 10, 14, 10, 41), 0))),
        ("new year in Berlin", Some((unix(2025, 12, 31, 23, 0), 60))),
        (
            "the last minute of the year",
            Some((unix(2026, 12, 31, 23, 59), 0)),
        ),
        ("a leap day at noon", Some((unix(2028, 2, 29, 12, 0), 0))),
        ("midnight in India", Some((unix(2026, 3, 1, 18, 30), 330))),
        (
            "late evening in Honolulu",
            Some((unix(2026, 6, 22, 9, 59), -600)),
        ),
        ("before 1970 (a clock never set)", Some((0, -60))),
        (
            "the end of the century",
            Some((unix(2099, 12, 31, 23, 59), 0)),
        ),
        ("no clock", None),
    ]
}

struct NoKeys;

impl paperpaw_sdk::keys::KeyBindings for NoKeys {
    fn bound(&self, _: &[&str], _: &str) -> Option<Id> {
        None
    }
}

/// What an `Env` is made of: the widget pack, the default fonts and the
/// kernel's translations registered, a storage in memory.
struct Harness {
    device: &'static DeviceInfo,
    settings: Settings,
    i18n: Translator,
    fonts: Fonts,
    formats: Formats,
    hyphenation: Hyphenation,
    menus: Menus,
    actions: Actions,
    plugins: Plugins,
    status: SystemStatus,
    usage: paperpaw_sdk::usage::Usage,
    plugin_usage: RefCell<Vec<paperpaw_sdk::usage::PluginUsage>>,
    status_items: StatusItems,
    text_filters: paperpaw_sdk::filters::TextFilters,
    decorators: paperpaw_sdk::decorations::Decorators,
    page_layers: paperpaw_sdk::layers::PageLayers,
    lookup: paperpaw_sdk::lookup::LookupProviders,
    readers: paperpaw_sdk::readers::Readers,
    widgets: Widgets,
    setup: paperpaw_sdk::setup::SetupSteps,
    services: paperpaw_sdk::services::Services,
    secrets: paperpaw_sdk::secrets::Secrets,
    storage: MemoryStorage,
    /// The time since start, in milliseconds.
    now: Cell<u64>,
}

impl Harness {
    fn new(device: &'static DeviceInfo, storage: MemoryStorage) -> Self {
        Self::with_fonts(device, storage, false)
    }

    /// The harness with the default fonts as bitmap packs (the same
    /// pixels whichever features the fonts plugin is built with), or
    /// with its scalable fonts (`scalable`).
    fn with_fonts(device: &'static DeviceInfo, storage: MemoryStorage, scalable: bool) -> Self {
        let mut h = Harness {
            device,
            settings: Settings::new(),
            i18n: Translator::new(),
            fonts: Fonts::new(64 * 1024),
            formats: Formats::new(),
            hyphenation: Hyphenation::new(),
            menus: Menus::new(),
            actions: Actions::new(),
            plugins: Plugins::new(),
            status: SystemStatus::new(None, Some(Battery::new(64, false, None))),
            usage: Default::default(),
            plugin_usage: Default::default(),
            status_items: StatusItems::new(),
            text_filters: Default::default(),
            decorators: Default::default(),
            page_layers: Default::default(),
            lookup: Default::default(),
            readers: Default::default(),
            widgets: Widgets::new(),
            setup: Default::default(),
            services: Default::default(),
            secrets: Default::default(),
            storage,
            now: Cell::new(1000),
        };
        // The look of the UI, as the kernel defines it.
        for (id, value) in [
            (core_settings::UI_SIZE.as_str(), 12),
            (core_settings::UI_ROW.as_str(), 160),
            (core_settings::UI_MARGIN.as_str(), 4),
            (core_settings::UI_LINES.as_str(), 2),
            (core_settings::BATTERY_LOW.as_str(), 15),
        ] {
            h.define(
                id,
                SettingKind::Int { min: 0, max: 400 },
                SettingValue::Int(value),
            );
        }
        for (id, value) in [
            (core_settings::CLOCK_FORMAT.as_str(), "core.clock.24h"),
            (core_settings::WEEK_START.as_str(), "core.week.auto"),
            ("core.logo", ""),
        ] {
            h.define(
                id,
                SettingKind::Text,
                SettingValue::Text(Cow::Borrowed(value)),
            );
        }
        h.i18n.add(
            Catalog::parse(include_str!("../../../crates/paperpaw-core/i18n/en.ini")).unwrap(),
        );
        h.i18n.add(
            Catalog::parse(include_str!("../../../crates/paperpaw-core/i18n/de.ini")).unwrap(),
        );
        let mut r = Registrar::new(Id::new_static("fonts-default"));
        if scalable {
            paperpaw_plugin_fonts_default::DefaultFonts.register(&mut r)
        } else {
            paperpaw_plugin_fonts_default::DefaultFontPacks.register(&mut r)
        }
        .unwrap();
        h.apply(r);
        let mut r = Registrar::new(paperpaw_plugin_widgets::PLUGIN);
        WidgetPack.register(&mut r).unwrap();
        h.apply(r);
        h.i18n.set_language("en");
        h.at(Some((unix(2026, 10, 14, 10, 41), 0)));
        h
    }

    fn define(&mut self, id: &str, kind: SettingKind, value: SettingValue) {
        self.settings
            .define(SettingSpec::new(
                Id::parse(id).unwrap(),
                "x",
                kind,
                value,
                paperpaw_sdk::builtin::groups::OTHER,
            ))
            .unwrap();
    }

    fn apply(&mut self, r: Registrar) {
        for contribution in r.into_contributions() {
            match contribution {
                Contribution::Setting(spec) => self.settings.define(*spec).unwrap(),
                Contribution::FixedSettings(list) => {
                    for at in 0..list.len() {
                        self.settings.define_fixed(list, at).unwrap();
                    }
                }
                Contribution::SettingGroup(group) => self.settings.add_group(group).unwrap(),
                Contribution::Widget(widget) => self.widgets.add(*widget),
                Contribution::Translations(text) => {
                    self.i18n.add(Catalog::parse(text).unwrap());
                }
                Contribution::Font(font) => self.fonts.add(*font).unwrap(),
                Contribution::FontBinding {
                    role,
                    font,
                    priority,
                    ..
                } => self.fonts.propose(role, font, priority),
                _ => {}
            }
        }
    }

    /// The clock at `time` (UTC seconds, offset in minutes), or none.
    fn at(&mut self, time: Option<(i64, i16)>) {
        self.status.clock = time
            .map(|(unix, offset)| WallClock::new(Instant(1000), unix * 1000, Some(offset.into())));
    }

    fn set(&mut self, id: &str, value: &str) {
        self.settings
            .set(id, SettingValue::Text(Cow::Owned(value.to_string())))
            .unwrap();
    }

    /// What the device can do: touch where it has it, black and white
    /// alone where its panel has no grays.
    fn caps(&self) -> Capabilities {
        let mut caps = Capabilities::default();
        caps.touch = self.device.input.touch;
        caps.black_and_white = self.device.display.format == PixelFormat::Mono1;
        caps
    }

    fn env(&self) -> Env<'_> {
        Env::from(EnvParts {
            device: self.device,
            now: Instant(self.now.get()),
            random: paperpaw_sdk::random::Random::NONE,
            memory: paperpaw_sdk::memory::Memory::NONE,
            clock: paperpaw_sdk::time::Clock::NONE,
            started: paperpaw_sdk::power::Start::Unknown,
            panel: paperpaw_sdk::realtime::PanelInfo::default(),
            keys_held: &[],
            room: paperpaw_sdk::realtime::Room::default(),
            metrics: Metrics::new(self.device.display.dpi),
            settings: &self.settings,
            i18n: &self.i18n,
            fonts: &self.fonts,
            formats: &self.formats,
            hyphenation: &self.hyphenation,
            menus: &self.menus,
            actions: &self.actions,
            plugins: &self.plugins,
            status: &self.status,
            usage: &self.usage,
            plugin_usage: &self.plugin_usage,
            status_items: &self.status_items,
            text_filters: &self.text_filters,
            decorators: &self.decorators,
            page_layers: &self.page_layers,
            lookup: &self.lookup,
            readers: &self.readers,
            stylesheets: &[],
            book_data: &[],
            book_fields: &[],
            book_badges: &[],
            widgets: &self.widgets,
            setup: &self.setup,
            services: &self.services,
            keys: &NoKeys,
            look: paperpaw_sdk::ui::look::Look::none(),
            caps: self.caps(),
            wifi: None,
            bluetooth: None,
            firmware: None,
            native: None,
            power: paperpaw_sdk::power::Power::normal(),
            work: paperpaw_sdk::work::WorkStates::none(),
            reader: true,
            clipboard: false,
            last_input: paperpaw_sdk::keys::InputMode::at_start(&self.device.input),
            inputs: &[],
            content: paperpaw_platform::Rect::default(),
            stack: &[],
            slots: &[],
            screens: paperpaw_sdk::ScreenList::default(),
            font_dirs: &[],
            catalog_device: None,
            secrets: &self.secrets,
            running: &paperpaw_sdk::secrets::Anyone,
            storage: &self.storage,
        })
    }

    /// The pack's widgets, those of `category` (all for `None`).
    fn ids(&self, category: Option<&str>) -> Vec<String> {
        self.widgets
            .iter()
            .map(|(id, _)| id.as_str().to_string())
            .filter(|id| category.is_none_or(|c| self.widgets.category(id) == Some(c)))
            .collect()
    }
}

/// The colours a page is cleared to before a widget is drawn on it: a
/// mid gray where there are grays (so that both ink and paper drawn
/// outside show), white and black where there are none.
fn sentinels(h: &Harness) -> Vec<Color> {
    if h.device.display.format == PixelFormat::Mono1 {
        vec![Color::WHITE, Color::BLACK]
    } else {
        vec![Color::gray(0x77)]
    }
}

/// Draws widget `id` into `area` of a page cleared to `sentinel`: how
/// many pixels inside `area` it changed; what it drew outside (or where
/// a target of it lies outside) as the problem.
fn draw(h: &Harness, id: &str, area: Rect, sentinel: Color) -> Result<usize, String> {
    let env = h.env();
    let mut widget = env.widgets().make(id, &env).ok_or("no such widget")?;
    let _ = widget.height(&env, area.width);
    let _ = widget.min_height(&env, area.width);
    widget.place(&env, area);
    let mut fb = Framebuffer::new(PAGE.0, PAGE.1, h.device.display.format).unwrap();
    fb.canvas().clear(sentinel);
    let before = fb.view().data.to_vec();
    let paper = fb.view().pixel(0, 0).luma();
    widget.draw(&mut fb.canvas(), &env, area);
    let view = fb.view();
    let (top, bottom) = (area.y, area.y + area.height);
    let (left, right) = (area.x, area.x + area.width);
    let inside = |x: u16, y: u16| (left..right).contains(&x) && (top..bottom).contains(&y);
    // The pixels changed outside: how many, their bounds.
    let mut outside = 0usize;
    let mut bounds = (u16::MAX, u16::MAX, 0u16, 0u16);
    let mut drawn = 0usize;
    for y in 0..PAGE.1 {
        let row = usize::from(y) * view.stride..(usize::from(y) + 1) * view.stride;
        if !(top..bottom).contains(&y) && view.data[row.clone()] == before[row] {
            continue;
        }
        for x in 0..PAGE.0 {
            if view.pixel(x, y).luma() == paper {
                continue;
            }
            if inside(x, y) {
                drawn += 1;
            } else {
                outside += 1;
                bounds = (
                    bounds.0.min(x),
                    bounds.1.min(y),
                    bounds.2.max(x),
                    bounds.3.max(y),
                );
            }
        }
    }
    let beyond = [
        left.saturating_sub(bounds.0),
        top.saturating_sub(bounds.1),
        (bounds.2 + 1).saturating_sub(right),
        (bounds.3 + 1).saturating_sub(bottom),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    if outside > 0 && beyond > TOLERANCE {
        return Err(format!(
            "drew {outside} px outside, up to {beyond} px beyond (x {}..={}, y {}..={})",
            bounds.0, bounds.2, bounds.1, bounds.3
        ));
    }
    for target in widget.targets(&env, area) {
        let t = target.area;
        let within =
            t.x >= left && t.y >= top && t.x + t.width <= right && t.y + t.height <= bottom;
        if !within {
            return Err(format!("a target at {t:?} lies outside"));
        }
    }
    Ok(drawn)
}

thread_local! {
    /// Where the last panic on this thread was, and what it said.
    static PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Runs `f`, a panic in it as an error saying where it was and why.
fn caught<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_default();
            let at = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_default();
            let noted = PANIC.with(|p| {
                *p.borrow_mut() = format!("{message} at {at}");
                // Panics of the tests themselves are told as usual.
                !at.contains("tests")
            });
            if !noted {
                default(info);
            }
        }));
    });
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| PANIC.with(|p| p.borrow().clone()))
}

/// A problem found: the widget, its place's size, what was set or how
/// it was drawn, and what went wrong.
struct Problem {
    id: String,
    size: (u16, u16),
    what: String,
    text: String,
}

/// Problems found, gathered so that a test reports all at once.
#[derive(Default)]
struct Problems(Vec<Problem>);

impl Problems {
    /// Draws widget `id` at every size on every sentinel, noting where it
    /// draws outside its place, panics, or (with `must_draw`, at the
    /// larger sizes, on paper) draws nothing.
    fn check(&mut self, h: &Harness, id: &str, sizes: &[(u16, u16)], what: &str, must_draw: bool) {
        self.check_as(h, id, sizes, what, must_draw, true);
    }

    /// [`Problems::check`], noting only panics without `inside`.
    fn check_as(
        &mut self,
        h: &Harness,
        id: &str,
        sizes: &[(u16, u16)],
        what: &str,
        must_draw: bool,
        inside: bool,
    ) {
        for &(w, hgt) in sizes {
            let area = Rect::new(AT.0, AT.1, w, hgt);
            for (n, sentinel) in sentinels(h).into_iter().enumerate() {
                let text = match caught(|| draw(h, id, area, sentinel)) {
                    Err(panic) => format!("panicked: {panic}"),
                    Ok(Err(problem)) if inside => problem,
                    Ok(Ok(0)) if must_draw && n == 0 && w >= 110 && hgt >= 95 => {
                        String::from("drew nothing")
                    }
                    Ok(_) => continue,
                };
                self.0.push(Problem {
                    id: id.to_string(),
                    size: (w, hgt),
                    what: what.to_string(),
                    text: format!("on {:?}: {text}", h.device.display.format),
                });
            }
        }
    }

    /// Fails with every problem found.
    fn assert_all(self) {
        let lines: Vec<String> = self
            .0
            .iter()
            .map(|p| {
                let what: String = p.what.chars().take(80).collect();
                format!("{} {}x{} ({what}) {}", p.id, p.size.0, p.size.1, p.text)
            })
            .collect();
        assert!(
            lines.is_empty(),
            "{} problems:\n{}",
            lines.len(),
            lines.join("\n")
        );
    }
}

#[test]
fn the_pack_has_its_widgets() {
    let h = Harness::new(&GRAY, MemoryStorage::new());
    let ids = h.ids(None);
    for id in [
        "widgets.analogue-clock",
        "widgets.word-clock",
        "widgets.tile-clock",
        "widgets.world-clock",
        "widgets.calendar",
        "widgets.year-progress",
        "widgets.countdown",
        "widgets.moon",
        "widgets.day",
        "widgets.quote",
        "widgets.picture",
        "widgets.note",
        "widgets.todo",
        "widgets.greeting",
        "widgets.logo",
        "widgets.next",
        "widgets.highlight",
        "widgets.series",
        "widgets.battery",
        "widgets.reading",
    ] {
        assert!(ids.iter().any(|i| i == id), "{id} in {ids:?}");
    }
    for id in &ids {
        let kind = h.widgets.category(id).unwrap();
        assert!(categories::ALL.contains(&kind), "{id}: {kind}");
    }
}

#[test]
fn time_widgets_stay_in_their_place_at_any_time() {
    let mut problems = Problems::default();
    for device in [&GRAY, &MONO] {
        let mut h = Harness::new(device, MemoryStorage::new());
        let ids = h.ids(Some(categories::TIME));
        assert!(ids.len() >= 9, "{ids:?}");
        for (what, time) in times() {
            h.at(time);
            for id in &ids {
                problems.check(&h, id, &SIZES, what, time.is_some());
            }
        }
    }
    problems.assert_all();
}

#[test]
fn time_widgets_in_twelve_hours_from_sunday_and_in_german() {
    let mut problems = Problems::default();
    let mut h = Harness::new(&GRAY, MemoryStorage::new());
    h.set(core_settings::CLOCK_FORMAT.as_str(), "core.clock.12h");
    h.set(core_settings::WEEK_START.as_str(), "core.week.sunday");
    h.i18n.set_language("de");
    let ids = h.ids(Some(categories::TIME));
    for (what, time) in times().into_iter().take(4) {
        h.at(time);
        for id in &ids {
            problems.check(&h, id, &SIZES, what, true);
        }
    }
    problems.assert_all();
}

/// A library of one book being read, with a cover and a highlight, and
/// the widgets' own files: to-dos, own quotes, where the book is read.
fn filled() -> MemoryStorage {
    let cover = ".paperpaw/covers/one.gray";
    let mut gray = b"PPGI".to_vec();
    gray.extend_from_slice(&60u32.to_le_bytes());
    gray.extend_from_slice(&90u32.to_le_bytes());
    gray.extend((0..60 * 90).map(|n| (n % 251) as u8));
    let storage = MemoryStorage::new()
        .with(cover, gray)
        .with("books/one.epub", "a book")
        .with(
            ".paperpaw/plugins/widgets/todo.txt",
            "Buy bread\nx 2026-10-14 Water the plants\nCall Anna\nRead chapter 3\n\
             A to-do with a very long text that cannot fit in any row of the widget at all\n\
             Six\nSeven\n",
        )
        .with(
            ".paperpaw/plugins/widgets/quotes.txt",
            "A quote of my own | Me\n",
        );
    let book = paperpaw_sdk::library::Book::default()
        .size(6)
        .indexed(true)
        .title("The One Book".to_string())
        .author("Ann Author".to_string())
        .series("Tales".to_string())
        .series_index("1".to_string())
        .cover(paperpaw_sdk::document::ImageRef {
            path: cover.to_string(),
            width: 60,
            height: 90,
        })
        .progress(40);
    paperpaw_sdk::library::save(&storage, "books/one.epub", &book).unwrap();
    paperpaw_sdk::library::catalog::write(
        &storage,
        &[paperpaw_sdk::library::catalog::Record::of(
            "books/one.epub",
            &book,
        )],
    )
    .unwrap();
    let _ = storage.write(".paperpaw/library/opened.txt", b"books/one.epub");
    storage
        .write(
            ".paperpaw/plugins/widgets/reading.ini",
            b"[reading]\npath = books/one.epub\npage = 34\npages = 210\nchapter = One\n\
              chapter-left = 4\nseconds = 600\nturned = 20\n",
        )
        .unwrap();
    storage
}

#[test]
fn the_other_widgets_stay_in_their_place_empty_and_filled() {
    let mut problems = Problems::default();
    for device in [&GRAY, &MONO] {
        for (what, storage) in [("empty", MemoryStorage::new()), ("filled", filled())] {
            let h = Harness::new(device, storage);
            for id in h.ids(None) {
                if h.widgets.category(&id) != Some(categories::TIME) {
                    problems.check(&h, &id, &SIZES, what, true);
                }
            }
        }
    }
    problems.assert_all();
}

/// The values to try of a setting: each of a choice, both of a switch,
/// the ends of a number, texts of every kind.
fn values(spec: &SettingSpec) -> Vec<SettingValue> {
    match &spec.kind {
        SettingKind::Bool => vec![SettingValue::Bool(true), SettingValue::Bool(false)],
        SettingKind::Int { min, max } => vec![SettingValue::Int(*min), SettingValue::Int(*max)],
        SettingKind::Choice(choices) => choices
            .iter()
            .map(|c| SettingValue::Text(Cow::Owned(c.to_string())))
            .collect(),
        SettingKind::Text => hostile_texts()
            .into_iter()
            .map(|t| SettingValue::Text(Cow::Owned(t)))
            .collect(),
        _ => Vec::new(),
    }
}

/// Texts a reader could type, or a broken file could hold.
fn hostile_texts() -> Vec<String> {
    vec![
        String::new(),
        " ".into(),
        "\n\n\n".into(),
        "\t".into(),
        "A".into(),
        "Supercalifragilisticexpialidocious".repeat(30),
        "word ".repeat(2000),
        "Grüße aus Köln, ½ ✓ 日本語のテキスト עברית العربية 🐾🐾🐾".into(),
        "e\u{301}\u{301}\u{301}\u{301}\u{301}".into(),
        "\u{200b}\u{200d}\u{feff}\u{0}".into(),
        "{hour} {title} {count} %s %d".into(),
        "../../../etc/passwd".into(),
        "/absolute/path/".into(),
        "C:\\Windows\\..\\x".into(),
        "2026-02-30".into(),
        "29.02.".into(),
        "9999-12-31".into(),
        "-1".into(),
    ]
}

#[test]
fn every_value_of_every_setting_stays_in_its_place() {
    let mut problems = Problems::default();
    let sizes = [(110, 95), (230, 200), (470, 420)];
    for device in [&GRAY, &MONO] {
        let mut h = Harness::new(device, filled());
        for id in h.ids(None) {
            let specs: Vec<SettingSpec> = h
                .settings
                .specs()
                .filter(|s| s.group.as_str() == id)
                .cloned()
                .collect();
            assert!(!specs.is_empty(), "{id} has settings");
            for spec in specs {
                for value in values(&spec) {
                    h.settings.set(spec.id.as_str(), value.clone()).unwrap();
                    let what = format!("{} = {value:?}", spec.id);
                    let what: String = what.chars().take(80).collect();
                    // An empty text may leave nothing to draw.
                    let must = !matches!(spec.kind, SettingKind::Text);
                    problems.check(&h, &id, &sizes, &what, must);
                }
                h.settings.reset(spec.id.as_str()).unwrap();
            }
        }
    }
    problems.assert_all();
}

#[test]
fn widgets_in_places_smaller_than_any_cell_do_not_panic() {
    let mut problems = Problems::default();
    let tiny = [(1, 1), (8, 8), (24, 16), (16, 60), (60, 12)];
    for device in [&GRAY, &MONO] {
        let h = Harness::new(device, filled());
        for id in h.ids(None) {
            problems.check_as(&h, &id, &tiny, "tiny", false, false);
        }
    }
    problems.assert_all();
}

/// The widgets' own files with what no widget wrote: bytes that are no
/// text, lines without end, numbers too large, dates that are none.
fn junk() -> MemoryStorage {
    let dir = ".paperpaw/plugins/widgets";
    let long_line = "x".repeat(100_000);
    let many = "item\n".repeat(5_000);
    MemoryStorage::new()
        .with(
            &format!("{dir}/todo.txt"),
            format!(
                "\u{feff}x\nx \nx 2026-13-40 bad date\nx 2026-10-14\n\u{0}\u{1}\n{long_line}\n{many}"
            ),
        )
        .with(
            &format!("{dir}/quotes.txt"),
            [
                b"|\n||||\n# only a comment\n\xff\xfe\xfd not utf-8 | \xc3\n".to_vec(),
                long_line.clone().into_bytes(),
                b"\n\"\" | nobody\n\xe2\x80\x9c\xe2\x80\x9d|\n".to_vec(),
            ]
            .concat(),
        )
        .with(&format!("{dir}/quote.txt"), "-9223372036854775808 4294967295\n")
        .with(&format!("{dir}/picture.txt"), "99999999999999999999999 -1\n")
        .with(&format!("{dir}/picture.stamp"), vec![0xffu8; 3000])
        .with(&format!("{dir}/picture.pixels"), b"PPGI\xff\xff\xff\xff\xff\xff\xff\xff".to_vec())
        .with(
            &format!("{dir}/reading.ini"),
            "[reading]\npath = ../../../etc\npage = 4294967295\npages = 0\n\
             chapter-left = 4294967295\nseconds = 4294967295\nturned = 1\n",
        )
        .with(&format!("{dir}/seen.bin"), vec![0xabu8; 13])
        .with(&format!("{dir}/newest.txt"), "\n\n../x\n/abs\n\\\\server\\share\n")
        .with("Pictures/.hidden.png", "not a picture")
}

#[test]
fn hostile_files_where_the_widgets_keep_theirs_never_panic() {
    let mut problems = Problems::default();
    let sizes = [SMALLEST, (110, 95), (470, 420)];
    for device in [&GRAY, &MONO] {
        let mut h = Harness::new(device, junk());
        h.set("widgets.quote.source", "widgets.quote.source.own");
        h.set("widgets.quote.change", "widgets.change.shown");
        for id in h.ids(None) {
            problems.check_as(&h, &id, &sizes, "junk files", false, false);
        }
    }
    problems.assert_all();
}

/// The CRC of PNG chunks.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A PNG of nothing but its header, saying it is `width` × `height`.
fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = b"IHDR".to_vec();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]);
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(&ihdr);
    png.extend_from_slice(&crc32(&ihdr).to_be_bytes());
    png
}

/// A real PNG, `side` pixels square, in `storage` at `path`.
fn real_png(storage: &MemoryStorage, path: &str, side: u32) -> Vec<u8> {
    let mut png = paperpaw_sdk::image::PngWriter::new(storage, path, side, side).unwrap();
    for y in 0..side {
        let row: Vec<u8> = (0..side).map(|x| ((x + y) * 7 % 256) as u8).collect();
        png.row(storage, path, &row).unwrap();
    }
    paperpaw_sdk::read_file(storage, path, 1 << 20).unwrap()
}

#[test]
fn pictures_that_cannot_be_read_never_panic() {
    let scratch = MemoryStorage::new();
    let good = real_png(&scratch, "good.png", 40);
    let mut cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty.png", Vec::new()),
        ("garbage.jpg", b"this is not a picture at all".to_vec()),
        (
            "header-only.jpg",
            b"\xff\xd8\xff\xe0\x00\x10JFIF\x00".to_vec(),
        ),
        ("header-only.gif", b"GIF89a\x10\x00\x10\x00".to_vec()),
        ("huge.png", png_header(100_000, 100_000)),
        ("zero.png", png_header(0, 0)),
        ("truncated.png", good[..good.len() / 2].to_vec()),
    ];
    // A good PNG with its middle bytes broken.
    let mut broken = good.clone();
    let middle = broken.len() / 2;
    for b in &mut broken[middle..middle + 20] {
        *b ^= 0x5a;
    }
    cases.push(("broken.png", broken));
    let mut problems = Problems::default();
    for device in [&GRAY, &MONO] {
        for (name, bytes) in &cases {
            // Each picture alone in the folder, so that it is the one shown.
            let storage = MemoryStorage::new().with(&format!("Pictures/{name}"), bytes.clone());
            let mut h = Harness::new(device, storage);
            for fit in ["widgets.picture.fit.fill", "widgets.picture.fit.whole"] {
                h.set("widgets.picture.fit", fit);
                h.settings
                    .set("widgets.picture.caption", SettingValue::Bool(true))
                    .unwrap();
                problems.check(&h, "widgets.picture", &SIZES, name, true);
            }
        }
    }
    problems.assert_all();
}

#[test]
fn a_good_picture_is_drawn_inside_its_place() {
    let storage = MemoryStorage::new();
    real_png(&storage, "Pictures/holiday_by_the_sea.png", 64);
    let mut problems = Problems::default();
    let mut h = Harness::new(&GRAY, storage);
    h.settings
        .set("widgets.picture.caption", SettingValue::Bool(true))
        .unwrap();
    for fit in ["widgets.picture.fit.fill", "widgets.picture.fit.whole"] {
        h.set("widgets.picture.fit", fit);
        problems.check(&h, "widgets.picture", &SIZES, fit, true);
    }
    problems.assert_all();
    // Decoded once, the picture's pixels are kept for the next time.
    assert!(h.storage.exists(".paperpaw/plugins/widgets/picture.pixels"));
}

#[test]
fn folders_of_pictures_that_are_no_folders_never_panic() {
    let storage = MemoryStorage::new();
    real_png(&storage, "Pictures/a.png", 16);
    let mut problems = Problems::default();
    let mut h = Harness::new(&GRAY, storage);
    for folder in [
        "",
        "/",
        "//",
        "Pictures/a.png",
        "../Pictures",
        "../../..",
        "\\Pictures",
        "Bilder 🖼",
        &"deep/".repeat(200),
    ] {
        h.set("widgets.picture.folder", folder);
        problems.check(
            &h,
            "widgets.picture",
            &[(110, 95), (470, 420)],
            folder,
            true,
        );
    }
    problems.assert_all();
}

#[test]
fn a_logo_that_cannot_be_read_never_panics() {
    let mut problems = Problems::default();
    let storage = MemoryStorage::new()
        .with("logo.png", png_header(50_000, 50_000))
        .with("garbage.png", "nothing");
    let mut h = Harness::new(&GRAY, storage);
    for logo in ["logo.png", "garbage.png", "missing.png", "../../x.png", ""] {
        h.set("core.logo", logo);
        problems.check(&h, "widgets.logo", &SIZES, logo, true);
    }
    problems.assert_all();
}

// ---- Problems once known ---------------------------------------------------
//
// Each showed a widget drawing outside its place in a way of its own
// (all fixed now, kept so they stay fixed), on the grays panel unless
// the problem was of black and white.

/// Draws widget `id` with `settings` at `sizes`, on `device`, with
/// `storage`, at an ordinary morning: every problem found.
fn problems_of(
    device: &'static DeviceInfo,
    storage: MemoryStorage,
    id: &str,
    settings: &[(&str, &str)],
    sizes: &[(u16, u16)],
) -> Problems {
    let mut h = Harness::new(device, storage);
    for (setting, value) in settings {
        h.set(setting, value);
    }
    let mut problems = Problems::default();
    problems.check(&h, id, sizes, "as set", true);
    problems
}

#[test]
fn the_world_clock_stays_in_one_cell_and_in_a_row() {
    let mut h = Harness::new(&GRAY, MemoryStorage::new());
    let mut problems = Problems::default();
    for (what, time) in times().into_iter().take(2) {
        h.at(time);
        let sizes = [(110, 95), (230, 95), (470, 95), (470, 56)];
        problems.check(&h, "widgets.world-clock", &sizes, what, true);
    }
    problems.assert_all();
}

#[test]
fn the_word_clock_stays_in_one_cell() {
    problems_of(
        &GRAY,
        MemoryStorage::new(),
        "widgets.word-clock",
        &[],
        &[(110, 95), (74, 56)],
    )
    .assert_all();
}

#[test]
fn a_countdown_with_a_title_stays_in_one_cell() {
    let settings = [
        ("widgets.countdown.date", "2026-12-24"),
        ("widgets.countdown.title", "Holiday"),
    ];
    problems_of(
        &GRAY,
        MemoryStorage::new(),
        "widgets.countdown",
        &settings,
        &[(110, 95)],
    )
    .assert_all();
    problems_of(
        &GRAY,
        MemoryStorage::new(),
        "widgets.countdown",
        &[],
        &[(470, 56)],
    )
    .assert_all();
}

#[test]
fn a_series_without_numbers_stays_in_its_place() {
    let mut h = Harness::new(&GRAY, filled());
    h.settings
        .set("widgets.series.numbers", SettingValue::Bool(false))
        .unwrap();
    let mut problems = Problems::default();
    let sizes = [(110, 95), (230, 200), (470, 420)];
    problems.check(&h, "widgets.series", &sizes, "no numbers", true);
    problems.assert_all();
}

#[test]
fn a_quote_in_a_set_size_stays_in_one_cell() {
    let settings = [("widgets.quote.size", "core.size.large")];
    problems_of(
        &GRAY,
        MemoryStorage::new(),
        "widgets.quote",
        &settings,
        &[(110, 95)],
    )
    .assert_all();
}

#[test]
fn a_quote_set_right_stays_in_its_place_in_black_and_white() {
    // Another quote each time it shows: every quote shipped in turn, a
    // minute apart (made within one, it counts as the same showing).
    let mut h = Harness::new(&MONO, filled());
    h.set("widgets.quote.align", "core.align.right");
    h.set("widgets.quote.change", "widgets.change.shown");
    let mut problems = Problems::default();
    for n in 0..60 {
        h.now.set(1000 + n * 60_000);
        problems.check(
            &h,
            "widgets.quote",
            &[(470, 420)],
            &format!("turn {n}"),
            true,
        );
    }
    problems.assert_all();
}

#[test]
fn the_day_as_a_sheet_stays_in_one_cell() {
    let settings = [("widgets.day.style", "widgets.day.style.sheet")];
    problems_of(
        &GRAY,
        MemoryStorage::new(),
        "widgets.day",
        &settings,
        &[(110, 95)],
    )
    .assert_all();
}

#[test]
fn the_year_stays_in_a_low_row() {
    problems_of(
        &GRAY,
        MemoryStorage::new(),
        "widgets.year-progress",
        &[],
        &[(470, 56), (74, 56)],
    )
    .assert_all();
}

#[test]
fn the_year_in_a_frame_stays_in_one_cell() {
    let settings = [("widgets.year-progress.frame", "core.frame.line")];
    problems_of(
        &GRAY,
        MemoryStorage::new(),
        "widgets.year-progress",
        &settings,
        &[(110, 95)],
    )
    .assert_all();
}

/// The ring is left out where it has no room (its subtractions used to
/// underflow).
#[test]
fn the_year_as_a_ring_in_one_cell_does_not_panic() {
    let mut h = Harness::new(&GRAY, MemoryStorage::new());
    h.set("widgets.year-progress.style", "widgets.year.ring");
    let mut problems = Problems::default();
    let sizes = [(110, 95), SMALLEST];
    problems.check_as(&h, "widgets.year-progress", &sizes, "ring", false, false);
    problems.assert_all();
}

#[test]
fn the_calendar_stays_in_narrow_and_low_places() {
    let sizes = [(74, 56), (74, 400), (470, 56)];
    problems_of(&GRAY, MemoryStorage::new(), "widgets.calendar", &[], &sizes).assert_all();
}

/// Where both months have no room, this month alone shows (both used to
/// draw 5 px outside).
#[test]
fn the_calendar_with_the_next_month_stays_in_one_cell() {
    let mut h = Harness::new(&GRAY, MemoryStorage::new());
    h.settings
        .set("widgets.calendar.next", SettingValue::Bool(true))
        .unwrap();
    let mut problems = Problems::default();
    problems.check(&h, "widgets.calendar", &[(110, 95)], "next month", true);
    problems.assert_all();
}

#[test]
fn the_greeting_stays_in_a_low_row() {
    problems_of(
        &GRAY,
        MemoryStorage::new(),
        "widgets.greeting",
        &[],
        &[(470, 56), (74, 56)],
    )
    .assert_all();
}

/// A picture folder of thousands of words: its lines are counted
/// without overflow (`lines * line height` used to overflow `u16`).
#[test]
fn a_long_folder_name_does_not_panic() {
    let folder = "word ".repeat(2000);
    let settings = [("widgets.picture.folder", folder.as_str())];
    problems_of(
        &GRAY,
        MemoryStorage::new(),
        "widgets.picture",
        &settings,
        &[(110, 95)],
    )
    .assert_all();
}

/// Its parts are measured without underflow (`height - gaps` used to
/// underflow in a frame in the smallest cell).
#[test]
fn the_world_clock_in_the_smallest_cell_with_a_frame_does_not_panic() {
    let mut h = Harness::new(&GRAY, MemoryStorage::new());
    h.set("widgets.world-clock.frame", "core.frame.line");
    let mut problems = Problems::default();
    problems.check_as(
        &h,
        "widgets.world-clock",
        &[SMALLEST],
        "framed",
        false,
        false,
    );
    problems.assert_all();
}

#[test]
fn widgets_stay_in_the_smallest_cell() {
    let mut problems = Problems::default();
    for storage in [MemoryStorage::new(), filled()] {
        let h = Harness::new(&GRAY, storage);
        for id in h.ids(None) {
            problems.check(&h, &id, &[SMALLEST], "defaults", false);
        }
    }
    problems.assert_all();
}

/// With scalable fonts, words are measured as nothing at the tiniest
/// sizes: in a place just wide enough to put the moon beside its text
/// (half again as wide as high) but narrower than the moon and the gap
/// after it, the text "fits" in no width at all, and the room left for
/// it was reckoned as `area.width - side - gap`, which underflowed.
#[test]
fn the_moon_beside_its_text_in_a_small_place_does_not_panic() {
    let h = Harness::with_fonts(&GRAY, MemoryStorage::new(), true);
    let mut problems = Problems::default();
    let sizes = [(24, 16), (30, 20), (45, 30)];
    problems.check_as(&h, "widgets.moon", &sizes, "scalable fonts", false, false);
    problems.assert_all();
}

#[test]
fn the_year_with_scalable_fonts_stays_in_its_place() {
    let h = Harness::with_fonts(&GRAY, MemoryStorage::new(), true);
    let mut problems = Problems::default();
    let sizes = [(230, 200)];
    problems.check(&h, "widgets.year-progress", &sizes, "scalable fonts", true);
    problems.assert_all();
}

#[test]
fn the_word_clock_in_a_large_size_with_scalable_fonts_stays_in_its_place() {
    let mut h = Harness::with_fonts(&GRAY, MemoryStorage::new(), true);
    h.set("widgets.word-clock.size", "core.size.large");
    let mut problems = Problems::default();
    problems.check(
        &h,
        "widgets.word-clock",
        &[(230, 200)],
        "scalable fonts",
        true,
    );
    problems.assert_all();
}
