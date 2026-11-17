//! The one-time low-memory notice (#248): the kernel shows it only over the
//! home screen, with a way to the plugin manager's list.

use super::*;

struct Blank;

impl crate::testing::Fake for Blank {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A home screen and another, under "lm".
fn plain(r: &mut crate::Items) -> Result<(), RegisterError> {
    for id in ["lm.home", "lm.other"] {
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static(id),
            "screen",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
    }
    r.bind_slot(slots::HOME, Id::new_static("lm.home"), Priority::DEFAULT)
}

/// Stands for the plugin manager, which has the list.
fn manager(r: &mut crate::Items) -> Result<(), RegisterError> {
    r.add_fake(crate::testing::FakeSpec::new(
        Id::new_static("plugin-manager.list"),
        "list",
        Box::new(|_, _| Box::new(Blank)),
    ))
}

fn kernel() -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            Manifest::new(Id::new_static("lm"), "Lm", Version::new(1, 0, 0))
                .uses(paperpaw_sdk::Uses::NOTHING),
            plain,
        )
        .items(
            Manifest::new(
                Id::new_static("plugin-manager"),
                "Manager",
                Version::new(1, 0, 0),
            )
            .uses(paperpaw_sdk::Uses::NOTHING),
            manager,
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel
}

fn active(kernel: &Kernel) -> Option<&str> {
    kernel.active_screen().map(Id::as_str)
}

#[test]
fn the_notice_shows_over_the_home_screen_only() {
    let mut kernel = kernel();
    assert_eq!(active(&kernel), Some("lm.home"));
    let (shown, _) = kernel.show_low_memory_notice(Instant(100));
    assert!(shown);
    assert_eq!(active(&kernel), Some("core.low-memory"));
    // Not over another screen: not over itself either.
    let (again, _) = kernel.show_low_memory_notice(Instant(200));
    assert!(!again);
    press(&mut kernel, 300, "back");
    assert_eq!(active(&kernel), Some("lm.home"));
    kernel.open_screen(
        Instant(400),
        Id::new_static("lm.other"),
        paperpaw_sdk::Params::new(),
    );
    render(&mut kernel);
    let (over_other, _) = kernel.show_low_memory_notice(Instant(500));
    assert!(!over_other);
    assert_eq!(active(&kernel), Some("lm.other"));
}

#[test]
fn plugins_goes_to_the_plugin_managers_list_and_not_now_back_to_home() {
    let mut kernel = kernel();
    assert!(kernel.show_low_memory_notice(Instant(100)).0);
    press(&mut kernel, 300, "confirm");
    assert_eq!(active(&kernel), Some("plugin-manager.list"));
    let mut kernel = self::kernel();
    assert!(kernel.show_low_memory_notice(Instant(100)).0);
    press(&mut kernel, 300, "right");
    press(&mut kernel, 500, "confirm");
    assert_eq!(active(&kernel), Some("lm.home"));
}
