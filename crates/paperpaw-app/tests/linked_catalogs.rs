//! A linked native plugin has its translation catalogs: its texts are found
//! (a native plugin built into the firmware brings them built in).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
#![cfg(feature = "native-linked")]

/// The catalogs of plugin `id` as its folder has them, each parsed.
fn catalogs_of(id: &str) -> Vec<paperpaw_i18n::Catalog> {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../plugins")
        .join(id)
        .join("i18n");
    ["en.ini", "de.ini"]
        .iter()
        .filter_map(|name| std::fs::read_to_string(folder.join(name)).ok())
        .map(|source| paperpaw_i18n::Catalog::parse(source).unwrap())
        .collect()
}

/// The tables alone, as the kernel has them, in `language`.
fn tables(language: &str) -> paperpaw_i18n::Translator {
    let mut translator = paperpaw_i18n::Translator::new();
    translator.set_language(language);
    for table in paperpaw_app::NATIVE_MESSAGES {
        translator.built_messages(table);
    }
    translator
}

/// The program carries the linked plugins' catalogs without their keys
/// (#258): every key of every one of their files reads through the tables
/// what the file says, in both languages, and no file is kept as a text
/// beside them.
#[test]
fn every_text_of_a_linked_plugin_reads_the_same_without_its_keys() {
    assert_eq!(paperpaw_app::NATIVE_MESSAGES.len(), 2);
    let (english, german) = (tables("en"), tables("de"));
    let (mut checked, mut with_texts) = (0, 0);
    for (id, _) in paperpaw_app::NATIVE_PLUGINS {
        let kept = paperpaw_app::native_catalogs(id);
        assert!(
            kept.is_empty(),
            "{id}: {} catalogs kept as texts",
            kept.len()
        );
        let catalogs = catalogs_of(id);
        with_texts += usize::from(!catalogs.is_empty());
        for catalog in catalogs {
            let translator = match catalog.language() {
                "en" => &english,
                _ => &german,
            };
            for key in catalog.keys() {
                assert_eq!(translator.get(key), catalog.get(key), "{id} {key}");
                checked += 1;
            }
        }
    }
    for id in ["keyboard", "menu", "settings", "help", "lock"] {
        assert!(
            paperpaw_app::NATIVE_PLUGINS
                .iter()
                .any(|(linked, _)| *linked == id),
            "{id}"
        );
    }
    assert!(with_texts > 10, "{with_texts} plugins with texts");
    assert!(checked > 1_000, "{checked} texts checked");
    assert!(paperpaw_app::native_catalogs("no-such-plugin").is_empty());
    // No key's text is in the program for them: the tables hold hashes.
    for table in paperpaw_app::NATIVE_MESSAGES {
        assert_eq!(table.keys.len(), table.entries.len());
        assert!(
            !table.values.contains("keyboard.done"),
            "{}",
            table.language
        );
    }
}

/// Only theirs: a plugin this build does not link (the X4's firmware has
/// none of the store's; a build with the store's plugins in their
/// interface-6 form registers those, which add their catalogs themselves)
/// has no text in the tables.
#[test]
fn a_plugin_the_build_does_not_link_has_no_text_in_the_tables() {
    let english = tables("en");
    let plugins = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    let mut others = 0;
    for entry in std::fs::read_dir(plugins).unwrap().flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        if paperpaw_app::NATIVE_PLUGINS
            .iter()
            .any(|(linked, _)| *linked == id)
        {
            continue;
        }
        assert!(paperpaw_app::native_catalogs(&id).is_empty(), "{id}");
        let catalogs = catalogs_of(&id);
        others += usize::from(!catalogs.is_empty());
        for catalog in catalogs {
            for key in catalog.keys() {
                assert_eq!(english.get(key), None, "{id} {key}");
            }
        }
    }
    assert!(others > 0, "no plugin with texts is left out");
}

#[test]
fn the_kernel_finds_the_texts_of_a_linked_plugin() {
    let mut kernel = paperpaw_core::Kernel::builder(device()).build().unwrap();
    assert_eq!(kernel.translator().tr("keyboard.done"), "keyboard.done");
    paperpaw_app::add_native_catalogs(&mut kernel);
    assert_ne!(kernel.translator().tr("keyboard.done"), "keyboard.done");
    // A second call adds nothing (a shell that calls it after
    // `register_native_plugins` did).
    let before = kernel.translator().tr("keyboard.done").to_string();
    paperpaw_app::add_native_catalogs(&mut kernel);
    assert_eq!(kernel.translator().tr("keyboard.done"), before);
}

fn device() -> paperpaw_platform::DeviceInfo {
    use paperpaw_platform::{
        DeviceInfo, DisplayInfo, InputCaps, MemoryClass, PixelFormat, Rotation,
    };
    DeviceInfo::new(
        "test-x4",
        "Test X4",
        DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
        InputCaps::new(true, false, false, &["back"]),
        MemoryClass::Small,
    )
}

/// Through the kernel, as a screen asks: once the tables are added every
/// key of every linked plugin's files reads what the file says, in English
/// and in German, and none did before.
#[test]
fn the_kernel_reads_every_text_of_the_linked_plugins_as_their_files_say() {
    let mut checked = 0;
    for language in ["en", "de"] {
        let mut kernel = paperpaw_core::Kernel::builder(device())
            .setting_default_text("core.language", language)
            .build()
            .unwrap();
        assert_eq!(kernel.translator().language(), language);
        let files: Vec<_> = paperpaw_app::NATIVE_PLUGINS
            .iter()
            .flat_map(|(id, _)| catalogs_of(id).into_iter().map(move |file| (id, file)))
            .filter(|(_, file)| file.language() == language)
            .collect();
        for (id, file) in &files {
            for key in file.keys() {
                assert_eq!(kernel.translator().get(key), None, "{id} {key}");
            }
        }
        paperpaw_app::add_native_catalogs(&mut kernel);
        for (id, file) in &files {
            for key in file.keys() {
                assert_eq!(kernel.translator().get(key), file.get(key), "{id} {key}");
                checked += 1;
            }
        }
    }
    assert!(checked > 1_000, "{checked} texts checked");
}

/// A plugin on the card that says a linked plugin's text anew is heard, as
/// it was while those texts were catalogs: the card's plugins are read
/// when the kernel is built, before a shell adds the tables.
#[test]
fn a_card_plugin_s_text_wins_over_the_tables() {
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with("plugins/mine/plugin.ini", "[plugin]\nname = Mine\n")
        .with(
            "plugins/mine/i18n/en.ini",
            "[meta]\nlanguage = en\nname = English\n\n[messages]\nkeyboard.done = Card done\n",
        );
    let mut kernel = paperpaw_core::Kernel::builder(device())
        .storage(storage)
        .build()
        .unwrap();
    assert_eq!(kernel.translator().tr("keyboard.done"), "Card done");
    paperpaw_app::add_native_catalogs(&mut kernel);
    assert_eq!(kernel.translator().tr("keyboard.done"), "Card done");
    kernel.start(paperpaw_platform::Instant(0));
    assert_eq!(kernel.translator().tr("keyboard.done"), "Card done");
    // What the card does not say is the tables'.
    assert_eq!(kernel.translator().tr("keyboard.space"), "Space");
}
