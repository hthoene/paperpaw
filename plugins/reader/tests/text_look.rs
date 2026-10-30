//! How book text is inked, with the real kernel: smooth or black and
//! white (`core.smooth-text`), and how dark (`core.text-contrast`). A page
//! of text without smooth letters asks for no refresh in grays on a panel
//! of black and white; sharp letters have no grays; darker steps ink
//! more; the defaults draw as before.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{DeviceInfo, Instant, PixelFormat};

const BOOK: &str = "books/numbered.txt";

/// The buttons device with a panel of black and white alone, as the X4.
const MONO: DeviceInfo = BUTTONS.with_display(BUTTONS.display.with_format(PixelFormat::Mono1));

fn reading(device: DeviceInfo, config: &str) -> Reading {
    let text = numbered_book(120, 12);
    let mut reading = Reading::new(device, Shared::default(), BOOK, text.as_bytes(), config);
    reading.open();
    reading
}

/// Whether turning a page is shown in grays: the refresh it asks for.
fn turns_in_grays(reading: &mut Reading) -> bool {
    reading.now += 100;
    let mut effects = reading
        .kernel
        .run_action(Instant(reading.now), actions::NEXT_PAGE);
    for _ in 0..100 {
        if effects.refresh().is_some() {
            return effects.grays();
        }
        reading.now += 10;
        effects = reading.kernel.tick(Instant(reading.now));
    }
    panic!("the page turned without a refresh");
}

#[test]
fn text_pages_turn_in_grays_only_with_smooth_text() {
    // Off at first on a panel of black and white.
    assert!(!turns_in_grays(&mut reading(MONO, "")));
    let smooth = "[core]\nsmooth-text = true\n";
    assert!(turns_in_grays(&mut reading(MONO, smooth)));
    // As the reader's own setting was called before.
    let before = "[reader]\nsmooth-text = true\n";
    assert!(turns_in_grays(&mut reading(MONO, before)));
}

/// The grays of the page's text lines (not the status line, whose text
/// is the interface's).
fn text_grays(reading: &mut Reading) -> Vec<u8> {
    let image = reading.snapshot();
    let lines = reading.lines(4);
    assert!(lines.len() > 5, "a page of text");
    lines
        .iter()
        .flat_map(|((top, bottom), _)| image[*top..*bottom].iter().flatten().copied())
        .collect()
}

#[test]
fn sharp_letters_have_no_grays() {
    let between = |grays: &[u8]| grays.iter().filter(|g| !matches!(g, 0 | 255)).count();
    // Smooth at first where the panel has grays.
    let smooth = text_grays(&mut reading(BUTTONS, ""));
    assert!(between(&smooth) > 1000, "{}", between(&smooth));
    let sharp = text_grays(&mut reading(BUTTONS, "[core]\nsmooth-text = false\n"));
    assert_eq!(between(&sharp), 0);
    assert!(sharp.contains(&0));
}

#[test]
fn darker_steps_ink_more_and_normal_is_as_before() {
    let ink = |config: &str| -> u64 {
        let grays = text_grays(&mut reading(BUTTONS, config));
        grays.iter().map(|g| u64::from(255 - g)).sum()
    };
    let step = |level: &str| {
        ink(&format!(
            "[core]\ntext-contrast = core.text-contrast.{level}\n"
        ))
    };
    let inks = ["lighter", "normal", "darker", "darkest"].map(step);
    assert!(inks.windows(2).all(|w| w[0] < w[1]), "{inks:?}");
    assert_eq!(inks[1], ink(""));
    // Without grays too: darker steps draw fuller letters.
    let sharp = |level: &str| {
        ink(&format!(
            "[core]\nsmooth-text = false\ntext-contrast = core.text-contrast.{level}\n"
        ))
    };
    assert!(sharp("lighter") < sharp("normal") && sharp("normal") < sharp("darkest"));
}

#[test]
fn the_default_page_is_drawn_as_before_on_black_and_white() {
    // Sharp letters from the panel's own threshold: the same pixels as
    // smooth ones drawn in black and white by the frame.
    let page = |config: &str| reading(MONO, config).snapshot();
    let before = page("");
    assert_eq!(before, page("[core]\nsmooth-text = true\n"));
    // The contrast still tells: fuller letters.
    let ink = |image: &[Vec<u8>]| image.iter().flatten().filter(|g| **g < 128).count();
    let darkest = page("[core]\ntext-contrast = core.text-contrast.darkest\n");
    assert!(ink(&darkest) > ink(&before));
}
