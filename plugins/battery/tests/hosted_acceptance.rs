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
    run.keys(&["back"]);
    assert_ne!(run.top().as_deref(), Some("battery.screen"), "back leaves");
    // An unknown screen is an error naming it, never a quiet success.
    let refused = run.open("battery.nothing").map(|_| ()).unwrap_err();
    assert!(refused.contains("battery.nothing"), "{refused}");
}

#[test]
fn a_blank_frame_is_not_the_golden() {
    let mut run = start();
    run.open("battery.screen").unwrap();
    let error = run
        .frame()
        .matches_golden(&golden("empty-unknown-en"))
        .unwrap_err();
    assert!(error.contains("pixels differ"), "{error}");
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
    let folder = std::env::temp_dir().join(format!("paperpaw-kit-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    // SAFETY: this test is the only one of its process that sets the variable.
    unsafe { std::env::set_var("PAPERPAW_ACCEPTANCE_EVIDENCE", &folder) };
    let mut gray = vec![255_u8; 8 * 4];
    gray[9] = 0;
    let png = folder.join("tiny.png");
    {
        let file = std::fs::File::create(&png).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 8, 4);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&gray)
            .unwrap();
    }
    let frame = Frame {
        width: 8,
        height: 4,
        gray: gray.clone(),
    };
    assert_eq!(frame.matches_golden(&png), Ok(()));
    let mut off = frame.clone();
    off.gray[9] = 1;
    let error = off.matches_golden(&png).unwrap_err();
    assert!(error.contains("1 pixels differ"), "{error}");
    assert!(folder.join("tiny.actual.png").is_file() && folder.join("tiny.diff.png").is_file());
    let wrong_size = Frame {
        width: 4,
        height: 8,
        gray,
    };
    assert!(wrong_size.matches_golden(&png).unwrap_err().contains("8x4"));
    assert!(frame.matches_golden(&folder.join("missing.png")).is_err());
    let _ = std::fs::remove_dir_all(&folder);
}
