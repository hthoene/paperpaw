//! The first-run guide as in Beta 3 (#472): the language chosen on its first
//! page changes the whole page at once, its title and the key bar's *Skip*
//! too; the focus stays on the card an answer was chosen in; the sample of
//! the text size is in the book's font. On buttons only, no answer keeps the
//! focus: the arrows leave each page's card for *Next* (#476).

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
    // The focus to the card, and English chosen there with Select.
    run_script(&mut device, "left,wait:300,confirm,wait:1500").unwrap();
    let after = labels(&mut device);
    assert!(shows(&after, "Language"), "the title: {after:?}");
    assert!(shows(&after, "Skip"), "the key bar: {after:?}");
    assert!(
        !shows(&after, "Sprache") && !shows(&after, "Später"),
        "{after:?}"
    );
    // The focus stayed on the card: Select chooses German again.
    run_script(&mut device, "confirm,wait:1500").unwrap();
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

/// The title of the page shown: the first text drawn.
fn title(device: &mut Device) -> String {
    labels(device).into_iter().next().unwrap_or_default()
}

/// The devices with buttons only: left and right are their only arrows.
const BUTTONS_ONLY: [&str; 3] = ["xteink-x4", "xteink-x4-classic", "xteink-x3"];

#[test]
fn on_buttons_only_the_arrows_leave_the_language_chosen() {
    for profile in BUTTONS_ONLY {
        let mut device = fresh(profile, "[core]\nlanguage = de\n[help]\nshow = false\n");
        // To the card, English and German again chosen with Select.
        run_script(&mut device, "left,wait:300,confirm,wait:1500").unwrap();
        assert_eq!(title(&mut device), "Language", "{profile}");
        run_script(&mut device, "confirm,wait:1500").unwrap();
        assert_eq!(title(&mut device), "Sprache", "{profile}");
        // The arrow on to *Next* (it chose a side and kept the focus on the
        // card: no way on), Select turns the page.
        run_script(&mut device, "right,wait:300,confirm,wait:1500").unwrap();
        assert_eq!(screen(&mut device), "setup.guide", "{profile}");
        assert_eq!(title(&mut device), "Tastenbelegung", "{profile}");
        // And from the card back to *Skip*: Back on the first page asks.
        let mut device = fresh(profile, "[core]\nlanguage = de\n[help]\nshow = false\n");
        run_script(&mut device, "left,wait:300,left,wait:300,confirm,wait:1500").unwrap();
        assert_ne!(
            screen(&mut device),
            "setup.guide",
            "{profile}: the skip question"
        );
    }
}

/// A third language, French: the language is chosen from a list then.
fn with_french(config: &str) -> MemoryStorage {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let french = std::fs::read_to_string(root.join("tools/plugin-files/languages/fr.ini")).unwrap();
    MemoryStorage::new()
        .with("paperpaw.ini", config)
        .with(
            "plugins/language-fr/plugin.ini",
            "[plugin]\nid = language-fr\nname = French\nversion = 1.0.0\nformats = 4\n",
        )
        .with(
            "plugins/language-fr/i18n/fr.ppcat",
            paperpaw_i18n::Catalog::compile(&french).unwrap(),
        )
}

#[test]
fn the_list_of_languages_is_left_with_a_choice_or_back_and_the_arrows_go_on() {
    for (leave, language) in [("right,wait:300,confirm", None), ("back", Some("Sprache"))] {
        let files = with_french("[core]\nlanguage = de\n[help]\nshow = false\n");
        paperpaw_emulator::budget::override_limit(None);
        let profile = profiles::find("xteink-x4").unwrap();
        let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
        device.start();
        run_script(&mut device, "wait:3000,left,wait:300,confirm,wait:1500").unwrap();
        assert_eq!(screen(&mut device), "settings.choose");
        run_script(&mut device, &format!("{leave},wait:1500")).unwrap();
        assert_eq!(screen(&mut device), "setup.guide", "{leave}");
        let chosen = title(&mut device);
        if let Some(language) = language {
            assert_eq!(chosen, language, "Back keeps the language");
        }
        run_script(&mut device, "right,wait:300,confirm,wait:1500").unwrap();
        assert_eq!(screen(&mut device), "setup.guide", "{leave}");
        assert_ne!(title(&mut device), chosen, "{leave}: the next page");
    }
}

#[test]
fn every_page_is_entered_left_and_gone_on_from_with_the_buttons() {
    let config = "[core]\nlanguage = en\n[help]\nshow = false\n";
    // The titles of the pages, Next to Next.
    let mut device = fresh("xteink-x4", config);
    let mut titles = vec![title(&mut device)];
    while screen(&mut device) == "setup.guide" && titles.len() < 20 {
        run_script(&mut device, "confirm,wait:1000").unwrap();
        titles.push(title(&mut device));
    }
    assert!(titles.len() > 4, "{titles:?}");
    // From page 1 on (the first page's Back asks to skip): to the card,
    // Select (a list, a screen, a number changed), Back out of it (one key,
    // one step), the arrow to *Next*, Select: the next page.
    for page in 1..titles.len() - 2 {
        let mut device = fresh("xteink-x4", config);
        for _ in 0..page {
            run_script(&mut device, "confirm,wait:1000").unwrap();
        }
        assert_eq!(title(&mut device), titles[page]);
        run_script(
            &mut device,
            "left,wait:300,confirm,wait:1500,back,wait:1500,right,wait:300,confirm,wait:1500",
        )
        .unwrap();
        assert_eq!(screen(&mut device), "setup.guide", "{}", titles[page]);
        assert_eq!(
            title(&mut device),
            titles[page + 1],
            "from {}",
            titles[page]
        );
    }
}
