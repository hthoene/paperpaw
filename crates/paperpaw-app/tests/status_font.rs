//! The status lines' font (`core.status-font`): the menus' at first, the
//! book's or a family of its own once chosen, for every reader (role
//! `core.font-status`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::builtin::{fonts, settings};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

fn status(kernel: &Kernel) -> String {
    let font = kernel.fonts().role(fonts::STATUS.as_str()).unwrap();
    font.id().to_string()
}

fn role(kernel: &Kernel, role: &paperpaw_sdk::Id) -> String {
    kernel.fonts().role(role.as_str()).unwrap().id().to_string()
}

#[test]
fn the_status_lines_follow_their_setting() {
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config("[help]\nshow = false\n");
    let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
    let _ = kernel.start(Instant(0));
    // As the menus at first.
    assert_eq!(status(&kernel), role(&kernel, &fonts::UI));
    assert_ne!(role(&kernel, &fonts::UI), role(&kernel, &fonts::READING));
    // As the book.
    let _ = kernel
        .set_setting(
            Instant(10),
            settings::STATUS_FONT.as_str(),
            settings::STATUS_FONT_BOOK,
        )
        .unwrap();
    assert_eq!(status(&kernel), role(&kernel, &fonts::READING));
    // A family of its own: every family installed is offered.
    let spec = kernel
        .settings()
        .spec(settings::STATUS_FONT.as_str())
        .unwrap();
    let paperpaw_sdk::SettingKind::Choice(choices) = &spec.kind else {
        panic!("a choice");
    };
    let families = kernel.fonts().families();
    assert!(families.iter().all(|f| choices.contains(f)), "{families:?}");
    let ui_family = families
        .iter()
        .find(|f| f.contains("Source Sans") || f.contains("PaperPaw Sans"))
        .unwrap()
        .to_string();
    let _ = kernel
        .set_setting(Instant(20), settings::STATUS_FONT.as_str(), &ui_family)
        .unwrap();
    assert_eq!(status(&kernel), role(&kernel, &fonts::UI));
}

#[test]
fn a_font_chosen_as_source_sans_3_is_kept_where_the_packs_are_paperpaw_sans() {
    // Chosen by the family's name before the packs were renamed.
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage).user_config(
        "[help]\nshow = false\n\n[core]\nreading-font = Source Sans 3\nstatus-font = Source Sans 3\n",
    );
    let packs = matches!(fonts(), paperpaw_app::Fonts::Packs);
    let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
    let _ = kernel.start(Instant(0));
    let _ = kernel.tick(Instant(10));
    let family = if packs {
        "PaperPaw Sans"
    } else {
        "Source Sans 3"
    };
    for setting in [settings::READING_FONT, settings::STATUS_FONT] {
        assert_eq!(
            kernel.settings().text(setting.as_str()),
            family,
            "{setting:?}"
        );
    }
    assert!(kernel.fonts().families().contains(&family));
}
