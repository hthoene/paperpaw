//! Menus defined and reset, screens best in grays and settings reset.

use super::*;

#[test]
fn menus_follow_order_and_user_definitions() {
    let log = log();
    let kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    let env_items = |kernel: &Kernel| -> Vec<String> {
        kernel
            .menus()
            .items("core.main")
            .iter()
            .map(|i| i.id.to_string())
            .collect()
    };
    let items = env_items(&kernel);
    // Entries of the core plugin and this plugin, sorted by order: a
    // short list, the quick panel first, the tools in a menu of their own.
    assert_eq!(
        items.iter().take(4).map(String::as_str).collect::<Vec<_>>(),
        ["core.quick-panel", "core.home", "core.home", "core.library"]
    );
    assert!(items.iter().any(|i| i == "core.tools"));
    assert!(!kernel.menus().items("core.main")[0].section);
    assert_eq!(kernel.shared.menus.icon("core.library"), Some("books"));
    assert_eq!(kernel.shared.menus.icon("core.tools"), Some("apps"));
    assert!(items.iter().filter(|a| *a == "core.home").count() == 2);

    let kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log }),
        )
        .user_config("[menus]\ncore.main = core.settings, core.toggle-night-mode\n")
        .build()
        .unwrap();
    assert_eq!(
        env_items(&kernel),
        ["core.settings", "core.toggle-night-mode"]
    );
}

struct GrayScreen;

impl crate::testing::Fake for GrayScreen {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Place<'_>) {}

    fn flags(&self) -> u32 {
        crate::testing::GRAYS
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A home screen best in grays (a picture), and the half overlay as its
/// menu.
pub(crate) fn gray_plugin() -> impl Fn(&mut Staging) -> Result<(), RegisterError> {
    |r| {
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("gray.page"),
            "page",
            Box::new(|_, _| Box::new(GrayScreen)),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("gray.page"), Priority::DEFAULT)?;
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("gray.menu"),
            "menu",
            Box::new(|_, _| Box::new(HalfOverlay)),
        ))?;
        r.bind_slot(slots::MENU, Id::new_static("gray.menu"), Priority::DEFAULT)
    }
}

/// What [`gray_plugin`] registers under.
pub(crate) fn gray_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("gray"), "Gray", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

#[test]
fn screens_on_top_say_when_they_are_best_in_grays() {
    let mut kernel = Kernel::builder(DEVICE)
        .items(gray_plugin_manifest(), gray_plugin())
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    assert!(effects.refresh().is_some() && effects.grays());
    render(&mut kernel);
    // A menu over it: not while it shows; back again once it closed.
    let effects = press(&mut kernel, 100, "menu");
    assert!(effects.refresh().is_some() && !effects.grays());
    render(&mut kernel);
    let effects = press(&mut kernel, 300, "back");
    assert!(effects.refresh().is_some() && effects.grays());
}

#[test]
fn frames_drawn_again_in_grays_in_bands_of_as_many_bytes() {
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log() }),
        )
        .frame_rows(32)
        .row_align(8)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let format = kernel.frame().format;
    let bits = u32::from(format.bits_per_pixel());
    let mut bands = Vec::new();
    assert!(kernel.render_gray(&mut |top, rows| {
        bands.push((top, rows.height, rows.format));
    }));
    // The whole screen, in two bits a pixel, bands no larger in bytes.
    assert_eq!(bands.iter().map(|b| u32::from(b.1)).sum::<u32>(), 90);
    assert!(bands.iter().all(|b| b.2 == PixelFormat::Gray2));
    assert!(bands.iter().all(|b| u32::from(b.1) * 2 <= 32 * bits));
    // Its own bands again after.
    assert_eq!(kernel.frame().format, format);
    assert_eq!(kernel.frame().height, 32);
}

#[test]
fn drawn_again_whole_and_fully() {
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    let effects = kernel.draw_again(Instant(100));
    assert_eq!(
        effects.refresh(),
        Some((Rect::new(0, 0, 90, 90), RefreshMode::Full))
    );
}

#[test]
fn settings_reset_to_their_defaults() {
    let storage = paperpaw_sdk::MemoryStorage::new().with(
        "paperpaw.ini",
        "[core]
ui-size = 12
full-refresh-interval = 3
",
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log() }),
        )
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let ui_size = "core.ui-size";
    let default = kernel
        .shared
        .settings
        .specs()
        .find(|s| s.id.as_str() == ui_size)
        .unwrap()
        .default
        .clone();
    assert_ne!(kernel.shared.settings.get(ui_size), Some(&default));
    kernel.apply(Ask::ResetSetting(
        paperpaw_sdk::builtin::settings::UI_SIZE.id().clone(),
    ));
    kernel.finish();
    assert_eq!(kernel.shared.settings.get(ui_size), Some(&default));
    // Gone from the file, the other setting kept.
    let mut buf = [0u8; 256];
    let n = kernel
        .shared
        .storage
        .read_at("paperpaw.ini", 0, &mut buf)
        .unwrap();
    let file = core::str::from_utf8(&buf[..n]).unwrap();
    assert!(!file.contains("ui-size"), "{file}");
    assert!(file.contains("full-refresh-interval = 3"), "{file}");
}

#[test]
fn menus_are_redefined_saved_and_reset() {
    let log = log();
    let storage = paperpaw_sdk::MemoryStorage::new().with(
        "paperpaw.ini",
        "[core]
ui-size = 12
",
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log }),
        )
        .build()
        .unwrap();
    let items = |kernel: &Kernel| -> Vec<String> {
        kernel
            .menus()
            .items("core.main")
            .iter()
            .map(|i| i.id.to_string())
            .collect()
    };
    let file = |kernel: &Kernel| -> String {
        let mut buf = [0u8; 4096];
        let n = kernel
            .shared
            .storage
            .read_at("paperpaw.ini", 0, &mut buf)
            .unwrap();
        String::from(core::str::from_utf8(&buf[..n]).unwrap())
    };
    let defaults = items(&kernel);
    kernel.set_menu(
        Id::new_static("core.main"),
        Some(alloc::vec![
            Id::new_static("core.settings"),
            Id::new_static("core.home")
        ]),
    );
    assert_eq!(items(&kernel), ["core.settings", "core.home"]);
    assert!(kernel.shared.menus.is_customized("core.main"));
    // With what plugins put there when it was redefined: entries added
    // later show too.
    let saved = file(&kernel);
    let start = "[core]
ui-size = 12

[menus]
core.main = core.settings, core.home

[menus-known]
core.main = ";
    assert!(saved.starts_with(start), "{saved}");
    // Only what the user left out.
    let known = &saved[start.len()..];
    assert!(
        !known.contains("core.home") && !known.contains("core.settings"),
        "{saved}"
    );
    kernel.set_menu(Id::new_static("core.main"), None);
    assert_eq!(items(&kernel), defaults);
    assert_eq!(
        file(&kernel),
        "[core]
ui-size = 12

[menus]

[menus-known]
"
    );
}
