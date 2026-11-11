//! The host's notice (#257): where no plugin fills the notice slot (native
//! plugins have no notice screen), the kernel's own shows what is notified;
//! a plugin's screen bound there still wins.

use super::*;
use paperpaw_sdk::Params;

/// Fills the notice slot as the interface-6 menu did.
struct OwnNotice;

impl Plugin for OwnNotice {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("on"), "Own notice", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::new_static("on.notice"),
            "notice",
            Box::new(|_, _| Box::new(HalfOverlay)),
        ))?;
        r.bind_slot(
            slots::NOTICE,
            Id::new_static("on.notice"),
            Priority::DEFAULT,
        )
    }
}

#[test]
fn without_a_plugins_notice_the_host_shows_it() {
    let mut kernel = Kernel::builder(DEVICE).build().unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    assert_eq!(
        kernel.shared.registry.resolve_slot(slots::NOTICE.as_str()),
        Some(&crate::native_host::notice::SCREEN)
    );
    kernel.apply(Command::Notify(String::from("Saved")));
    let effects = kernel.finish();
    assert!(kernel.notice.is_some(), "a notice shows");
    let (region, _) = effects.refresh().unwrap();
    let screen = kernel.framebuffer.bounds();
    assert!(region.width > 0 && region.height > 0, "{region:?}");
    assert!(
        region.y + region.height <= screen.height,
        "on the screen: {region:?}"
    );
    assert_eq!(
        effects.wake_at(),
        Some(Instant(4000)),
        "four seconds by default"
    );
    render(&mut kernel);
    let middle = (region.x + region.width / 2, region.y + region.height / 2);
    assert_eq!(kernel.frame().pixel(middle.0, middle.1), Color::BLACK);
    // The input stays with the screen below.
    assert_eq!(kernel.active_screen(), Some(&screens::PLACEHOLDER));
    // Time is up: gone.
    let effects = kernel.tick(Instant(4000));
    assert!(kernel.notice.is_none());
    assert_eq!(effects.refresh().map(|(r, _)| r), Some(region));
}

/// `core.notice-seconds` says how long; a new notice takes the place of the
/// one shown (never two).
#[test]
fn a_notice_shows_as_long_as_set_and_a_new_one_replaces_it() {
    let mut kernel = Kernel::builder(DEVICE).build().unwrap();
    kernel.start(Instant(0));
    kernel
        .set_setting(Instant(0), settings::NOTICE_SECONDS.as_str(), "2")
        .unwrap();
    render(&mut kernel);
    kernel.apply(Command::Notify(String::from("First")));
    assert_eq!(kernel.finish().wake_at(), Some(Instant(2000)));
    kernel.tick(Instant(1000));
    kernel.apply(Command::Notify(String::from("Second")));
    assert_eq!(
        kernel.finish().wake_at(),
        Some(Instant(3000)),
        "the new one's time"
    );
    kernel.tick(Instant(2500));
    assert!(
        kernel.notice.is_some(),
        "the first's time does not end the second"
    );
    kernel.tick(Instant(3000));
    assert!(kernel.notice.is_none());
}

/// A text too long for three lines is cut on the third, never more lines.
#[test]
fn a_long_text_is_cut_to_three_lines() {
    let kernel = Kernel::builder(DEVICE).plugin(FontPlugin).build().unwrap();
    let env = kernel.shared.env(Instant(0));
    let long = "aaaa ".repeat(400);
    let notice = crate::native_host::notice::Notice::new(
        &env,
        &Params::new().with(paperpaw_sdk::builtin::params::TEXT, long.as_str()),
    );
    assert_eq!(notice.lines().len(), 3, "{:?}", notice.lines());
    let short = crate::native_host::notice::Notice::new(
        &env,
        &Params::new().with(paperpaw_sdk::builtin::params::TEXT, "Saved"),
    );
    assert_eq!(short.lines(), ["Saved"]);
    let empty = crate::native_host::notice::Notice::new(&env, &Params::new());
    assert!(empty.lines().len() <= 1);
}

#[test]
fn a_plugins_notice_wins_over_the_hosts() {
    let mut kernel = Kernel::builder(DEVICE).plugin(OwnNotice).build().unwrap();
    kernel.start(Instant(0));
    assert_eq!(
        kernel.shared.registry.resolve_slot(slots::NOTICE.as_str()),
        Some(&Id::new_static("on.notice"))
    );
}
