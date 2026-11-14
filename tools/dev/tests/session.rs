//! A session on an emulated X4: a screen and a widget shown, the widget's
//! cell measured.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_dev::cases;
use paperpaw_dev::cli::Context;
use paperpaw_dev::session::Session;
use paperpaw_dev::target::Target;
use paperpaw_dev::variant::Variant;

fn context(name: &str) -> Context {
    let out = std::env::temp_dir().join(format!("paperpaw-dev-{name}-{}", std::process::id()));
    Context::new(
        Target {
            plugins: vec!["widgets".into(), "settings".into()],
            ..Target::default()
        },
        out,
    )
}

#[test]
#[ignore = "the dev tool measures a widget's cell with its interface-6 probe widget, which the native Home does not place (todo 634, #244)"]
fn a_screen_and_a_widget_are_shown() {
    let context = context("session");
    let mut session = Session::new(&context, &Variant::x4()).unwrap();
    let screens = cases::screens(&session, &context.target);
    assert!(
        screens
            .iter()
            .all(|s| s.starts_with("widgets.") || s.starts_with("settings."))
    );
    let list = screens
        .iter()
        .find(|s| s.as_str() == "settings.list")
        .unwrap();
    let shown = cases::show_screen(&mut session, list, &[]).unwrap();
    assert!(shown.opened(), "{:?}", shown.shown);
    assert!(!shown.parts.is_empty());
    assert_eq!((shown.image.width, shown.image.height), (480, 800));

    let widgets = cases::widgets(&session, &context.target);
    let (clock, _) = widgets
        .iter()
        .find(|(id, _)| id == "widgets.analogue-clock")
        .unwrap();
    let grid = cases::grid(&session);
    let small = cases::show_widget(&mut session, clock, (1, 1)).unwrap();
    let large = cases::show_widget(&mut session, clock, (grid.0, 2)).unwrap();
    let (small, large) = (small.cell.unwrap(), large.cell.unwrap());
    assert!(large.width > 2 * small.width, "{small:?} {large:?}");
    assert_eq!(small.x, large.x);
    drop(session);
    let _ = std::fs::remove_dir_all(&context.out);
}
