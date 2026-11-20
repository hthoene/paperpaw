//! Light or dark for the screens of the sleep, the switch-off, the start and
//! the restart (#420): each follows its own setting whoever draws it, the
//! start picture drawn ahead included; the screens around keep night mode.

use super::*;
use crate::Staging;
use crate::start_picture::{self, Prepared};
use paperpaw_sdk::Params;
use paperpaw_sdk::builtin::params;

const SCREEN: Id = Id::new_static("sleep.shot");
const OTHER: Id = Id::new_static("sleep.other");
const DEVICE_LOOK: &str = "sleep.look.device";
const SETTINGS: [(&str, Id); 4] = [
    ("sleep.look", slots::SLEEP),
    ("sleep.off-look", slots::OFF),
    ("sleep.start-look", slots::START),
    ("sleep.restart-look", slots::RESTART),
];

/// A screen with a black square at the top left on white.
struct Shot;

impl crate::testing::Fake for Shot {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        canvas.clear(Color::WHITE);
        canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
    }
}

fn plugin() -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    |r| {
        for id in [SCREEN, OTHER] {
            r.add_fake(crate::testing::FakeSpec::new(
                id,
                "look",
                Box::new(|_, _| Box::new(Shot)),
            ))?;
        }
        for (setting, slot) in SETTINGS {
            r.add_setting(SettingSpec::new(
                Id::new_static(setting),
                setting,
                SettingKind::Text,
                SettingValue::Text(DEVICE_LOOK.into()),
                paperpaw_sdk::builtin::groups::OTHER,
            ))?;
            r.bind_slot(slot, SCREEN, Priority::DEFAULT)?;
        }
        Ok(())
    }
}

fn kernel() -> Kernel {
    let manifest = Manifest::new(Id::new_static("sleep"), "Sleep", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING);
    let mut kernel = Kernel::builder(DEVICE)
        .items(manifest, plugin())
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

/// Whether the corner square (black as drawn) and the rest (white as drawn)
/// come out inverted, of the frame drawn now.
fn inverted(kernel: &mut Kernel) -> bool {
    let (mut corner, mut rest) = (None, None);
    kernel.render(&mut |top, view| {
        if top == 0 {
            corner = Some(view.pixel(0, 0).r);
        }
        if top <= 50 && 50 < top + view.height {
            rest = Some(view.pixel(50, 50 - top).r);
        }
    });
    match (corner, rest) {
        (Some(0), Some(255)) => false,
        (Some(255), Some(0)) => true,
        other => panic!("not the square on white, drawn: {other:?}"),
    }
}

/// Opens the screen of `slot` and draws it.
fn show(kernel: &mut Kernel, slot: &Id) {
    let now = Instant(10);
    if *slot == slots::OFF {
        kernel.switch_off_screen(now, params::OFF_TIMER).unwrap();
    } else if *slot == slots::RESTART {
        kernel.restart_screen(now, &|| now).unwrap();
    } else {
        kernel.apply(Ask::OpenSlot(slot.clone(), Params::new()));
        kernel.finish();
    }
}

fn set(kernel: &mut Kernel, id: &str, value: &str) {
    kernel.set_setting(Instant(5), id, value).unwrap();
}

#[test]
fn each_screen_follows_its_own_look_and_the_others_night_mode() {
    for (setting, slot) in SETTINGS
        .into_iter()
        .filter(|(_, slot)| *slot != slots::START)
    {
        for (look, night, dark) in [
            (DEVICE_LOOK, false, false),
            (DEVICE_LOOK, true, true),
            ("sleep.look.light", true, false),
            ("sleep.look.dark", false, true),
        ] {
            let mut kernel = kernel();
            set(
                &mut kernel,
                "core.night-mode",
                if night { "on" } else { "off" },
            );
            set(&mut kernel, setting, look);
            show(&mut kernel, &slot);
            assert_eq!(inverted(&mut kernel), dark, "{slot} {look} night {night}");
        }
        // Another screen's look does not touch it.
        let mut kernel = kernel();
        for (other, _) in SETTINGS {
            if other != setting {
                set(&mut kernel, other, "sleep.look.dark");
            }
        }
        show(&mut kernel, &slot);
        assert!(!inverted(&mut kernel), "{slot} by another's look");
    }
}

#[test]
fn a_screen_of_its_own_look_gives_night_mode_back_when_it_closes() {
    let mut kernel = kernel();
    set(&mut kernel, "core.night-mode", "on");
    set(&mut kernel, "sleep.look", "sleep.look.light");
    show(&mut kernel, &slots::SLEEP);
    assert!(!inverted(&mut kernel));
    kernel.run_action(Instant(20), actions::BACK);
    kernel.render(&mut |_, _| {});
    assert!(
        kernel.framebuffer.is_inverted(),
        "the screen below, at night"
    );
    // A screen not from a slot keeps night mode, whatever the looks say.
    kernel.open_screen(Instant(30), OTHER, Params::new());
    assert!(inverted(&mut kernel));
}

#[test]
fn the_start_picture_is_drawn_ahead_as_its_look_says() {
    for (look, night, dark) in [
        (DEVICE_LOOK, false, false),
        (DEVICE_LOOK, true, true),
        ("sleep.look.light", true, false),
        ("sleep.look.dark", false, true),
    ] {
        let mut kernel = kernel();
        set(
            &mut kernel,
            "core.night-mode",
            if night { "on" } else { "off" },
        );
        set(&mut kernel, "sleep.start-look", look);
        let prepared = kernel.prepare_start_picture(Instant(0), &|| Instant(0));
        assert_eq!(prepared, Prepared::Written);
        let mut rows = Vec::new();
        start_picture::show(kernel.storage(), 90, 90, 32, &mut |_, view| {
            rows.extend_from_slice(view.data);
            true
        })
        .unwrap();
        // 12 bytes a row, the panel's 1 for white: the square at the top
        // left is black as drawn, the rest white, inverted for dark.
        assert_eq!(rows[0], if dark { 0xff } else { 0x00 }, "{look} {night}");
        assert_eq!(
            rows[12 * 10 + 1],
            if dark { 0x00 } else { 0xff },
            "{look} {night}"
        );
    }
}
