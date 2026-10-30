//! The keyboard's size (`keyboard.size`) on the X4 Pro and the Boox Go 7:
//! larger sizes give higher keys, no size goes below a finger's least
//! key or past half the screen, Medium is the keyboard as before; with
//! buttons only the size makes keys higher and the rows stay whole.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::semantics::{Part, Role};
use paperpaw_sdk::{Id, MemoryStorage, Metrics, Params};

/// The X4 Pro as the emulator's profile has it: touch and buttons.
const X4_PRO: DeviceInfo = DeviceInfo::new(
    "test-x4-pro",
    "Test X4 Pro",
    DisplayInfo::new(480, 800, 217, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        true,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

/// The Boox Go 7: a larger touch screen, finer.
const BOOX: DeviceInfo = DeviceInfo::new(
    "test-boox",
    "Test Boox",
    X4_PRO
        .display
        .with_width(1264)
        .with_height(1680)
        .with_dpi(300)
        .with_format(PixelFormat::Gray8),
    InputCaps::new(false, true, true, &[]),
    MemoryClass::Large,
);

/// The X4: buttons only.
const X4: DeviceInfo = X4_PRO
    .with_id("test-x4")
    .with_name("Test X4")
    .with_input(InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ));

const SIZES: [&str; 3] = [
    "keyboard.size.small",
    "keyboard.size.medium",
    "keyboard.size.large",
];

/// The keyboard as drawn on `device` with `config`: its parts.
fn keyboard(device: DeviceInfo, config: &str) -> Vec<Part> {
    let mut kernel = Kernel::builder(device)
        .storage(MemoryStorage::new())
        .user_config(format!(
            "[core]\nlanguage = en\n[help]\nshow = false\n{config}"
        ))
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Keyboard)
        .build()
        .unwrap();
    let mut effects = kernel.start(Instant(0));
    settle(&mut kernel, &mut effects);
    let params = Params::new().with(params::TITLE, "Name");
    let mut effects = kernel.open_slot(Instant(100), slots::KEYBOARD, params);
    settle(&mut kernel, &mut effects);
    assert_eq!(
        kernel.active_screen().map(Id::as_str),
        Some("keyboard.screen")
    );
    kernel.render(&mut |_, _| {});
    kernel.describe()
}

fn settle(kernel: &mut Kernel, effects: &mut Effects) {
    let mut now = 0;
    while let Some(at) = effects.wake_at() {
        if at.0 > now + 1000 {
            return;
        }
        now = now.max(at.0);
        *effects = kernel.tick(Instant(now));
    }
}

fn key(parts: &[Part], label: &str) -> Part {
    parts
        .iter()
        .find(|p| p.role == Role::Button && p.label == label)
        .unwrap_or_else(|| panic!("no key says {label:?}"))
        .clone()
}

/// What a key takes from one row to the next, in pixels: its height
/// where it is tapped, seam included.
fn step(parts: &[Part]) -> u16 {
    key(parts, "a").area.y - key(parts, "q").area.y
}

/// How high the keyboard is, from the top of its title to the bottom.
fn height(device: DeviceInfo, parts: &[Part]) -> u16 {
    let top = parts
        .iter()
        .find(|p| p.label == "Name")
        .expect("the title")
        .area
        .y;
    device.display.height - top
}

fn sized(device: DeviceInfo, size: &str) -> Vec<Part> {
    keyboard(device, &format!("[keyboard]\nsize = {size}\n"))
}

#[test]
fn larger_sizes_give_higher_keys_and_never_below_a_finger() {
    for device in [X4_PRO, BOOX] {
        let least = Metrics::new(device.display.dpi).mm(4) as u16;
        let steps: Vec<u16> = SIZES.iter().map(|s| step(&sized(device, s))).collect();
        assert!(steps[0] < steps[1], "{}: {steps:?}", device.id);
        assert!(steps[1] < steps[2], "{}: {steps:?}", device.id);
        for size in SIZES {
            let parts = sized(device, size);
            assert!(
                step(&parts) >= least,
                "{} {size}: {}",
                device.id,
                step(&parts)
            );
            let height = height(device, &parts);
            assert!(
                height <= device.display.height / 2,
                "{} {size}: {height}",
                device.id
            );
            // The keys stay as wide as before: the size changes heights.
            assert_eq!(
                key(&parts, "q").area.width,
                key(&keyboard(device, ""), "q").area.width
            );
        }
    }
}

#[test]
fn round_keys_take_about_30_40_and_50_percent() {
    for device in [X4_PRO, BOOX] {
        let least = Metrics::new(device.display.dpi).mm(4) as u16;
        for (size, near) in SIZES.iter().zip([30, 40, 50]) {
            let parts = sized(device, size);
            let share = u32::from(height(device, &parts)) * 100 / u32::from(device.display.height);
            // Where the keys are as small as a finger allows, the
            // keyboard is as low as it can be.
            let floored = step(&parts) == least && share > near;
            assert!(
                floored || share.abs_diff(near) <= 2,
                "{} {size}: {share} %",
                device.id
            );
        }
    }
}

#[test]
fn medium_is_the_keyboard_as_before() {
    for device in [X4_PRO, BOOX] {
        assert_eq!(
            sized(device, "keyboard.size.medium"),
            keyboard(device, ""),
            "{}",
            device.id
        );
    }
}

/// Keys chosen with buttons: the rows stay a computer keyboard's at
/// every size (qwertyuiop, asdfghjkl, zxcvbnm whole), and the size makes
/// the keys higher.
#[test]
fn with_buttons_the_size_makes_keys_higher_in_whole_rows() {
    // The letters' rows, by the top of their keys.
    let rows = |parts: &[Part]| {
        let mut tops: Vec<u16> = parts
            .iter()
            .filter(|p| p.label.chars().count() == 1 && p.label.chars().all(char::is_alphabetic))
            .map(|p| p.area.y)
            .collect();
        tops.dedup();
        tops.len()
    };
    let (small, medium, large) = (
        sized(X4, "keyboard.size.small"),
        sized(X4, "keyboard.size.medium"),
        sized(X4, "keyboard.size.large"),
    );
    for parts in [&small, &medium, &large] {
        assert_eq!(rows(parts), 3, "qwertyuiop, asdfghjkl, zxcvbnm whole");
    }
    let high = |parts: &[Part]| key(parts, "q").area.height;
    assert!(high(&small) < high(&medium) && high(&medium) < high(&large));
    assert!(height(X4, &small) < height(X4, &medium) && height(X4, &medium) < height(X4, &large));
}

#[test]
fn the_floor_wins_where_a_size_cannot_be_reached() {
    // A small screen of large pixels: Small would go below a finger.
    let coarse = X4_PRO.with_display(
        X4_PRO
            .display
            .with_width(320)
            .with_height(480)
            .with_dpi(160),
    );
    let least = Metrics::new(160).mm(4) as u16;
    let small = sized(coarse, "keyboard.size.small");
    assert!(step(&small) >= least, "{}", step(&small));
}

#[test]
#[ignore = "measures, for the notes"]
fn key_widths_on_the_x4() {
    for size in [
        "keyboard.size.small",
        "keyboard.size.medium",
        "keyboard.size.large",
    ] {
        let parts = sized(X4, size);
        let q = key(&parts, "q").area;
        let w = key(&parts, "w").area;
        std::println!(
            "{size}: q {}x{} at x {}, gap {}, keyboard {} high",
            q.width,
            q.height,
            q.x,
            w.x - (q.x + q.width),
            height(X4, &parts)
        );
    }
    for size in [
        "keyboard.size.small",
        "keyboard.size.medium",
        "keyboard.size.large",
    ] {
        let parts = sized(BOOX, size);
        let q = key(&parts, "q").area;
        let w = key(&parts, "w").area;
        std::println!(
            "boox {size}: q {}x{}, gap {}",
            q.width,
            q.height,
            w.x - (q.x + q.width)
        );
    }
}
