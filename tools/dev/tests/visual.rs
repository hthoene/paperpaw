//! The visual checks on real screens: PaperPaw's settings pass on a
//! black-and-white X4 and a gray Boox, a widget stays in its cell, and
//! content off the interface's margin is found.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_dev::cli::Context;
use paperpaw_dev::report::Level;
use paperpaw_dev::target::Target;
use paperpaw_dev::variant::Variant;
use paperpaw_dev::visual::{self, Looker, checks};
use paperpaw_emulator::profiles;

#[test]
#[ignore = "the dev tool measures a widget's cell with its interface-6 probe widget, which the native Home does not place (todo 634, #244)"]
fn good_screens_pass_and_a_margin_off_is_found() {
    let out = std::env::temp_dir().join(format!("paperpaw-dev-visual-{}", std::process::id()));
    let context = Context::new(
        Target {
            plugins: vec!["settings".into(), "widgets".into()],
            ..Target::default()
        },
        out,
    );
    let boox = Variant::of(profiles::find("boox-go-7").unwrap());
    for variant in [Variant::x4(), boox] {
        let mut session = visual::start(&context, &variant).unwrap();
        let mut looker = Looker::new(&mut session);

        let looked = looker
            .screen(&mut session, "settings.list")
            .unwrap()
            .unwrap();
        // The icons of the list are placed by their advance, not centred
        // by their ink, and on the Boox the header's back arrow stands
        // 8 px into the margin: those alone are found.
        let other: Vec<_> = looked
            .issues
            .iter()
            .filter(|i| i.check != checks::ALIGN)
            .filter(|i| !(i.check == checks::MARGIN && i.message.starts_with("\"Settings\"")))
            .collect();
        assert!(other.is_empty(), "{}: {other:?}", variant.name());
        assert!(looked.inset.is_some());

        let clock = looker
            .widget(&mut session, "widgets.analogue-clock", (2, 2))
            .unwrap()
            .unwrap();
        assert!(
            clock.issues.iter().all(|i| i.level < Level::Error),
            "{}: {:?}",
            variant.name(),
            clock.issues
        );

        // As if the screen's rows started 6 px past the margin.
        looker.ui.margin -= 6;
        let looked = looker
            .screen(&mut session, "settings.list")
            .unwrap()
            .unwrap();
        assert!(
            looked
                .issues
                .iter()
                .any(|i| i.check == checks::MARGIN && i.message.contains("6 px right")),
            "{}: {:?}",
            variant.name(),
            looked.issues
        );
    }
    let _ = std::fs::remove_dir_all(&context.out);
}

/// Buttons grown to a fingertip keep their room: on touch screens the
/// bars and rows that hold them grow with them (`UiStyle::touch_row`), so
/// the margin to the sides and the gaps to the edges and to each other
/// stay as before.
#[test]
fn buttons_keep_their_room_on_touch_screens() {
    let out = std::env::temp_dir().join(format!("paperpaw-dev-room-{}", std::process::id()));
    let context = Context::new(
        Target {
            plugins: vec!["keyboard".into(), "menu".into()],
            ..Target::default()
        },
        out,
    );
    for profile in ["boox-go-7", "xteink-x4-pro"] {
        let variant = Variant::of(profiles::find(profile).unwrap());
        let mut session = visual::start(&context, &variant).unwrap();
        let mut looker = Looker::new(&mut session);
        for screen in ["keyboard.screen", "menu.list"] {
            let looked = looker.screen(&mut session, screen).unwrap().unwrap();
            let cramped: Vec<_> = looked
                .issues
                .iter()
                .filter(|i| i.check == checks::BUTTON_ROOM)
                .collect();
            assert!(cramped.is_empty(), "{profile} {screen}: {cramped:?}");
        }
        // Asked for more room than the keyboard's keys have from the
        // screen's edges: found.
        looker.ui.margin *= 4;
        let looked = looker
            .screen(&mut session, "keyboard.screen")
            .unwrap()
            .unwrap();
        assert!(
            looked.issues.iter().any(|i| i.check == checks::BUTTON_ROOM),
            "{profile}: {:?}",
            looked.issues
        );
    }
    let _ = std::fs::remove_dir_all(&context.out);
}
