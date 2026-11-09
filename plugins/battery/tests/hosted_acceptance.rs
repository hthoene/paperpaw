//! Battery's screen through the hosted acceptance kit (`plugins/test-support/acceptance.rs`):
//! the linked native plugin on a real kernel, its screen opened, keys pressed, the frame
//! compared with the interface-6 golden (`testdata/screens`).
//!
//! What runs today: the screen opens on the kernel's own stack and a key script moves it. What
//! does not: `NativeScreen` draws with an `Env`-only scope, so the plugin's font and usage
//! services answer `State` and the frame stays blank (H1/H3, the real `CoreFrame` for a native
//! screen's draw). `a_blank_frame_is_not_the_golden` pins that, and
//! `the_screen_looks_as_it_did` is the acceptance case that turns on when it is fixed.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
#[path = "../../test-support/acceptance.rs"]
mod acceptance;
#[path = "../../test-support/hosted.rs"]
mod hosted;

use acceptance::{Frame, Session};
use std::path::{Path, PathBuf};

const SERVICES: &[&str] = &[
    "core.plugin",
    "core.plugin-usage",
    "core.usage",
    "core.work-note",
];

fn start() -> Session {
    Session::start(
        hosted::Setup::x4(),
        paperpaw_plugin_battery::NATIVE_ENTRY.entry(),
        "battery",
        SERVICES,
    )
    .expect("installed")
}

/// A folder of this test's own (tests of one process run in parallel; none sets the environment).
fn scratch(test: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("paperpaw-kit-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn write_png(path: &Path, size: (u32, u32), color: png::ColorType, depth: png::BitDepth, data: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size.0, size.1);
    encoder.set_color(color);
    encoder.set_depth(depth);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(data)
        .unwrap();
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/screens")
        .join(format!("{name}.png"))
}

#[test]
fn the_screen_opens_on_the_real_stack_and_back_leaves_it() {
    let mut run = start();
    run.open("battery.screen").expect("the screen opens");
    assert_eq!(run.top().as_deref(), Some("battery.screen"));
    run.keys(&["back"]).expect("the X4 has back");
    assert_ne!(run.top().as_deref(), Some("battery.screen"), "back leaves");
    // A button the device lacks is an error naming it, and presses nothing.
    let lacking = run.keys(&["menu"]).map(|_| ()).unwrap_err();
    assert!(lacking.contains("menu"), "{lacking}");
    // An unknown screen is an error naming it, never a quiet success.
    let refused = run.open("battery.nothing").map(|_| ()).unwrap_err();
    assert!(refused.contains("battery.nothing"), "{refused}");
}

#[test]
fn a_blank_frame_is_not_the_golden() {
    let folder = scratch("blank");
    let mut run = start();
    run.open("battery.screen").unwrap();
    let error = run
        .frame()
        .matches_golden_in(&golden("empty-unknown-en"), &folder)
        .unwrap_err();
    assert!(error.contains("pixels differ"), "{error}");
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
#[ignore = "BLOCKED H1/H3: a native screen draws with an Env-only scope, so battery's font and usage services answer State and the frame stays blank"]
fn the_screen_looks_as_it_did() {
    let mut run = start();
    run.open("battery.screen").unwrap();
    run.frame()
        .matches_golden(&golden("empty-unknown-en"))
        .unwrap();
}

/// The kit's own comparison: exact, with the evidence of a difference written out.
#[test]
fn the_comparison_is_exact_and_leaves_evidence() {
    let folder = scratch("comparison");
    let mut gray = vec![255_u8; 8 * 4];
    gray[9] = 0;
    let png = folder.join("tiny.png");
    write_png(&png, (8, 4), png::ColorType::Grayscale, png::BitDepth::Eight, &gray);
    let frame = Frame {
        width: 8,
        height: 4,
        gray: gray.clone(),
    };
    assert_eq!(frame.matches_golden_in(&png, &folder), Ok(()));
    let mut off = frame.clone();
    off.gray[9] = 1;
    let error = off.matches_golden_in(&png, &folder).unwrap_err();
    assert!(error.contains("1 pixels differ"), "{error}");
    assert!(folder.join("tiny.actual.png").is_file() && folder.join("tiny.diff.png").is_file());
    let wrong_size = Frame {
        width: 4,
        height: 8,
        gray,
    };
    assert!(wrong_size
            .matches_golden_in(&png, &folder)
            .unwrap_err().contains("8x4"));
    assert!(frame
        .matches_golden_in(&folder.join("missing.png"), &folder)
        .is_err());
    let _ = std::fs::remove_dir_all(&folder);
}

/// Goldens other than 8-bit gray read as the frame's grays: a deeper or shallower gray, a
/// palette and a color one (rounded as `Pixel::luma` does), never a false pass.
#[test]
fn goldens_of_other_depths_and_colors_compare_as_grays() {
    let folder = scratch("depths");
    let frame = Frame {
        width: 4,
        height: 1,
        gray: vec![0, 85, 170, 255],
    };
    // Two bits a pixel: 0, 1, 2, 3 widen to 0, 85, 170, 255.
    let two = folder.join("two.png");
    write_png(&two, (4, 1), png::ColorType::Grayscale, png::BitDepth::Two, &[0b0001_1011]);
    assert_eq!(frame.matches_golden_in(&two, &folder), Ok(()));
    let mut off = frame.clone();
    off.gray[1] = 84;
    assert!(off.matches_golden_in(&two, &folder).is_err());
    // Sixteen bits: the high byte counts.
    let deep = folder.join("deep.png");
    write_png(
        &deep,
        (4, 1),
        png::ColorType::Grayscale,
        png::BitDepth::Sixteen,
        &[0, 0, 85, 85, 170, 170, 255, 255],
    );
    assert_eq!(frame.matches_golden_in(&deep, &folder), Ok(()));
    // Color: (0, 1, 0) is luma 1 (0.587 rounds up), (255, 255, 255) is 255.
    let color = folder.join("color.png");
    write_png(
        &color,
        (2, 1),
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &[0, 1, 0, 255, 255, 255],
    );
    let two_px = Frame {
        width: 2,
        height: 1,
        gray: vec![1, 255],
    };
    assert_eq!(two_px.matches_golden_in(&color, &folder), Ok(()));
    // An alpha channel is refused, not read as gray.
    let alpha = folder.join("alpha.png");
    write_png(
        &alpha,
        (2, 1),
        png::ColorType::GrayscaleAlpha,
        png::BitDepth::Eight,
        &[1, 255, 255, 255],
    );
    assert!(two_px.matches_golden_in(&alpha, &folder).unwrap_err().contains("unexpected"));
    let _ = std::fs::remove_dir_all(&folder);
}
