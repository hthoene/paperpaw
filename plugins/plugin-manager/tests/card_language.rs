//! The plugin manager with a language from the card (ADR 0033, #208): a
//! long list of plugins must not read the descriptions of all of them in
//! one call (the card's read budget is 16 KiB: what comes after it would
//! be English), only those of the rows shown; the rows keep their height
//! from page to page.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_i18n::{Catalog, MAX_READ};
use paperpaw_platform::Rotation;
use paperpaw_plugin_plugin_manager::{OPEN, PluginManager};
use paperpaw_sdk::builtin::actions::{NEXT_PAGE, PREV_PAGE};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass, MemoryStorage,
    PixelFormat, Plugin, RegisterError, Registrar, Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// How many plugins are listed, and how long each description is.
const PLUGINS: usize = 300;

/// A plugin with nothing to do but be listed.
struct Listed(Id, &'static str);

impl Plugin for Listed {
    fn manifest(&self) -> Manifest {
        Manifest::new(self.0.clone(), self.1, Version::new(0, 1, 0))
            .description("English description.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, _: &mut Registrar) -> Result<(), RegisterError> {
        Ok(())
    }
}

fn id(i: usize) -> String {
    format!("pl{i:03}")
}

/// A language `fr` on the card: the manager's title and a name and a
/// description for every plugin listed: two lines of 100 bytes, in the
/// first half, in the second four lines of 150.
fn card() -> MemoryStorage {
    let mut source = String::from(
        "[meta]\nlanguage = fr\nname = Francais\nplural = one-other\n[messages]\n\
         plugin-manager.title = Gestionnaire\n",
    );
    for i in 0..PLUGINS {
        source.push_str(&format!(
            "{0}.plugin.name = Nom {i:03}\n{0}.plugin.description = Description {i:03} {1}\n",
            id(i),
            "mots de la description ".repeat(if i < PLUGINS / 2 { 4 } else { 6 })
        ));
    }
    MemoryStorage::new()
        .with(
            "plugins/pack/plugin.ini",
            b"[plugin]\nname = French\nformats = 4\n".to_vec(),
        )
        .with(
            "plugins/pack/i18n/fr.ppcat",
            Catalog::compile(&source).unwrap(),
        )
}

fn started() -> Kernel {
    let mut builder = Kernel::builder(DEVICE)
        .storage(card())
        .user_config("[core]\nlanguage = fr\n".to_string())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(PluginManager);
    for i in 0..PLUGINS {
        let id: &'static str = Box::leak(id(i).into_boxed_str());
        builder = builder.plugin(Listed(Id::new_static(id), "English name"));
    }
    let mut kernel = builder.build().unwrap();
    kernel.start(Instant(0));
    kernel
}

fn shown(kernel: &mut Kernel) -> String {
    kernel.render(&mut |_, _| {});
    kernel
        .describe()
        .into_iter()
        .map(|p| p.label)
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The labels of the rows shown, one text.
fn page_of(kernel: &mut Kernel) -> String {
    shown(kernel)
}

/// Down the list a page at a time to its end: every page as shown.
fn forward(kernel: &mut Kernel, at: &mut u64) -> Vec<String> {
    let mut pages = Vec::new();
    loop {
        let page = page_of(kernel);
        // Whatever was read for it fits the budget with room to spare.
        let read = kernel.translator().read_bytes();
        assert!(read < MAX_READ, "{read} bytes read for {page}");
        let last = pages.last() == Some(&page);
        pages.push(page);
        if last || pages.len() > 400 {
            pages.pop();
            return pages;
        }
        *at += 100;
        kernel.run_action(Instant(*at), NEXT_PAGE);
    }
}

fn back_to_the_start(kernel: &mut Kernel, at: &mut u64) {
    for _ in 0..400 {
        *at += 100;
        kernel.run_action(Instant(*at), PREV_PAGE);
    }
}

#[test]
fn opening_a_long_list_reads_the_rows_shown_not_all_descriptions() {
    let mut kernel = started();
    assert_eq!(kernel.translator().language(), "fr");
    kernel.run_action(Instant(100), OPEN);
    let first = page_of(&mut kernel);
    assert!(first.contains("Gestionnaire"), "{first}");
    assert!(!first.contains("English description"), "{first}");
}

#[test]
fn every_page_of_a_long_list_is_in_the_language_of_the_card() {
    let mut kernel = started();
    kernel.run_action(Instant(100), OPEN);
    let mut at = 100;
    let pages = forward(&mut kernel, &mut at);
    assert!(pages.len() > 50, "{} pages", pages.len());
    let all = pages.join(" | ");
    for i in 0..PLUGINS {
        assert!(all.contains(&format!("Description {i:03} ")), "{i} missing");
    }
    assert!(!all.contains("English description"), "{all}");
    assert!(!all.contains("English name"), "{all}");
    // Whole descriptions: none cut short with an ellipsis.
    assert!(!all.contains('…'), "{all}");
}

/// A page with longer notes makes the rows higher from then on, never
/// lower: the pages are the same each time down the list.
#[test]
fn the_pages_of_a_long_list_stay_as_they_are_once_seen() {
    let mut kernel = started();
    kernel.run_action(Instant(100), OPEN);
    let mut at = 100;
    forward(&mut kernel, &mut at);
    back_to_the_start(&mut kernel, &mut at);
    let second = forward(&mut kernel, &mut at);
    back_to_the_start(&mut kernel, &mut at);
    let third = forward(&mut kernel, &mut at);
    assert_eq!(second, third);
}
