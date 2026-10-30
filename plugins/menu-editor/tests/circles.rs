//! Menus put into their own submenus, with the real kernel: not offered,
//! and where `paperpaw.ini` has such a circle all the same, both menus
//! are listed and *Reset all* undoes it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::BTreeSet;

use paperpaw_core::Kernel;
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_plugin_menu_editor::{MenuEditor, OPEN};
use paperpaw_sdk::builtin::menus;
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

struct Test {
    kernel: Kernel,
    now: u64,
}

impl Test {
    fn new(config: &str) -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new().with("paperpaw.ini", config))
            .plugin(paperpaw_plugin_fonts_default::DefaultFonts)
            .plugin(MenuEditor)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self { kernel, now: 0 }
    }

    fn at(&mut self) -> Instant {
        self.now += 200;
        Instant(self.now)
    }

    fn press(&mut self, name: &'static str) {
        let at = self.at();
        for (later, state) in [(0, ButtonState::Pressed), (50, ButtonState::Released)] {
            self.kernel
                .handle(at.add_millis(later), InputEvent::Button { name, state });
        }
        self.kernel.render(&mut |_, _| {});
    }

    /// Every row's text, page by page with the buttons.
    fn all_labels(&mut self) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        loop {
            let before = seen.len();
            seen.extend(self.kernel.describe().into_iter().map(|p| p.label));
            if seen.len() == before {
                return seen;
            }
            self.press("page-forward");
        }
    }
}

#[test]
fn a_menu_is_not_offered_inside_its_own_submenu() {
    let mut test = Test::new("");
    // Adding to the tools, which the main menu leads to.
    let at = test.at();
    let params = Params::new().with("menu", menus::TOOLS.as_str().to_string());
    test.kernel
        .open_screen(at, Id::new_static("menu-editor.add"), params);
    let labels = test.all_labels();
    assert!(!labels.contains("Main menu"), "{labels:?}");
    assert!(labels.contains("Quick panel"), "{labels:?}");
}

#[test]
fn menus_in_a_circle_are_listed_and_reset() {
    // The main menu in the tools: a circle, as an older editor made it.
    let mut test = Test::new("[menus]\ncore.tools = core.screenshot, core.main\n");
    let at = test.at();
    test.kernel.run_action(at, OPEN);
    let labels = test.all_labels();
    assert!(labels.contains("Main menu"), "{labels:?}");
    assert!(labels.iter().any(|l| l.contains("Tools")), "{labels:?}");
    // *Reset all*, the last row: up from the first, opened anew.
    test.press("back");
    let at = test.at();
    test.kernel.run_action(at, OPEN);
    test.press("up");
    test.press("confirm");
    let data = paperpaw_sdk::read_file(test.kernel.storage(), "paperpaw.ini", 4096).unwrap();
    let config = String::from_utf8(data).unwrap();
    assert!(!config.contains("core.tools ="), "{config}");
}
