//! The first-run guide as in Beta 3 (#472): the language chosen on its first
//! page changes the whole page at once, its title and the key bar's *Skip*
//! too; the focus stays on the card an answer was chosen in; the sample of
//! the text size is in the book's font.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

/// A first start on `profile` with `config` in `paperpaw.ini`: the guide shows.
fn fresh(profile: &str, config: &str) -> Device {
    let files = MemoryStorage::new().with("paperpaw.ini", config);
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find(profile).unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    run_script(&mut device, "wait:3000").unwrap();
    assert_eq!(screen(&mut device), "setup.guide");
    device
}

fn screen(device: &mut Device) -> String {
    let mut screen = String::new();
    device.call(|kernel, _| {
        screen = kernel
            .active_screen()
            .map(ToString::to_string)
            .unwrap_or_default();
        paperpaw_core::Effects::default()
    });
    screen
}

/// What the screen shows: each part's label and width.
fn parts(device: &mut Device) -> Vec<(String, u16)> {
    let mut parts = Vec::new();
    device.call(|kernel, _| {
        parts = kernel
            .describe()
            .into_iter()
            .map(|p| (p.label, p.area.width))
            .collect();
        paperpaw_core::Effects::default()
    });
    parts
}

fn labels(device: &mut Device) -> Vec<String> {
    parts(device).into_iter().map(|(label, _)| label).collect()
}

fn shows(labels: &[String], text: &str) -> bool {
    labels.iter().any(|l| l == text)
}

#[test]
fn the_language_chosen_changes_the_title_and_the_key_bar_at_once() {
    let mut device = fresh("xteink-x4", "[core]\nlanguage = de\n[help]\nshow = false\n");
    let before = labels(&mut device);
    assert!(
        shows(&before, "Sprache") && shows(&before, "Später"),
        "{before:?}"
    );
    // The focus to the card, and English (the first option) chosen there.
    run_script(&mut device, "left,wait:300,left,wait:1500").unwrap();
    let after = labels(&mut device);
    assert!(shows(&after, "Language"), "the title: {after:?}");
    assert!(shows(&after, "Skip"), "the key bar: {after:?}");
    assert!(
        !shows(&after, "Sprache") && !shows(&after, "Später"),
        "{after:?}"
    );
    // The focus stayed on the card: the other side chooses German again.
    run_script(&mut device, "right,wait:1500").unwrap();
    let again = labels(&mut device);
    assert!(
        shows(&again, "Sprache") && shows(&again, "Später"),
        "{again:?}"
    );
}

#[test]
fn back_from_the_list_of_a_choice_the_focus_is_on_its_card() {
    let mut device = fresh("xteink-x4", "[help]\nshow = false\n");
    // The button layout's page, its card, its list: the second option.
    run_script(
        &mut device,
        "confirm,wait:800,left,wait:300,confirm,wait:1000,right,wait:300,confirm,wait:1500",
    )
    .unwrap();
    assert_eq!(screen(&mut device), "setup.guide");
    assert!(shows(&labels(&mut device), "Two buttons ›"));
    // Select on the card opens its list again, it does not turn the page.
    run_script(&mut device, "confirm,wait:1000").unwrap();
    assert_eq!(screen(&mut device), "settings.choose");
}

#[test]
fn the_sample_of_the_text_size_is_in_the_reading_font() {
    // Literata for book text, built in here; the UI font is another.
    let mut device = fresh("boox-go-7", "[core]\nlanguage = en\n[help]\nshow = false\n");
    run_script(&mut device, "tap-on:Next,wait:800").unwrap();
    let shown = parts(&mut device);
    let points: f32 = shown
        .iter()
        .find_map(|(label, _)| label.strip_suffix(" pt")?.parse().ok())
        .expect("the size shown");
    let sample = "Reading is a quiet joy.";
    let width = shown
        .iter()
        .find_map(|(label, width)| (label == sample).then_some(*width))
        .expect("the sample");
    let mut widths = (0.0, 0.0);
    device.call(|kernel, _| {
        let size = points * f32::from(kernel.device().display.dpi) / 72.0;
        let fonts = kernel.fonts();
        let measure = |role: &str| fonts.role(role).unwrap().measure(size, sample);
        widths = (measure("core.font-reading"), measure("core.font-ui"));
        paperpaw_core::Effects::default()
    });
    let (reading, ui) = widths;
    assert!((reading - ui).abs() > 4.0, "the fonts differ: {widths:?}");
    assert!(
        (f32::from(width) - reading.ceil()).abs() <= 1.0,
        "drawn {width} px wide, {reading} in the reading font, {ui} in the UI font"
    );
}
