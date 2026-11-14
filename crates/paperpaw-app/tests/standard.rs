//! Built-in texts: no index on the heap, the same text through the table
//! and the source. (The probe plugin that reached the reader through the
//! standard ids, and the decorator test, tested the interface-6 reader and
//! went with it, #244.)

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;
use native_activation::BuildNative as _;

use paperpaw_core::Kernel;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::MemoryStorage;

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

struct App {
    kernel: Kernel,
}

impl App {
    fn new() -> Self {
        let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config("[core]\nlanguage = en\n");
        let mut kernel = paperpaw_app::install(builder, fonts()).build_native();
        kernel.start(Instant(0));
        Self { kernel }
    }
}

/// The catalogs built in take their indexes from the program (made by
/// the build script): none is made on the heap, in English or German;
/// the English ones are one catalog whose table holds each plugin's
/// source in two words, and the German ones another.
#[test]
fn translations_keep_no_index_on_the_heap() {
    let tables = paperpaw_app::BUILT_CATALOGS;
    // A table for English and one for German, each of the plugins built in.
    assert_eq!(tables.len(), 2);
    assert!(tables.iter().all(|table| table.catalogs.prints.len() > 20));
    let slots: usize = tables
        .iter()
        .map(|table| table.catalogs.prints.len() * 2 * core::mem::size_of::<usize>())
        .sum();
    let app = App::new();
    assert_eq!(app.kernel.translator().indexes_on_heap(), slots);
    // One English catalog for the plugins built in.
    let english = app
        .kernel
        .translator()
        .catalogs()
        .iter()
        .filter(|c| c.language() == "en")
        .count();
    assert!(english < 5, "{english} English catalogs");
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage).user_config(
        "[core]
language = de
",
    );
    let mut kernel = native_activation::build(paperpaw_app::install(builder, fonts()));
    kernel.start(Instant(0));
    assert_eq!(kernel.translator().language(), "de");
    assert_eq!(kernel.translator().indexes_on_heap(), slots);
    // One German catalog for the plugins built in, as one English.
    let german = kernel
        .translator()
        .catalogs()
        .iter()
        .filter(|c| c.language() == "de")
        .count();
    // Exactly the one built table's: a source left out of it would be a
    // catalog of its own and fail here.
    assert_eq!(german, 1, "{german} German catalogs");
}

/// The `.ini` files in `folder` and below, as the build script finds them.
fn ini_files(folder: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            ini_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "ini") {
            files.push(path);
        }
    }
}

/// Every key of every real catalog a built table holds, English and German,
/// reads the same through the table as through the catalog parsed on its
/// own, and German with English under it reads as the catalogs parsed on
/// their own would, English where German has a gap (review of #1189).
#[test]
fn every_built_in_text_reads_the_same_through_its_table() {
    let app = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = app.join("../..");
    let mut folders = vec![root.join("crates/paperpaw-core/i18n"), app.join("i18n")];
    folders.extend(
        paperpaw_app::CRATES
            .split(',')
            .filter_map(|name| name.strip_prefix("paperpaw-plugin-"))
            .map(|dir| root.join("plugins").join(dir).join("i18n")),
    );
    let mut files = Vec::new();
    for folder in &folders {
        ini_files(folder, &mut files);
    }
    let sources: Vec<&'static str> = files
        .iter()
        .map(|file| &*std::fs::read_to_string(file).unwrap().leak())
        .collect();
    let mut checked = 0;
    let mut all_held = Vec::new();
    for table in paperpaw_app::BUILT_CATALOGS {
        let held: Vec<&'static str> = sources
            .iter()
            .copied()
            .filter(|source| {
                table
                    .catalogs
                    .prints
                    .contains(&paperpaw_i18n::fingerprint(source))
            })
            .collect();
        assert_eq!(
            held.len(),
            table.catalogs.prints.len(),
            "{}",
            table.language
        );
        let mut built = paperpaw_i18n::Translator::new();
        built.set_language(table.language);
        built.built_language(table);
        for source in &held {
            built.add(paperpaw_i18n::Catalog::parse(*source).unwrap());
        }
        // Every source filled its slot: one catalog, the table's.
        assert_eq!(built.catalogs().len(), 1, "{}", table.language);
        for source in &held {
            let catalog = paperpaw_i18n::Catalog::parse(*source).unwrap();
            for key in catalog.keys() {
                assert_eq!(built.get(key), catalog.get(key), "{} {key}", table.language);
                checked += 1;
            }
        }
        all_held.extend(held);
    }
    assert!(checked > 1_000, "{checked} texts checked");
    // German as a German start has it, both tables under it, against every
    // source parsed on its own: a key German lacks reads English's text.
    let mut built = paperpaw_i18n::Translator::new();
    let mut plain = paperpaw_i18n::Translator::new();
    for translator in [&mut built, &mut plain] {
        translator.set_language("de");
    }
    for table in paperpaw_app::BUILT_CATALOGS {
        built.built_language(table);
    }
    for source in &all_held {
        built.add(paperpaw_i18n::Catalog::parse(*source).unwrap());
        plain.add(paperpaw_i18n::Catalog::parse(*source).unwrap());
    }
    let mut gaps = 0;
    for source in &all_held {
        let catalog = paperpaw_i18n::Catalog::parse(*source).unwrap();
        for key in catalog.keys() {
            assert_eq!(built.get(key), plain.get(key), "de {key}");
            let english = catalog.language() == "en";
            if english && built.get(key) == catalog.get(key) {
                gaps += 1;
            }
        }
    }
    std::println!("{checked} texts checked; {gaps} English texts read in German");
}
