//! The waveform lab (`shells/xteink/src/lab.rs`) on the host: its screen
//! opened in a kernel as the X4 Pro has it (touch, no buttons), with the
//! controller said to be a UC8179, tapped and looked at. The firmware
//! cannot run here, nor the panel; what the screen does with taps, what
//! its settings become and how it is laid out can.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

extern crate alloc;

#[path = "../../../shells/xteink/src/lab.rs"]
#[allow(dead_code, reason = "the firmware calls what the test does not")]
mod lab;

use std::borrow::Cow;
use std::path::PathBuf;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    ButtonState, DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::{
    Manifest, MemoryStorage, Params, Plugin, RegisterError, Registrar, Uses, Version,
};

const PRO: DeviceInfo = DeviceInfo::new(
    "test-x4-pro",
    "Test X4 Pro",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(false, true, false, &["power"]),
    MemoryClass::Small,
);

const BUTTONS: DeviceInfo = PRO.with_input(InputCaps::new(
    true,
    false,
    false,
    &["back", "confirm", "left", "right", "up", "down", "power"],
));

/// What `device::X4` registers of the lab.
struct X4;

impl Plugin for X4 {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            paperpaw_sdk::Id::new_static("x4"),
            "Xteink",
            Version::new(0, 1, 0),
        )
        .uses(Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&lab::SETTINGS)?;
        r.add_screens(&lab::SCREENS)?;
        r.add_actions(&lab::ACTIONS)?;
        r.add_menu_entries(&lab::ENTRIES)?;
        r.add_translations(Cow::Borrowed(include_str!(
            "../../../shells/xteink/i18n/en.ini"
        )))?;
        Ok(())
    }
}

fn kernel(device: DeviceInfo) -> Kernel {
    let builder = Kernel::builder(device)
        .storage(MemoryStorage::new())
        .frame_rows(192)
        .row_align(8)
        .user_config("[help]\nshow = false\n[core]\nlanguage = en\n");
    let mut kernel = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs)
        .plugin(X4)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.render(&mut |_, _| {});
    kernel
}

fn tap(kernel: &mut Kernel, at: u64, x: i32, y: i32) -> paperpaw_core::Effects {
    let event = |phase| {
        InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            Point::new(x, y),
            PointerTool::Finger,
            1000,
        ))
    };
    kernel.handle(Instant(at), event(PointerPhase::Down));
    let effects = kernel.handle(Instant(at + 60), event(PointerPhase::Up));
    kernel.render(&mut |_, _| {});
    effects
}

fn press(kernel: &mut Kernel, at: u64, name: &'static str) {
    for (state, at) in [(ButtonState::Pressed, at), (ButtonState::Released, at + 50)] {
        kernel.handle(Instant(at), InputEvent::Button { name, state });
    }
    kernel.render(&mut |_, _| {});
}

/// The frame as it is drawn, written as a PBM next to the other test
/// output: `LAB_OUT` or the temporary directory.
fn shot(kernel: &mut Kernel, name: &str) {
    let (width, height) = (480usize, 800usize);
    let mut bits = vec![0xFFu8; width.div_ceil(8) * height];
    kernel.draw_again(Instant(1));
    kernel.render(&mut |top, rows| {
        let stride = rows.stride;
        for y in 0..usize::from(rows.height) {
            let at = (usize::from(top) + y) * width.div_ceil(8);
            let from = y * stride;
            bits[at..at + width.div_ceil(8)]
                .copy_from_slice(&rows.data[from..from + width.div_ceil(8)]);
        }
    });
    let dir = std::env::var_os("LAB_OUT").map_or_else(std::env::temp_dir, PathBuf::from);
    let mut file = format!("P4\n{width} {height}\n").into_bytes();
    // PBM: 1 is black; the frame has 1 for white.
    file.extend(bits.iter().map(|b| !b));
    std::fs::write(dir.join(format!("{name}.pbm")), file).unwrap();
}

/// Whether the pixel at (`x`, `y`) of the frame is white.
fn white(kernel: &mut Kernel, x: usize, y: usize) -> bool {
    let mut white = false;
    kernel.draw_again(Instant(1));
    kernel.render(&mut |top, rows| {
        let top = usize::from(top);
        if (top..top + usize::from(rows.height)).contains(&y) {
            white = rows.data[(y - top) * rows.stride + x / 8] & (0x80 >> (x % 8)) != 0;
        }
    });
    white
}

fn text_of(kernel: &mut Kernel) -> Vec<String> {
    kernel
        .described_active()
        .iter()
        .map(|part| format!("{:?} {:?} {:?}", part.role, part.label, part.area))
        .collect()
}

#[test]
fn the_lab_opens_and_its_rows_can_be_tapped() {
    lab::set_support(lab::Support::Uc8179);
    let mut kernel = kernel(PRO);
    kernel.open_screen(Instant(100), lab::SCREEN, Params::new());
    kernel.render(&mut |_, _| {});
    assert_eq!(kernel.active_screen(), Some(&lab::SCREEN));
    shot(&mut kernel, "lab-0-opened");
    for line in text_of(&mut kernel) {
        eprintln!("{line}");
    }
    eprintln!("waveform: {:?}", kernel.settings().text("x4.lab.waveform"));
}

#[test]
fn tapping_the_waveform_row_chooses_the_direct_drive_and_it_stays() {
    lab::set_support(lab::Support::Uc8179);
    let mut kernel = kernel(PRO);
    kernel.open_screen(Instant(100), lab::SCREEN, Params::new());
    kernel.render(&mut |_, _| {});
    // The first row, "Waveform": shown the panel's own way (partial), not
    // with what is tried. The panel's own, at a temperature, then direct.
    let effects = tap(&mut kernel, 1_000, 240, 406);
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(paperpaw_platform::RefreshMode::Partial)
    );
    assert_eq!(
        kernel.settings().text("x4.lab.waveform"),
        "x4.lab.waveform.temperature"
    );
    tap(&mut kernel, 1_500, 240, 406);
    eprintln!(
        "after one tap: {:?}",
        kernel.settings().text("x4.lab.waveform")
    );
    shot(&mut kernel, "lab-1-tapped");
    // Time passes: is it still so?
    kernel.tick(Instant(3_000));
    kernel.render(&mut |_, _| {});
    eprintln!("later: {:?}", kernel.settings().text("x4.lab.waveform"));
    assert_eq!(
        kernel.settings().text("x4.lab.waveform"),
        "x4.lab.waveform.direct"
    );
    assert!(
        lab::experiment(kernel.active_screen(), kernel.settings()).is_some(),
        "the lab asks for the direct drive"
    );
}

#[test]
fn the_buttons_of_the_x4_do_the_same() {
    lab::set_support(lab::Support::Uc8179);
    let mut kernel = kernel(BUTTONS);
    kernel.open_screen(Instant(100), lab::SCREEN, Params::new());
    kernel.render(&mut |_, _| {});
    // The focus starts on the first row: right changes it.
    press(&mut kernel, 1_000, "right");
    eprintln!(
        "after right: {:?}",
        kernel.settings().text("x4.lab.waveform")
    );
    shot(&mut kernel, "lab-2-buttons");
}

#[test]
fn a_tap_on_the_picture_flips_it_with_a_fast_refresh() {
    lab::set_support(lab::Support::Uc8179);
    let mut kernel = kernel(PRO);
    kernel.open_screen(Instant(100), lab::SCREEN, Params::new());
    kernel.render(&mut |_, _| {});
    tap(&mut kernel, 900, 240, 406);
    tap(&mut kernel, 1_000, 240, 406);
    // The first stripe of the picture is black; flipped it is white.
    assert!(!white(&mut kernel, 5, 200));
    let effects = tap(&mut kernel, 2_000, 240, 200);
    assert!(white(&mut kernel, 5, 200), "the picture was flipped");
    let (area, mode) = effects.refresh().expect("a refresh");
    eprintln!("flip: {area:?} {mode:?}");
    assert_eq!(mode, paperpaw_platform::RefreshMode::Fast);
    assert!(area.height < 400, "only the picture and its line: {area:?}");
    shot(&mut kernel, "lab-3-flipped");
}

#[test]
fn the_rows_of_a_direct_drive_on_a_uc8179_fit_and_page() {
    lab::set_support(lab::Support::Uc8179);
    let mut kernel = kernel(PRO);
    kernel.open_screen(Instant(100), lab::SCREEN, Params::new());
    kernel.render(&mut |_, _| {});
    tap(&mut kernel, 900, 240, 406);
    tap(&mut kernel, 1_000, 240, 406);
    shot(&mut kernel, "lab-4-direct");
    // Frames: the second row; its + is at the right.
    let before = kernel.settings().int("x4.lab.frames");
    tap(&mut kernel, 2_000, 440, 470);
    eprintln!(
        "frames {before} -> {}",
        kernel.settings().int("x4.lab.frames")
    );
    assert_eq!(kernel.settings().int("x4.lab.frames"), before + 1);
    shot(&mut kernel, "lab-5-plus");
}
