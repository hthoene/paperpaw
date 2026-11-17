//! Places of widgets: a stack of them above each other, placed in the
//! middle or pushed by free spaces, what does not fit left out; what an
//! event means for the widgets a screen shows; whether any place shows a
//! widget.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::widgets::{
    self, FILL, PlaceKind, Widget, WidgetPlace, WidgetSpec, WidgetStack, WidgetTarget,
};
use paperpaw_sdk::{Canvas, Command, Env, Id, Rect, SettingSpec, SettingValue};

use support::{Device, Harness};

/// A widget as high as `n` times 100, that runs `t.open` when tapped.
struct Block(u16);

impl Widget for Block {
    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        self.0 * 100
    }

    fn draw(&self, _: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        vec![WidgetTarget::new(
            area,
            Command::run(Id::new_static("t.open")),
        )]
    }
}

const LIST: Setting<Text> = Setting::new("t.list");
const PLACE_SETTINGS: &[Id] = &[Id::new_static("t.list")];

/// Blocks one and three high, and a place listing them in `t.list`.
fn harness() -> Harness {
    let mut h = Harness::new(Device::TOUCH);
    h.widgets
        .add(WidgetSpec::new(Id::new_static("t.one"), "t.one", |_| {
            Box::new(Block(1))
        }));
    h.widgets.add(WidgetSpec::new(
        Id::new_static("t.three"),
        "t.three",
        |_| Box::new(Block(3)),
    ));
    h.widgets.add_place(WidgetPlace::new(
        Id::new_static("t.place"),
        "t.place",
        PLACE_SETTINGS,
        PlaceKind::Stack,
    ));
    h.settings
        .define(SettingSpec::text(
            LIST.static_str(),
            "t.list",
            "",
            paperpaw_sdk::builtin::groups::OTHER,
        ))
        .unwrap();
    h
}

/// Where the widgets of a stack of `list` go in an 800 high screen.
fn areas(h: &Harness, list: &str) -> Vec<Rect> {
    let env = h.env();
    WidgetStack::new(&env, list, Rect::new(0, 0, 480, 800))
        .targets(&env)
        .into_iter()
        .map(|t| t.area)
        .collect()
}

#[test]
fn a_stack_is_in_the_middle_unless_free_space_pushes_it() {
    let h = harness();
    let gap = paperpaw_sdk::ui::UiStyle::new(&h.env()).gap();
    let margin = paperpaw_sdk::ui::UiStyle::new(&h.env()).margin();
    let two = areas(&h, "t.one, t.one");
    assert_eq!(two.len(), 2);
    let total = 200 + 2 * gap;
    assert_eq!(two[0].y, (800 - total) / 2);
    assert_eq!(two[1].y, two[0].y + 100 + 2 * gap);
    assert_eq!((two[0].x, two[0].width), (margin, 480 - 2 * margin));
    // A free space first: at the bottom.
    let pushed = areas(&h, &format!("{FILL}, t.one"));
    assert_eq!(pushed[0].y + pushed[0].height, 800);
    // What does not fit below the others is left out; unknown ones too.
    let full = areas(&h, "t.three, t.three, t.three, t.unknown, t.one");
    assert_eq!(full.len(), 2, "{full:?}");
}

/// Areas are small, medium or large by their shorter side.
#[test]
fn areas_have_a_size_class() {
    use paperpaw_sdk::widgets::SizeClass;
    let h = harness();
    let env = h.env();
    let mm = |n: u32| env.metrics().mm(n) as u16;
    assert_eq!(
        SizeClass::of(&env, Rect::new(0, 0, mm(80), mm(10))),
        SizeClass::Small
    );
    assert_eq!(
        SizeClass::of(&env, Rect::new(0, 0, mm(80), mm(30))),
        SizeClass::Medium
    );
    assert_eq!(
        SizeClass::of(&env, Rect::new(0, 0, mm(80), mm(60))),
        SizeClass::Large
    );
}

/// A plugin's own kind of widgets comes after the kernel's, with its
/// icon; a kind nobody added shows a puzzle piece.
#[test]
fn plugins_add_kinds_of_widgets_with_their_icons() {
    use paperpaw_sdk::widgets::{WidgetCategory, categories};
    let mut h = harness();
    h.widgets
        .add_category(WidgetCategory::new(Id::new_static("t.games"), "dice"));
    let env = h.env();
    let kinds: Vec<&str> = env.widgets().kinds().collect();
    assert_eq!(kinds.first(), Some(&categories::TIME));
    assert_eq!(kinds.last(), Some(&"t.games"));
    assert_eq!(env.widgets().category_icon("t.games"), "dice");
    assert_eq!(env.widgets().category_icon(categories::BOOKS), "books");
    assert_eq!(env.widgets().category_icon("t.unknown"), "puzzle");
}

/// One frame for every widget whose own the reader did not choose, and
/// the settings a widget look covers.
#[test]
fn one_frame_for_all_widgets_unless_one_has_its_own() {
    use paperpaw_sdk::builtin::settings::WIDGET_FRAME;
    use paperpaw_sdk::widgets::Frame;
    use paperpaw_sdk::widgets::{ALL_FRAMES, FRAMES, OWN_FRAME};
    let mut h = harness();
    h.settings
        .define(SettingSpec::choice(
            WIDGET_FRAME.static_str(),
            "t.all",
            ALL_FRAMES,
            OWN_FRAME,
            Id::new_static("t.group"),
        ))
        .unwrap();
    for widget in ["t.one", "t.three"] {
        h.settings
            .define(widgets::frame_setting(
                &Id::parse(widget).unwrap(),
                Frame::None,
            ))
            .unwrap();
    }
    let frame = |h: &Harness, widget| widgets::frame(&h.env(), widget);
    assert_eq!(frame(&h, "t.one"), Frame::None);
    h.settings
        .set(
            WIDGET_FRAME.static_str(),
            SettingValue::Text(FRAMES[2].into()),
        )
        .unwrap();
    assert_eq!(frame(&h, "t.one"), Frame::Card);
    h.settings
        .set("t.three.frame", SettingValue::Text(FRAMES[1].into()))
        .unwrap();
    assert_eq!(frame(&h, "t.three"), Frame::Line, "its own, chosen");
    // Its own shown while chosen, and a placement's own, win too.
    h.settings.set_preview(vec![(
        Id::new_static("t.one.frame"),
        Some(SettingValue::Text(FRAMES[3].into())),
    )]);
    assert_eq!(frame(&h, "t.one"), Frame::Dark, "shown while chosen");
    // Its own kept, but shown reset while one is chosen: the frame of all,
    // as it would be.
    h.settings
        .set("t.three.frame", SettingValue::Text(FRAMES[1].into()))
        .unwrap();
    h.settings
        .set_preview(vec![(Id::new_static("t.three.frame"), None)]);
    assert_eq!(frame(&h, "t.three"), Frame::Card, "shown reset");
    h.settings.set_preview(Vec::new());
    h.settings
        .set("t.one@2.frame", SettingValue::Text(FRAMES[1].into()))
        .unwrap();
    let placed = h
        .settings
        .with_placement(Some("t.one@2"), || frame(&h, "t.one"));
    assert_eq!(placed, Frame::Line, "a placement's own");
    // Not a setting that ends so but is no widget's (a status line's
    // size), nor the frame of all in a theme.
    h.settings
        .define(SettingSpec::number(
            "t.status.size",
            "t.size",
            5,
            16,
            6,
            paperpaw_sdk::builtin::groups::OTHER,
        ))
        .unwrap();
    let covered = paperpaw_sdk::presets::widget_look_settings(&h.settings, &h.widgets);
    let covered: Vec<&str> = covered.iter().map(Id::as_str).collect();
    assert!(covered.contains(&WIDGET_FRAME.static_str()), "{covered:?}");
    assert!(covered.contains(&"t.one.frame"), "{covered:?}");
    assert!(!covered.contains(&"t.list"), "{covered:?}");
    assert!(!covered.contains(&"t.status.size"), "{covered:?}");
    let themed = paperpaw_sdk::presets::theme_settings(&h.settings);
    assert!(
        !themed
            .iter()
            .any(|id| id.as_str() == WIDGET_FRAME.static_str())
    );
}
