use super::*;
use crate::fail_alloc::sweep;
use crate::native_model::{SETTING_SHOWN, SETTING_SLIDER, flag};
use alloc::string::ToString;
use alloc::vec;

/// Eight pixels a character.
struct Eight;

impl Measure for Eight {
    fn width(&self, text: &str) -> f32 {
        text.chars().count() as f32 * 8.0
    }
}

fn geometry() -> Geometry {
    Geometry {
        width: 480,
        height: 800,
        gap: 10,
        spacing: 5,
        row: 60,
        button: 40,
        border: 2,
        label: 150,
        pager: 45,
        margin: 20,
        rtl: false,
        top: false,
        touch: false,
    }
}

fn row(item: &str, kind: Kind, title: &str) -> Row {
    Row {
        item: item.to_string(),
        kind,
        flags: flag::ENABLED,
        title: title.to_string(),
        icon: String::new(),
        badge: String::new(),
        describe: String::new(),
    }
}

fn info(kind: SettingKind, choices: u32) -> SettingInfo {
    SettingInfo {
        key: "a.setting".to_string(),
        kind,
        flags: SETTING_SHOWN,
        value: None,
        min: 0,
        max: 10,
        step: 1,
        unit: String::new(),
        editor_kind: 0,
        editor: String::new(),
        choices,
    }
}

fn entry<'a>(row: &'a Row, info: Option<&'a SettingInfo>, options: &'a [String]) -> Entry<'a> {
    Entry { row, info, options }
}

#[test]
fn switches_and_buttons_share_rows_but_not_with_each_other() {
    let switch = info(SettingKind::Bool, 0);
    let rows = [
        row("a.one", Kind::Setting, "One"),
        row("a.two", Kind::Setting, "Two"),
        row("a.run", Kind::Action, "Run"),
        row("a.menu", Kind::Submenu, "More"),
    ];
    let entries = [
        entry(&rows[0], Some(&switch), &[]),
        entry(&rows[1], Some(&switch), &[]),
        entry(&rows[2], None, &[]),
        entry(&rows[3], None, &[]),
    ];
    let lines = lines(&entries, &Eight, &geometry()).unwrap();
    assert_eq!(
        lines,
        [
            Line::Buttons(vec![Control::Toggle(0), Control::Toggle(1)]),
            Line::Buttons(vec![Control::Run(2), Control::Run(3)]),
        ]
    );
}

#[test]
fn four_buttons_go_two_and_two_not_three_and_one() {
    // "• " and 26 letters are 224 px: three do not fit a 480 px row (each
    // cell 400 / 3 - 20 = 113 px), two do (180 px)... after the bullet two
    // of 224 do not either, so these are shorter: 15 letters = 136 px.
    let title = "ABCDEFGHIJKLMNO";
    let rows: Vec<Row> = (0..4).map(|_| row("a.run", Kind::Action, title)).collect();
    let entries: Vec<Entry<'_>> = rows.iter().map(|r| entry(r, None, &[])).collect();
    let lines = lines(&entries, &Eight, &geometry()).unwrap();
    assert_eq!(
        lines,
        [
            Line::Buttons(vec![Control::Run(0), Control::Run(1)]),
            Line::Buttons(vec![Control::Run(2), Control::Run(3)]),
        ]
    );
}

#[test]
fn a_long_label_gets_a_row_of_its_own() {
    let long = row(
        "a.long",
        Kind::Action,
        "A label that is far too long for two",
    );
    let short = row("a.short", Kind::Action, "Short");
    let entries = [entry(&long, None, &[]), entry(&short, None, &[])];
    let lines = lines(&entries, &Eight, &geometry()).unwrap();
    assert_eq!(lines.len(), 2);
}

#[test]
fn a_badge_follows_the_label_and_counts_in_the_width() {
    let mut r = row("a.run", Kind::Action, "Run");
    assert_eq!(button_text(&r).unwrap(), "Run");
    r.badge = "3".to_string();
    assert_eq!(button_text(&r).unwrap(), "Run · 3");
}

#[test]
fn settings_become_the_control_that_suits_them() {
    let number = info(SettingKind::Int, 0);
    let two = info(SettingKind::Choice, 2);
    let many = info(SettingKind::Choice, 9);
    let mut own = info(SettingKind::Bool, 0);
    own.editor_kind = 1;
    let text = info(SettingKind::Text, 0);
    let rows = [
        row("a.size", Kind::Setting, "Size"),
        row("a.align", Kind::Setting, "Align"),
        row("a.font", Kind::Setting, "Font"),
        row("a.own", Kind::Setting, "Own editor"),
        row("a.text", Kind::Setting, "Text"),
        row("a.gone", Kind::Setting, "No registry entry"),
        row("", Kind::Section, "Heading"),
    ];
    let two_options = vec!["Left".to_string(), "Right".to_string()];
    let nine = vec!["x".to_string(); 5];
    let entries = [
        entry(&rows[0], Some(&number), &[]),
        entry(&rows[1], Some(&two), &two_options),
        entry(&rows[2], Some(&many), &nine),
        entry(&rows[3], Some(&own), &[]),
        entry(&rows[4], Some(&text), &[]),
        entry(&rows[5], None, &[]),
        entry(&rows[6], None, &[]),
    ];
    let lines = lines(&entries, &Eight, &geometry()).unwrap();
    assert_eq!(
        lines,
        [
            Line::Stepper(0),
            Line::Segments(1, 2),
            // More than four options open their list.
            Line::Open(2),
            // A setting with its own editor opens it.
            Line::Open(3),
            Line::Open(4),
            // No registry entry: a button, as an action is (interface 6).
            Line::Buttons(vec![Control::Run(5)]),
        ]
    );
}

#[test]
fn settings_that_are_not_shown_now_are_left_out() {
    let mut hidden = info(SettingKind::Int, 0);
    hidden.flags = 0;
    let lonely = info(SettingKind::Choice, 1);
    let shown = info(SettingKind::Int, 0);
    let rows = [
        row("a.hidden", Kind::Setting, "Hidden"),
        row("a.lonely", Kind::Setting, "Lonely"),
        row("a.shown", Kind::Setting, "Shown"),
    ];
    let entries = [
        entry(&rows[0], Some(&hidden), &[]),
        entry(&rows[1], Some(&lonely), &[]),
        entry(&rows[2], Some(&shown), &[]),
    ];
    assert_eq!(
        lines(&entries, &Eight, &geometry()).unwrap(),
        [Line::Stepper(2)]
    );
    // Nor do they give the labels their width.
    assert_eq!(label_width(&entries, &Eight, &geometry()), 5 * 8 + 20);
}

#[test]
fn the_pager_of_a_tiny_screen_does_not_underflow() {
    let tiny = Geometry {
        height: 100,
        ..geometry()
    };
    let lines: Vec<Line> = (0..5).map(Line::Open).collect();
    let layout = layout(&lines, &tiny).unwrap();
    assert!(layout.pager.is_some());
    assert!(layout.region.height <= 100);
}

#[test]
fn options_that_do_not_fit_open_a_list() {
    let two = info(SettingKind::Choice, 2);
    let r = row("a.align", Kind::Setting, "Align");
    let wide = vec![
        "A very wide option".to_string(),
        "Another wide one".to_string(),
    ];
    let entries = [entry(&r, Some(&two), &wide)];
    assert_eq!(
        lines(&entries, &Eight, &geometry()).unwrap(),
        [Line::Open(0)]
    );
}

#[test]
fn labels_are_as_wide_as_the_longest_at_most_half_the_row() {
    let number = info(SettingKind::Int, 0);
    let switch = info(SettingKind::Bool, 0);
    let a = row("a.size", Kind::Setting, "Size");
    let b = row(
        "a.switch",
        Kind::Setting,
        "A switch with a very long label indeed",
    );
    let c = row("a.long", Kind::Setting, "Zoom");
    let entries = [entry(&a, Some(&number), &[]), entry(&b, Some(&switch), &[])];
    // Switches carry no label: "Size" is 32 px and two gaps.
    assert_eq!(label_width(&entries, &Eight, &geometry()), 32 + 20);
    let wide = row("a.wide", Kind::Setting, &"W".repeat(60));
    let entries = [
        entry(&wide, Some(&number), &[]),
        entry(&c, Some(&number), &[]),
    ];
    assert_eq!(label_width(&entries, &Eight, &geometry()), (480 - 40) / 2);
}

#[test]
fn at_the_bottom_rows_follow_each_other() {
    let lines = [Line::Stepper(0), Line::Open(1)];
    let layout = layout(&lines, &geometry()).unwrap();
    assert_eq!(layout.region.y + layout.region.height, 800);
    assert!(layout.pager.is_none());
    let [first, second] = layout.rows.as_slice() else {
        panic!("two rows");
    };
    assert_eq!((first.page, second.page), (0, 0));
    assert_eq!(second.area.y, first.area.y + 60);
    assert!(first.area.y >= layout.region.y + 2);
    assert_eq!(first.label, Some(0));
}

/// From the top, from the screen's very edge: no band of the screen below
/// shows above it.
#[test]
fn from_the_top_it_starts_at_the_edge() {
    let g = Geometry {
        top: true,
        ..geometry()
    };
    let one = layout(&[Line::Open(0)], &g).unwrap();
    assert_eq!(one.region.y, 0);
    assert_eq!(one.rows[0].area.y, 10, "a gap below the edge");
    // Many rows: at most the share of the screen, the pager at its foot,
    // above the border towards the rest of the screen.
    let lines: Vec<Line> = (0..20).map(Line::Open).collect();
    let paged = layout(&lines, &g).unwrap();
    assert_eq!(paged.region.y, 0);
    assert!(u32::from(paged.region.height) <= 800 * SHARE_TOP / 100);
    let pager = paged.pager.expect("a pager");
    assert_eq!(pager.y + pager.height, paged.region.height - 2);
}

#[test]
fn too_many_rows_go_to_pages() {
    let lines: Vec<Line> = (0..20).map(Line::Open).collect();
    let layout = layout(&lines, &geometry()).unwrap();
    assert!(layout.rows.last().unwrap().page > 0);
    assert!(u32::from(layout.region.height) <= 800 * SHARE_BOTTOM / 100);
    let pager = layout.pager.expect("a pager");
    assert_eq!(pager.y + pager.height, 800);
    // A row of the second page stands where the first page's first one does,
    // and every row is within the panel above the pager.
    let second_page = layout.rows.iter().find(|r| r.page == 1).unwrap();
    assert_eq!(second_page.area.y, layout.rows[0].area.y);
    assert!(
        layout
            .rows
            .iter()
            .all(|r| r.area.bottom() <= u32::from(pager.y))
    );
}

#[test]
fn rows_fill_each_page_in_turn() {
    let lines: Vec<Line> = (0..20).map(Line::Open).collect();
    let layout = layout(&lines, &geometry()).unwrap();
    let pages: Vec<usize> = layout.rows.iter().map(|r| r.page).collect();
    let per_page = pages.iter().take_while(|p| **p == 0).count();
    assert!(per_page > 1 && per_page < 20);
    assert!(pages.windows(2).all(|w| w[0] <= w[1] && w[1] - w[0] <= 1));
}

#[test]
fn controls_share_their_row() {
    let buttons = Line::Buttons(vec![
        Control::Toggle(0),
        Control::Toggle(1),
        Control::Toggle(2),
    ]);
    let row = place(&buttons, Rect::new(0, 100, 480, 60), &geometry()).unwrap();
    assert_eq!(row.controls.len(), 3);
    let widths: Vec<u16> = row.controls.iter().map(|(_, r)| r.width).collect();
    // As wide as each other to a pixel, the last ending at the margin.
    assert!(widths.iter().all(|w| w.abs_diff(widths[0]) <= 1));
    assert_eq!(row.controls[2].1.right(), 480 - 20);
    assert_eq!(row.controls[1].1.x, row.controls[0].1.right() as u16 + 5);

    // Right to left, the first is the one at the right.
    let g = Geometry {
        rtl: true,
        ..geometry()
    };
    let mirrored = place(&buttons, Rect::new(0, 100, 480, 60), &g).unwrap();
    assert_eq!(mirrored.controls[0].0, Control::Toggle(0));
    assert!(mirrored.controls[0].1.x > mirrored.controls[1].1.x);
    assert!(mirrored.controls[1].1.x > mirrored.controls[2].1.x);
    assert_eq!(mirrored.controls[0].1.right(), 480 - 20);

    // Segments sit after the label, their options in order.
    let row = place(&Line::Segments(4, 2), Rect::new(0, 0, 480, 60), &geometry()).unwrap();
    assert_eq!(row.controls.len(), 2);
    assert_eq!(row.controls[1].0, Control::Option(4, 1));
    assert!(row.controls[0].1.x >= 20 + 150);
    assert_eq!(row.controls[1].1.right(), 480 - 20);

    // A stepper's `−` and `+` are at the ends of what follows the label.
    let area = Rect::new(0, 0, 480, 60);
    let stepper = place(&Line::Stepper(2), area, &geometry()).unwrap();
    let [(minus, left), (plus, right)] = stepper.controls.as_slice() else {
        panic!("two buttons");
    };
    assert_eq!((*minus, *plus), (Control::Step(2, -1), Control::Step(2, 1)));
    assert_eq!(left.x, 20 + 150);
    assert_eq!(right.right(), 480 - 20);
    assert!(left.right() < u32::from(right.x));
}

#[test]
fn the_models_survive_refused_allocations() {
    let switch = info(SettingKind::Bool, 0);
    let number = info(SettingKind::Int, 0);
    let two = info(SettingKind::Choice, 2);
    let rows = [
        row("a.one", Kind::Setting, "One"),
        row("a.two", Kind::Setting, "Two"),
        row("a.size", Kind::Setting, "Size"),
        row("a.align", Kind::Setting, "Align"),
        row("a.run", Kind::Action, "Run"),
    ];
    let options = vec!["L".to_string(), "R".to_string()];
    let entries = [
        entry(&rows[0], Some(&switch), &[]),
        entry(&rows[1], Some(&switch), &[]),
        entry(&rows[2], Some(&number), &[]),
        entry(&rows[3], Some(&two), &options),
        entry(&rows[4], None, &[]),
    ];
    sweep(|| {
        let lines = lines(&entries, &Eight, &geometry())?;
        let layout = layout(&lines, &geometry())?;
        Ok(layout.rows.len())
    });
    let mut badged = row("a.run", Kind::Action, "Run");
    badged.badge = "1".to_string();
    sweep(|| button_text(&badged).map(|t| t.len()));
}

#[test]
fn the_focus_goes_round_and_past_faint_controls() {
    let all = |_: usize| true;
    assert_eq!(step_focus(0, None, 1, all), None);
    // Nowhere yet: forward the first, back the last.
    assert_eq!(step_focus(4, None, 1, all), Some(0));
    assert_eq!(step_focus(4, None, -1, all), Some(3));
    assert_eq!(step_focus(4, Some(3), 1, all), Some(0));
    assert_eq!(step_focus(4, Some(0), -1, all), Some(3));
    // Past faint ones, round the end.
    let not_one_two = |i: usize| i != 1 && i != 2;
    assert_eq!(step_focus(4, Some(0), 1, not_one_two), Some(3));
    assert_eq!(step_focus(4, Some(3), -1, not_one_two), Some(0));
    // All faint: one on all the same.
    assert_eq!(step_focus(3, Some(0), 1, |_| false), Some(1));
}

#[test]
fn controls_have_pages_rows_and_hits() {
    let lines: Vec<Line> = (0..20).map(Line::Open).collect();
    let layout = layout(&lines, &geometry()).unwrap();
    assert_eq!(layout.count(), 20);
    assert!(layout.pages() > 1);
    assert_eq!(layout.page_of(0), 0);
    assert_eq!(layout.page_of(19), layout.pages() - 1);
    assert_eq!(layout.row_of(1), Some(layout.rows[1].area));
    assert_eq!(layout.row_of(20), None);
    let second = &layout.rows[1];
    let inside =
        paperpaw_sdk::Point::new(i32::from(second.area.x) + 5, i32::from(second.area.y) + 5);
    assert_eq!(layout.hit(0, inside), Some((1, Control::Open(1))));
    // The same place on another page is another control, or none.
    assert_ne!(layout.hit(1, inside).map(|(i, _)| i), Some(1));
    assert_eq!(layout.hit(0, paperpaw_sdk::Point::new(5, 0)), None);
}

#[test]
fn a_long_menu_is_cut_to_the_cap() {
    let mut rows: Vec<Row> = (0..MAX_ROWS + 6)
        .map(|i| row(&alloc::format!("a.{i}"), Kind::Action, "Run"))
        .collect();
    cap_rows(&mut rows);
    assert_eq!(rows.len(), MAX_ROWS);
    assert_eq!(
        rows[MAX_ROWS - 1].item,
        alloc::format!("a.{}", MAX_ROWS - 1)
    );
    let mut few = vec![row("a.one", Kind::Action, "Run")];
    cap_rows(&mut few);
    assert_eq!(few.len(), 1);
}

fn slider_info() -> SettingInfo {
    SettingInfo {
        flags: SETTING_SHOWN | SETTING_SLIDER,
        min: 0,
        max: 100,
        step: 5,
        ..info(SettingKind::Int, 0)
    }
}

#[test]
fn a_number_that_asks_for_a_slider_gets_a_track_on_a_touch_screen_only() {
    let slider = slider_info();
    let rows = [row("a.light", Kind::Setting, "Front light")];
    let entries = [entry(&rows[0], Some(&slider), &[])];
    let touch = Geometry {
        touch: true,
        ..geometry()
    };
    assert_eq!(lines(&entries, &Eight, &touch).unwrap(), [Line::Slider(0)]);
    // Without touch the keys move it: `−` and `+`, as in interface 6.
    assert_eq!(
        lines(&entries, &Eight, &geometry()).unwrap(),
        [Line::Stepper(0)]
    );
    // A slider over one value is not one.
    let flat = SettingInfo {
        max: 0,
        ..slider_info()
    };
    let entries = [entry(&rows[0], Some(&flat), &[])];
    assert_eq!(lines(&entries, &Eight, &touch).unwrap(), [Line::Stepper(0)]);
}

#[test]
fn the_track_lies_between_the_minus_and_the_value_before_the_plus() {
    let g = Geometry {
        touch: true,
        ..geometry()
    };
    let placed = place(&Line::Slider(0), Rect::new(0, 100, 480, 60), &g).unwrap();
    let track = placed.track.unwrap();
    let (minus, plus) = (placed.controls[0].1, placed.controls[1].1);
    assert_eq!(placed.controls.len(), 2, "the track is not a control");
    assert!(track.x > minus.x + minus.width);
    assert!(u32::from(track.x) + u32::from(track.width) < u32::from(plus.x));
    assert!(track.width > 40, "room to move a finger: {track:?}");
    // A tap on it is found, on the buttons it is not.
    let layout = Layout {
        rows: vec![placed],
        region: Rect::new(0, 100, 480, 60),
        pager: None,
    };
    let middle = paperpaw_sdk::Point::new(i32::from(track.x) + 20, i32::from(track.y) + 5);
    assert_eq!(layout.track_at(0, middle).map(|t| t.0), Some(0));
    let on_plus = paperpaw_sdk::Point::new(i32::from(plus.x) + 5, i32::from(plus.y) + 5);
    assert_eq!(layout.track_at(0, on_plus), None);
    assert_eq!(layout.track_at(1, middle), None, "another page");
}

#[test]
fn a_tap_sets_the_nearest_step_and_the_ends_are_reached() {
    let track = Rect::new(100, 0, 260, 60);
    let radius = i32::from(knob_radius(track));
    let range = (0, 100, 5);
    let (first, last) = (100 + radius, 100 + 260 - radius);
    assert_eq!(value_at(track, first, range), 0);
    assert_eq!(value_at(track, last, range), 100);
    // Beyond the ends: the ends.
    assert_eq!(value_at(track, 0, range), 0);
    assert_eq!(value_at(track, 480, range), 100);
    // The middle is the middle, on a step.
    let mid = (first + last) / 2;
    let value = value_at(track, mid, range);
    assert!((45..=55).contains(&value) && value % 5 == 0, "{value}");
    // The knob stands where a tap on it would set.
    let at = knob_x(track, 40, (0, 100));
    assert_eq!(value_at(track, at, range), 40);
    // A range that does not start at 0, and a step that does not divide it.
    let odd = value_at(track, (first + last) / 2, (10, 31, 4));
    assert!((10..=31).contains(&odd) && (odd - 10) % 4 == 0, "{odd}");
}
