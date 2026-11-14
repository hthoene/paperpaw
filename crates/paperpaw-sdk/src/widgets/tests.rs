//! Tests of `widgets.rs`.
use super::*;

#[test]
fn alignments_place_text_in_the_area() {
    let area = Rect::new(10, 0, 100, 20);
    assert_eq!(Align::Left.x(area, 40.0), 10.0);
    assert_eq!(Align::Center.x(area, 40.0), 40.0);
    assert_eq!(Align::Right.x(area, 40.0), 70.0);
    // Too wide: from the left edge.
    assert_eq!(Align::Right.x(area, 140.0), 10.0);
}

#[test]
fn alignments_round_trip() {
    for align in [Align::Left, Align::Center, Align::Right] {
        assert_eq!(Align::of(align.value()), align);
    }
    assert_eq!(Align::of("sideways"), Align::Left);
}

#[test]
fn fills_share_the_room_left() {
    assert_eq!(fill_height(1000, 400, 2), 300);
    assert_eq!(fill_height(1000, 1200, 1), 0);
    assert_eq!(fill_height(1000, 400, 0), 0);
    assert!(repeatable(FILL) && repeatable(SPACE) && !repeatable("home.clock"));
}

#[test]
fn previews_go_with_their_group_only() {
    fn nothing(_: &Env<'_>, _: u16) -> u16 {
        0
    }
    fn draw(_: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}
    let drawn = Drawn::new(nothing, draw);
    let reading = Id::new_static("core.group.reading");
    let mut widgets = Widgets::new();
    widgets.add(WidgetSpec::drawn(
        Id::new_static("a.clock"),
        "a.clock",
        drawn,
    ));
    widgets.add(
        WidgetSpec::drawn(Id::new_static("a.sample"), "a.sample", drawn).previews(reading.clone()),
    );
    widgets.add(WidgetSpec::drawn(Id::new_static("b.sample"), "b.sample", drawn).previews(reading));
    // Not offered for screens; the one added last previews.
    let offered: Vec<&str> = widgets.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(offered, ["a.clock"]);
    assert_eq!(
        widgets.preview("core.group.reading").map(Id::as_str),
        Some("b.sample")
    );
    assert_eq!(widgets.preview("core.group.display"), None);
    assert!(widgets.has("a.sample"));
}

/// Widgets kept in the program are found, made and listed as those
/// made at the start, in the order they came, and take one entry of
/// two words for the list.
#[test]
fn widgets_kept_in_the_program_are_as_those_made() {
    fn nothing(_: &Env<'_>, _: u16) -> u16 {
        0
    }
    fn draw(_: &mut Canvas<'_>, _: &Env<'_>, _: Rect) {}
    static FIXED: [FixedWidget; 3] = [
        FixedWidget::drawn(
            Id::new_static("a.clock"),
            "a.clock.title",
            Drawn::new(nothing, draw),
        )
        .category(categories::TIME)
        .cells(CellSpan::new(2, 1)),
        FixedWidget::new(Id::new_static("a.made"), "a.made.title", |_| {
            Box::new(DrawnWidget {
                drawn: Drawn::new(nothing, draw),
                shown: None,
            })
        }),
        FixedWidget::drawn(
            Id::new_static("a.sample"),
            "a.sample",
            Drawn::new(nothing, draw),
        )
        .previews(Id::new_static("core.group.reading")),
    ];
    let mut widgets = Widgets::new();
    widgets.add(WidgetSpec::drawn(
        Id::new_static("b.first"),
        "b.first",
        Drawn::new(nothing, draw),
    ));
    widgets.add_fixed(&FIXED);
    widgets.add(WidgetSpec::drawn(
        Id::new_static("c.last"),
        "c.last",
        Drawn::new(nothing, draw),
    ));
    let offered: Vec<&str> = widgets.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(offered, ["b.first", "a.clock", "a.made", "c.last"]);
    assert_eq!(widgets.title("a.clock@2"), Some("a.clock.title"));
    assert_eq!(widgets.category("a.clock"), Some(categories::TIME));
    assert_eq!(widgets.category("a.made"), Some(categories::OTHER));
    assert_eq!(widgets.cells("a.clock"), Some(CellSpan::new(2, 1)));
    assert_eq!(
        widgets.preview("core.group.reading").map(Id::as_str),
        Some("a.sample")
    );
    assert!(widgets.has("a.made") && !widgets.has("a.none"));
    assert_eq!(
        core::mem::size_of::<Entry>(),
        2 * core::mem::size_of::<usize>()
    );
}

#[test]
fn the_setting_belongs_to_the_widget() {
    let widget = Id::new_static("home.clock");
    let spec = align_setting(&widget, Align::Center);
    assert_eq!(spec.id.as_str(), "home.clock.align");
    assert_eq!(spec.group, widget);
    assert_eq!(
        spec.default,
        SettingValue::Text(Cow::Borrowed("core.align.center"))
    );
}
