//! The shipped Brazilian Portuguese catalog loaded from a compiled pack on the card.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_i18n::{Arg, Catalog, MAX_READ};
use paperpaw_sdk::builtin::fonts;

const SOURCE: &str = include_str!("../../../tools/plugin-files/languages/pt-br.ini");
const LANGUAGE: &str = "pt-BR";
const PLUGIN: &str = "language-pt-br";

fn source() -> Catalog {
    Catalog::parse(SOURCE).unwrap()
}

fn message(key: &str) -> String {
    source()
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}"))
        .to_string()
}

/// Install the compiled catalog, without loading its source into the app.
fn card() -> Shared {
    let manifest = format!(
        "[plugin]\nid = {PLUGIN}\nname = Brazilian Portuguese\nversion = 1.0.0\nformats = 4\n"
    );
    let compiled = Catalog::compile(SOURCE).unwrap();
    Shared::memory(
        &format!("[core]\nlanguage = {LANGUAGE}\n[help]\nshow = false\n"),
        &[
            (&format!("plugins/{PLUGIN}/plugin.ini"), manifest.as_bytes()),
            (&format!("plugins/{PLUGIN}/i18n/pt-br.ppcat"), &compiled),
        ],
    )
}

fn within_read_budget(app: &App) {
    let read = app.kernel.translator().read_bytes();
    assert!(read < MAX_READ, "{} read {read} bytes", app.screen());
}

#[test]
fn the_compiled_pack_translates_home_on_buttons_and_touch() {
    for device in DEVICES {
        let mut app = App::new(device, card());
        assert_eq!(app.kernel.translator().language(), LANGUAGE);
        let title = message("home.start.title");
        assert_ne!(title, "Start reading");
        assert!(app.shows(&title), "{}", app.described());
        within_read_budget(&app);
        app.open_screen(
            Id::new_static("settings.choose"),
            Params::new().with("id", "core.language"),
        );
        assert!(app.shows("Português (Brasil)"), "{}", app.described());
        within_read_budget(&app);
    }
}

#[test]
fn the_drawer_translates_its_heading_and_keeps_left_to_right_order() {
    for device in DEVICES {
        let mut app = App::new(device, card());
        app.open_screen(Id::new_static("home.drawer"), Params::new());
        let title = message("home.drawer");
        assert!(app.shows(&title), "{}", app.described());
        let library = message("core.action.library");
        assert_ne!(library, "Library");
        assert!(app.shows(&library), "{}", app.described());
        within_read_budget(&app);
        let buttons: Vec<_> = app
            .parts()
            .into_iter()
            .filter(|part| part.role == Role::Button)
            .map(|part| part.area)
            .collect();
        assert!(buttons.len() >= 2, "{buttons:?}");
        assert!(buttons[0].x < buttons[1].x, "{buttons:?}");
        assert_eq!(buttons[0].y, buttons[1].y);
    }
}

#[test]
fn the_home_clock_uses_local_weekdays_months_and_date_order() {
    let mut app = App::new(X4, card());
    app.set("home.page-1", "home.clock 0/0 4x2, home.continue 0/2 4x2");
    let clock = paperpaw_sdk::WallClock::new(Instant(app.now), 1_760_438_460_000, Some(0));
    let status = paperpaw_sdk::SystemStatus::new(Some(clock), None);
    let effects = app.kernel.set_status(Instant(app.now), status);
    app.settle(effects);
    let weekday = message("core.weekday.2");
    let month = message("core.month.10");
    assert_eq!(weekday, "terça-feira");
    assert_eq!(month, "outubro");
    let expected = app.kernel.translator().format(
        "core.date",
        &[
            ("weekday", Arg::Str(&weekday)),
            ("day", Arg::Int(14)),
            ("month", Arg::Str(&month)),
        ],
    );
    assert!(app.shows(&expected), "{}", app.described());
    within_read_budget(&app);
}

#[test]
fn the_battery_screen_uses_a_comma_in_its_voltage() {
    let mut app = App::new(X4, card());
    let battery = paperpaw_platform::Battery::new(87, false, Some(4012));
    let status = paperpaw_sdk::SystemStatus::new(None, Some(battery));
    app.now += 60_000;
    let effects = app.kernel.set_status(Instant(app.now), status);
    app.settle(effects);
    app.run(Id::new_static("battery.open"));
    assert!(app.shows("4,01 V"), "{}", app.described());
    assert!(!app.shows("4.01 V"));
    within_read_budget(&app);
}

#[test]
fn plurals_choose_singular_for_zero_and_one_and_plural_for_two_and_millions() {
    let app = App::new(X4, card());
    let catalog = source();
    for (count, form) in [(0, "one"), (1, "one"), (2, "other"), (1_000_000, "many")] {
        let key = format!("files.books.{form}");
        let expected = catalog
            .get(&key)
            .or_else(|| catalog.get("files.books.other"))
            .unwrap()
            .replace("{count}", &count.to_string());
        assert_eq!(
            app.kernel.translator().plural("files.books", count, &[]),
            expected,
            "{count}"
        );
    }
}

#[test]
fn default_bitmap_fonts_draw_every_letter_in_the_shipped_catalog() {
    let app = App::new(X4, card());
    let catalog = source();
    let letters: std::collections::BTreeSet<char> = catalog
        .keys()
        .flat_map(|key| catalog.get(key).unwrap().chars())
        .filter(|ch| ch.is_alphabetic() && !ch.is_ascii())
        .collect();
    assert!(!letters.is_empty());
    for role in [fonts::UI, fonts::READING] {
        let font = app.kernel.fonts().role(role.as_str()).unwrap();
        for &ch in &letters {
            assert!(font.has_char(ch), "{role} lacks {ch:?}");
        }
    }
}
