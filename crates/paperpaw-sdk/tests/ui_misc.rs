//! The smaller UI pieces: questions for text and folders ([`ask`]),
//! icons by name, what a screen tells about itself ([`semantics`]) as
//! the drawing helpers describe it, menus offered by what the device can
//! do, the drawn logo, and the shared look of widgets (frames, captions,
//! wrapped text) and their placements.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use std::borrow::Cow;

use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::logo::{self, Logo};
use paperpaw_sdk::replies::{FolderChosen, Typed};
use paperpaw_sdk::semantics::{self, Role};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame, TextSize, WidgetSpec, Wrapped};
use paperpaw_sdk::{
    Canvas, Capabilities, Color, Command, Env, Id, MenuEntry, MenuNeed, MenuSection, Menus, Params,
    Rect, Storage, ask, icons,
};
use proptest::prelude::*;

use support::{Device, Harness, describe, drawn_in, drawn_outside, frame};

// ---- Questions ----------------------------------------------------------

#[test]
fn questions_ask_the_keyboard_and_the_folder_chooser_with_their_reply() {
    const NAMED: Event<Typed> = Event::new("t.named");
    const PLACED: Event<FolderChosen> = Event::new("t.placed");
    let command = ask::text("Name of the shelf", &NAMED)
        .initial("Novels")
        .password()
        .with("shelf", "3")
        .context(&Params::new().with("from", "menu"))
        .command();
    let Command::OpenSlot(slot, p) = command else {
        panic!("{command:?}");
    };
    assert_eq!(slot, slots::KEYBOARD);
    assert_eq!(p.get(params::TITLE), Some("Name of the shelf"));
    assert_eq!(p.get(params::REPLY), Some("t.named"));
    assert_eq!(p.get(params::TEXT), Some("Novels"));
    assert_eq!(p.get(params::MODE), Some("password"));
    assert_eq!(p.get("shelf"), Some("3"));
    assert_eq!(p.get("from"), Some("menu"));
    // Digits only; on another device.
    let remote = ask::text("PIN", &NAMED).number().remote_command();
    let Command::OpenSlot(slot, p) = remote else {
        panic!("{remote:?}");
    };
    assert_eq!(slot, slots::REMOTE_KEYBOARD);
    assert_eq!(p.get(params::MODE), Some("number"));
    let folder = ask::folder(&PLACED)
        .start("books/")
        .with("book", "a.epub")
        .command();
    let Command::OpenSlot(slot, p) = folder else {
        panic!("{folder:?}");
    };
    assert_eq!(slot, slots::CHOOSE_FOLDER);
    assert_eq!(p.get(params::REPLY), Some("t.placed"));
    assert_eq!(p.get(params::TEXT), Some("books/"));
    assert_eq!(p.get("book"), Some("a.epub"));
}

// ---- Icons ----------------------------------------------------------------

#[test]
fn every_icon_is_found_by_its_name() {
    let names: Vec<&str> = icons::names().collect();
    assert!(names.windows(2).all(|w| w[0] < w[1]), "sorted, each once");
    for name in &names {
        assert!(!name.is_empty());
        let c = icons::char(name).unwrap_or_else(|| panic!("{name}"));
        // Past the letters, where icon fonts keep their drawings.
        assert!(c >= '\u{e000}', "{name}: {c:?}");
    }
}

/// The names and characters of Tabler Icons 3.35.0 as the table had them
/// before it was packed (`name=U+code` a line, FNV-1a): the packing
/// changes none.
#[test]
fn the_packed_table_holds_the_same_names_and_characters() {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut count = 0;
    for name in icons::names() {
        let c = icons::char(name).unwrap_or_else(|| panic!("{name}"));
        for byte in format!("{name}=U+{:04X}\n", u32::from(c)).bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        count += 1;
    }
    assert_eq!(count, 5934);
    assert_eq!(hash, 0x8a8d_e240_0cd5_330a);
}

/// A name's hash as the table keeps it in place of the name (FNV-1a, 32
/// bits; `tools/icons/subset.py`).
fn name_hash(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5, |hash: u32, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// Whether `name` is taken for an icon: its hash is an icon name's. The
/// table holds no names (#258), so that is all it can tell.
fn hashes_as_an_icon(name: &str) -> bool {
    static HASHES: std::sync::OnceLock<std::collections::BTreeSet<u32>> =
        std::sync::OnceLock::new();
    HASHES
        .get_or_init(|| icons::names().map(name_hash).collect())
        .contains(&name_hash(name))
}

/// No two icons' names hash alike, and names that are no icon's (near
/// ones, none at all, what an icon means) are not found.
#[test]
fn names_are_told_apart_by_their_hashes() {
    let hashes: std::collections::BTreeSet<u32> = icons::names().map(name_hash).collect();
    assert_eq!(hashes.len(), 5934);
    for name in [
        "",
        "no-such-icon",
        "book-",
        "books2",
        "Books",
        "arrow",
        icons::BACK,
    ] {
        assert_eq!(icons::char(name), None, "{name}");
    }
    assert_eq!(icons::char("U+F8000"), Some('\u{f8000}'));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// Any name at all is looked up without a panic: an icon's own, or
    /// none (but for one whose hash is an icon name's).
    #[test]
    fn any_name_is_looked_up_safely(name in "\\PC{0,30}") {
        let found = icons::char(&name);
        prop_assert_eq!(found.is_some(), hashes_as_an_icon(&name));
    }

    /// Names next to real ones (cut short or longer, as `a-b` is of
    /// `a-b-2`) are found exactly when their hash is an icon name's, and
    /// an icon's own name gives its own character.
    #[test]
    fn names_next_to_real_ones_are_told_apart(
        index in 0..5934usize,
        cut in 0..40usize,
        tail in "[a-z0-9-]{0,4}",
    ) {
        let real = icons::names().nth(index).unwrap();
        let name = format!("{}{tail}", &real[..cut.min(real.len())]);
        let found = icons::char(&name);
        prop_assert_eq!(found.is_some(), hashes_as_an_icon(&name));
        if icons::names().any(|n| n == name) {
            prop_assert!(found.is_some(), "{}", name);
        }
    }
}

// ---- What a screen tells ------------------------------------------------

/// A list drawn with the helpers tells its rows, each with the title and
/// value drawn in it, the focused one focused: what a screen reader or a
/// remote control reads.
#[test]
fn a_drawn_list_tells_its_rows_and_their_texts() {
    let h = Harness::new(Device::BUTTONS);
    let env = h.env();
    let style = UiStyle::new(&env);
    let rows = [
        ("Night mode", "On"),
        ("Font size", "12 pt"),
        ("Language", "English"),
    ];
    let mut list = ListView::new(Rect::new(0, 100, 480, 600), u32::from(style.row()));
    list.set_focus(1);
    let (described, _) = describe(480, 800, |canvas| {
        list.draw(
            &mut paperpaw_sdk::ui::Ui::new(canvas, &env, &style),
            rows.len(),
            |ui, i, rect, focused| {
                let canvas = ui.canvas();
                ui::Row::value(rows[i].0, rows[i].1)
                    .focused(focused)
                    .draw(&mut ui::Ui::new(canvas, &env, &style), rect);
            },
        );
    });
    let parts = semantics::parts(described);
    let told: Vec<(&str, Role, bool)> = parts
        .iter()
        .map(|p| (p.label.as_str(), p.role, p.focused))
        .collect();
    assert_eq!(
        told,
        [
            ("Night mode, On", Role::Row, false),
            ("Font size, 12 pt", Role::Row, true),
            ("Language, English", Role::Row, false),
        ]
    );
    assert_eq!(
        semantics::focused(&parts).map(|p| p.label.as_str()),
        Some("Font size, 12 pt")
    );
    assert_eq!(
        semantics::find(&parts, "LANGUAGE").map(|p| p.area),
        Some(list.row_rect(2))
    );
    assert!(semantics::find(&parts, "nothing like it").is_none());
}

#[test]
fn nothing_told_tells_nothing() {
    assert!(semantics::parts(Vec::new()).is_empty());
    assert!(semantics::focused(&[]).is_none());
    assert!(semantics::find(&[], "").is_none());
}

// ---- Menus ----------------------------------------------------------------

const NEEDS: [MenuNeed; 9] = [
    MenuNeed::Speech,
    MenuNeed::Sleep,
    MenuNeed::Rotation,
    MenuNeed::NoTouch,
    MenuNeed::ButtonRow,
    MenuNeed::Colors,
    MenuNeed::Wifi,
    MenuNeed::Firmware,
    MenuNeed::BlackAndWhite,
];

fn caps_of(bits: u16) -> Capabilities {
    let mut caps = Capabilities::default();
    let flags: [&mut bool; 9] = [
        &mut caps.speech,
        &mut caps.sleep,
        &mut caps.rotation,
        &mut caps.touch,
        &mut caps.button_row,
        &mut caps.colors,
        &mut caps.wifi,
        &mut caps.firmware,
        &mut caps.black_and_white,
    ];
    for (n, flag) in flags.into_iter().enumerate() {
        *flag = bits & (1 << n) != 0;
    }
    caps
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// An entry that needs something is offered where the device has it
    /// (one for devices without touch: where there is none, or where the
    /// user listed it), in the plugins' menus and the user's alike; an
    /// entry without a need everywhere.
    #[test]
    fn entries_are_offered_where_the_device_can_do_what_they_need(bits in 0u16..512) {
        let caps = caps_of(bits);
        let mut menus = Menus::new();
        menus.set_capabilities(caps);
        let menu = Id::new_static("t.menu");
        let mut ids = Vec::new();
        for (n, need) in NEEDS.iter().enumerate() {
            let id = Id::parse(&format!("t.entry{n}")).unwrap();
            menus.add(MenuEntry::new(menu.clone(), id.clone()).order(n as i16));
            menus.set_title(id.clone(), Cow::Borrowed("t.title"));
            menus.set_need(id.clone(), *need);
            ids.push(id);
        }
        let plain = Id::new_static("t.plain");
        menus.add(MenuEntry::new(menu.clone(), plain.clone()).order(99));
        menus.set_title(plain.clone(), Cow::Borrowed("t.title"));
        let has = [
            caps.speech, caps.sleep, caps.rotation, !caps.touch, caps.button_row,
            caps.colors, caps.wifi, caps.firmware, caps.black_and_white,
        ];
        let shown: Vec<&Id> = menus.ids("t.menu");
        let mut expected: Vec<&Id> = ids.iter().zip(has).filter(|(_, h)| *h).map(|(id, _)| id).collect();
        expected.push(&plain);
        prop_assert_eq!(&shown, &expected);
        for (id, has) in ids.iter().zip(has) {
            prop_assert_eq!(menus.available(id.as_str()), has);
        }
        // Listed by the user: the same, but for devices without touch,
        // which the user may list anywhere.
        let mut all = ids.clone();
        all.push(plain.clone());
        menus.customize(menu.clone(), all);
        let mut expected: Vec<&Id> = ids
            .iter()
            .zip(has)
            .enumerate()
            .filter(|(n, (_, h))| *h || NEEDS[*n] == MenuNeed::NoTouch)
            .map(|(_, (id, _))| id)
            .collect();
        expected.push(&plain);
        let shown = menus.ids("t.menu");
        prop_assert_eq!(shown, expected);
    }
}

#[test]
fn entries_come_in_their_order_headings_among_them() {
    let mut menus = Menus::new();
    let menu = Id::new_static("t.menu");
    for (item, order) in [("t.c", 30), ("t.a", 10), ("t.b", 10), ("t.d", -5)] {
        let id = Id::parse(item).unwrap();
        menus.add(MenuEntry::new(menu.clone(), id.clone()).order(order));
        menus.set_title(id, Cow::Borrowed(item));
    }
    menus.add_section(
        MenuSection::new(Id::new_static("t.heading"), "t.heading.title", menu.clone()).order(20),
    );
    menus.set_icon(Id::new_static("t.a"), Cow::Borrowed("books"));
    let (actions, settings) = (paperpaw_sdk::Actions::new(), paperpaw_sdk::Settings::new());
    let items = paperpaw_sdk::MenuView::new(&menus, &actions, &settings).items("t.menu");
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    // Equal orders keep the order they were added in.
    assert_eq!(ids, ["t.d", "t.a", "t.b", "t.heading", "t.c"]);
    assert!(items[3].section && !items[0].section);
    assert_eq!(items[1].icon, Some("books"));
    assert_eq!(menus.order_of("t.menu", "t.heading"), Some(20));
    assert!(menus.menu_ids().contains(&&menu));
    // A user's menu naming what does not exist: left out, no panic.
    menus.customize(
        menu.clone(),
        vec![Id::new_static("t.gone"), Id::new_static("t.c")],
    );
    let ids: Vec<&str> = paperpaw_sdk::MenuView::new(&menus, &actions, &settings)
        .items("t.menu")
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(ids, ["t.c"]);
    assert!(menus.is_customized("t.menu"));
    assert_eq!(menus.default_ids("t.menu").len(), 5);
    menus.reset("t.menu");
    assert!(!menus.is_customized("t.menu"));
    // Hidden where a menu is opened so, the list written any way.
    let p = Params::new().with(params::HIDE, " t.a ,t.b,, ");
    assert!(Menus::hidden(&p, "t.a") && Menus::hidden(&p, "t.b"));
    assert!(!Menus::hidden(&p, "t.c"));
}

// ---- The logo -------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48)))]

    /// The drawn logo fills the middle of its area as a square, as large
    /// as fits, and draws nowhere else; too small to show, it draws
    /// nothing.
    #[test]
    fn the_logo_is_drawn_square_in_the_middle(
        x in 0u16..100,
        y in 0u16..100,
        width in 0u16..200,
        height in 0u16..200,
    ) {
        let h = Harness::new(Device::TOUCH);
        let area = Rect::new(x, y, width, height);
        let logo = Logo::Drawn;
        let (w, hh) = logo.fit(width, height);
        prop_assert_eq!((w, hh), (width.min(height), width.min(height)));
        let mut frame = frame(300, 300);
        logo.draw(&mut frame.canvas(), &h.storage, area, Color::BLACK);
        let side = width.min(height);
        let at = Rect::new(x + (width - side) / 2, y + (height - side) / 2, side, side);
        prop_assert_eq!(drawn_outside(&mut frame, at), None);
        if side >= 16 {
            prop_assert!(drawn_in(&frame, at), "the logo shows");
        }
        if side < 8 {
            prop_assert!(!drawn_in(&frame, Rect::new(0, 0, 300, 300)));
        }
    }
}

#[test]
fn a_logo_picture_that_is_not_there_is_the_drawn_logo() {
    let mut h = Harness::new(Device::TOUCH);
    assert_eq!(Logo::read(&h.env(), 200, 200), Logo::Drawn, "none set");
    h.define_text(logo::LOGO.as_str(), " /plugins/mine/logo.png/ ");
    assert_eq!(Logo::read(&h.env(), 200, 200), Logo::Drawn, "not there");
    h.storage
        .write("plugins/mine/logo.png", b"not a picture")
        .unwrap();
    assert_eq!(Logo::read(&h.env(), 200, 200), Logo::Drawn, "not a picture");
}

// ---- The look of widgets ----------------------------------------------------

const CLOCK: Id = Id::new_static("t.clock");

fn look_harness(frame_value: &str, align: &str) -> Harness {
    let mut h = Harness::new(Device::TOUCH);
    h.define_text("t.clock.frame", frame_value);
    h.define_text("t.clock.align", align);
    h
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// Every frame draws inside the widget's area and leaves its content
    /// an area inside it; the caption and wrapped text keep to that.
    #[test]
    fn a_widgets_frame_caption_and_text_keep_to_its_area(
        frame_kind in 0usize..5,
        align in 0usize..4,
        x in 0u16..100,
        y in 0u16..300,
        width in 100u16..380,
        height in 40u16..400,
        caption in "\\PC{0,30}",
        text in "[a-zA-Z ]{0,120}",
    ) {
        let frames = ["core.frame.none", "core.frame.line", "core.frame.card", "core.frame.dark", "t.unknown"];
        let aligns = ["core.align.left", "core.align.center", "core.align.right", ""];
        let h = look_harness(frames[frame_kind], aligns[align]);
        let env = h.env();
        let area = Rect::new(x, y, width, height);
        let mut frame = frame(480, 800);
        let look = widgets::draw_frame(&mut frame.canvas(), &env, "t.clock", area);
        prop_assert_eq!(drawn_outside(&mut frame, area), None);
        let inner = look.area;
        prop_assert!(inner.x >= area.x && inner.right() <= area.right());
        prop_assert!(inner.y >= area.y && inner.bottom() <= area.bottom());
        if look.frame == Frame::Dark {
            prop_assert_eq!(look.ink, Color::WHITE);
        }
        let mut frame = support::frame(480, 800);
        let below = widgets::draw_caption(&mut frame.canvas(), &env, "t.clock", &look, &caption);
        prop_assert!(below.y >= inner.y && below.bottom() == inner.bottom());
        let style = UiStyle::new(&env);
        let drawn = widgets::draw_wrapped(
            &mut frame.canvas(),
            &env,
            below,
            widgets::align(&env, "t.clock"),
            style.text(),
            &text,
            look.ink,
        );
        prop_assert!(drawn <= below.height.max(widgets::line_height(&env, style.text())));
        // Two pixels of slack for letters reaching past their box.
        let bound = Rect::new(inner.x.saturating_sub(2), inner.y, inner.width + 4, inner.height);
        if below.height >= widgets::line_height(&env, style.text()) {
            prop_assert_eq!(drawn_outside(&mut frame, bound), None);
        }
    }

    /// Text broken into lines: no line wider than the width, no more
    /// lines than fit the height (one at least), the last shortened
    /// where there is more.
    #[test]
    fn wrapped_text_fits_its_width_and_height(
        text in "[a-zA-Z äöü,.]{0,300}",
        width in 100u16..400,
        height in 0u16..300,
        align in proptest::sample::select(vec![Align::Left, Align::Center, Align::Right]),
    ) {
        let h = Harness::new(Device::TOUCH);
        let env = h.env();
        let style = UiStyle::new(&env);
        let wrapped = Wrapped::new(&env, &text, style.text(), width, height);
        let line = widgets::line_height(&env, style.text());
        prop_assert!(wrapped.height(&env) <= height.max(line));
        let area = Rect::new(20, 10, width, height.max(line));
        let (described, mut frame) = describe(480, 800, |c| {
            wrapped.draw(c, &env, area, align, Color::BLACK);
        });
        if !text.trim().is_empty() {
            prop_assert_eq!(described.len(), 1, "told as one text");
        }
        // Two pixels of slack for letters reaching past their box.
        let bound = Rect::new(area.x - 2, area.y, area.width + 4, area.height);
        prop_assert_eq!(drawn_outside(&mut frame, bound), None);
    }
}

/// Found by the property above: a caption of a combining mark alone (a
/// diaeresis below) reached back past the area's left edge, onto a
/// letter that is not there. It sits on the start of its line instead.
#[test]
fn a_caption_of_a_lone_mark_keeps_to_its_area() {
    let h = look_harness("core.frame.none", "");
    let env = h.env();
    let area = Rect::new(3, 0, 100, 99);
    let mut frame = frame(480, 800);
    let look = widgets::draw_frame(&mut frame.canvas(), &env, "t.clock", area);
    let mut frame = support::frame(480, 800);
    widgets::draw_caption(&mut frame.canvas(), &env, "t.clock", &look, "\u{324}");
    assert!(drawn_in(&frame, look.area), "the mark is drawn");
    assert_eq!(drawn_outside(&mut frame, look.area), None);
}

/// The lines of a heading on more lines are closer than those of text,
/// and still the highest letters of a line (capitals with accents) stay
/// clear of the lowest of the line above.
#[test]
fn heading_lines_are_closer_than_text_and_never_touch() {
    let h = Harness::new(Device::TOUCH);
    let env = h.env();
    let font = UiStyle::new(&env).font(&env).unwrap();
    for size in [12.0, 16.0, 20.0, 26.0, 34.0, 48.0] {
        let apart = font.heading_line_height(size);
        assert!(apart < font.line_metrics(size).line_height, "{size}");
        assert_eq!(widgets::heading_line_height(&env, size), apart as u16);
        let (top, _) = font.ink(size, "ÅÄÉÖÜ").unwrap();
        let (_, bottom) = font.ink(size, "gjpqyß").unwrap();
        assert!(
            ((bottom - top) as f32) < apart,
            "{size}: {top} {bottom} {apart}"
        );
    }
}

#[test]
fn frames_and_sizes_read_from_settings_fall_back_when_unknown() {
    let h = look_harness("core.frame.cloud", "sideways");
    let env = h.env();
    assert_eq!(widgets::frame(&env, "t.clock"), Frame::None);
    assert_eq!(widgets::align(&env, "t.clock"), Align::Left);
    assert_eq!(widgets::size(&env, "t.clock"), TextSize::Medium);
    assert!(!widgets::shows(&env, "t.clock", "date"));
    let style = UiStyle::new(&env);
    assert!(TextSize::Small.text(&style) < TextSize::Medium.text(&style));
    assert!(TextSize::Medium.text(&style) < TextSize::Large.text(&style));
    assert_eq!(TextSize::Fit.text(&style), TextSize::Large.text(&style));
}

fn shows_the_placement(env: &Env<'_>) -> String {
    env.settings().text("t.clock.frame").to_string()
}

fn sixty(_: &Env<'_>, _: u16) -> u16 {
    60
}

fn draw_nothing(_: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}

/// A placement of a widget (`t.clock@2`) reads its own values of the
/// widget's settings, the widget itself the global ones; a drawn widget
/// says it changed only when what it shows did, once.
#[test]
fn placements_read_their_own_settings_and_changes_are_told_once() {
    let mut h = look_harness("core.frame.line", "core.align.left");
    h.widgets.add(WidgetSpec::drawn(
        CLOCK,
        "t.clock",
        Drawn::new(sixty, draw_nothing).shows(shows_the_placement),
    ));
    h.settings
        .set_from_text("t.clock@2.frame", "core.frame.dark")
        .unwrap();
    assert!(h.widgets.has("t.clock@2"));
    assert_eq!(widgets::base("t.clock@2"), "t.clock");
    assert_eq!(widgets::base("t.clock"), "t.clock");
    assert_eq!(h.widgets.title("t.clock@7"), Some("t.clock"));
    {
        let env = h.env();
        assert_eq!(widgets::frame(&env, "t.clock"), Frame::Line);
        let placed = env
            .settings()
            .with_placement(Some("t.clock@2"), || widgets::frame(&env, "t.clock"));
        assert_eq!(placed, Frame::Dark);
        assert!(h.widgets.make("t.unknown", &env).is_none());
    }
    let (mut widget, mut copy) = {
        let env = h.env();
        let widget = h.widgets.make("t.clock", &env).unwrap();
        let copy = h.widgets.make("t.clock@2", &env).unwrap();
        (widget, copy)
    };
    {
        let env = h.env();
        assert!(!widget.update(&env), "nothing changed since it was made");
        assert!(!copy.update(&env));
        assert_eq!(copy.height(&env, 100), 60);
    }
    // The placement's frame changes: the copy changed, the widget not.
    h.settings
        .set_from_text("t.clock@2.frame", "core.frame.card")
        .unwrap();
    let env = h.env();
    assert!(!widget.update(&env));
    assert!(copy.update(&env), "told");
    assert!(!copy.update(&env), "once");
}

#[test]
fn fills_share_what_is_left_and_never_overflow() {
    assert_eq!(widgets::fill_height(u32::MAX, 0, 1), u16::MAX);
    assert_eq!(widgets::fill_height(100, 0, usize::MAX), 0);
    assert_eq!(widgets::fill_height(0, 100, 3), 0);
    assert_eq!(widgets::fill_height(301, 0, 3), 100);
    let area = Rect::new(10, 0, 100, 20);
    for align in [Align::Left, Align::Center, Align::Right] {
        for width in [0.0, 50.0, 100.0, 500.0, f32::INFINITY] {
            let x = align.x(area, width);
            assert!((10.0..=110.0).contains(&x), "{align:?} {width}: {x}");
        }
    }
}

/// Lengths of time read as people say them, in the language's words.
#[test]
fn durations_read_in_hours_and_minutes() {
    let h = support::Harness::new(support::Device::BUTTONS);
    let env = h.env();
    let text = |s| paperpaw_sdk::time::duration_text(&env, s);
    assert_eq!(text(0), "< 1 min");
    assert_eq!(text(59), "< 1 min");
    assert_eq!(text(60 * 42), "42 min");
    assert_eq!(text(3600 + 5 * 60 + 30), "1 h 05 min");
    assert_eq!(text(10 * 3600), "10 h 00 min");
}

/// What choosing a setting opens is the same on every screen: text on the
/// keyboard, answered to the kernel; an editor; a switch nothing.
#[test]
fn a_text_setting_opens_the_keyboard_on_every_screen() {
    use paperpaw_sdk::builtin::{events, params, slots};
    use paperpaw_sdk::{Command, SettingSpec, Text};
    const NAME: paperpaw_sdk::Setting<Text> = paperpaw_sdk::Setting::new("t.name");
    const ON: paperpaw_sdk::Setting<bool> = paperpaw_sdk::Setting::new("t.on");
    let h = support::Harness::new(support::Device::TOUCH);
    let env = h.env();
    let name = SettingSpec::text(
        NAME.static_str(),
        "t.name",
        "Ann",
        paperpaw_sdk::builtin::groups::OTHER,
    );
    let Some(Command::OpenSlot(slot, asked)) = ui::setting_opens(&env, &name) else {
        panic!("the keyboard");
    };
    assert_eq!(slot, slots::KEYBOARD);
    assert_eq!(
        asked.get(params::REPLY),
        Some(events::SETTING_TYPED.as_str())
    );
    assert_eq!(asked.get(params::ID), Some("t.name"));
    static EDIT: paperpaw_sdk::SettingExtra =
        paperpaw_sdk::SettingExtra::new().editor(Id::new_static("t.edit"));
    let edited = SettingSpec::text(
        NAME.static_str(),
        "t.name",
        "",
        paperpaw_sdk::builtin::groups::OTHER,
    )
    .with_extra(&EDIT);
    assert!(matches!(
        ui::setting_opens(&env, &edited),
        Some(Command::OpenScreen(screen, _)) if screen == Id::new_static("t.edit")
    ));
    assert_eq!(
        ui::setting_opens(
            &env,
            &SettingSpec::switch(
                ON.static_str(),
                "t.on",
                true,
                paperpaw_sdk::builtin::groups::OTHER
            )
        ),
        None
    );
}

/// A letter that reaches far past its box (the fraction slash, U+2044,
/// meant to overhang its neighbours) still keeps to the widget's area,
/// first and last in a caption or a line, wherever they are placed.
#[test]
fn an_overhanging_letter_keeps_to_the_area() {
    let frames = [
        "core.frame.none",
        "core.frame.line",
        "core.frame.card",
        "core.frame.dark",
    ];
    let aligns = ["core.align.left", "core.align.center", "core.align.right"];
    for text in ["⁄", "⁄1⁄2⁄"] {
        for frame_value in frames {
            for align in aligns {
                let h = look_harness(frame_value, align);
                let env = h.env();
                let area = Rect::new(10, 10, 150, 120);
                let look =
                    widgets::draw_frame(&mut frame(480, 800).canvas(), &env, "t.clock", area);
                let inner = look.area;
                let bound = Rect::new(inner.x - 2, inner.y, inner.width + 4, inner.height);
                let mut drawn = frame(480, 800);
                let below =
                    widgets::draw_caption(&mut drawn.canvas(), &env, "t.clock", &look, text);
                let style = UiStyle::new(&env);
                let wrapped_text = format!("{text} words {text}");
                widgets::draw_wrapped(
                    &mut drawn.canvas(),
                    &env,
                    below,
                    widgets::align(&env, "t.clock"),
                    style.text(),
                    &wrapped_text,
                    look.ink,
                );
                assert_eq!(
                    drawn_outside(&mut drawn, bound),
                    None,
                    "{text:?} {frame_value} {align}"
                );
            }
        }
    }
}
