//! Font and hyphenation records of a native plugin: published with the rest,
//! all or nothing, and gone with the plugin.
use super::*;

/// The batch of the resource mode `res`: the screen and
/// 1 the three kinds; 2 a face with no family; 3 a language twice.
pub(super) fn batch(
    res: u8,
    screen: wire::Record<'static>,
    bytes: &mut [u8],
) -> Result<usize, i32> {
    let face = |family| {
        wire::Record::ResourceFontFace(wire::ResourceFontFace {
            name: "serif",
            path: "fonts/serif.ppfp",
            family,
            style: 1,
            weight: 0,
            role: "core.font-ui",
            priority: 5,
        })
    };
    let extension = wire::Record::ResourceFontExtension(wire::ResourceFontExtension {
        name: "cyrillic",
        path: "fonts/cyrillic.ppfp",
        extends: "factory.serif",
    });
    let table = |language| {
        wire::Record::ResourceHyphenation(wire::ResourceHyphenation {
            language,
            path: "hyphenation/de.pphp",
        })
    };
    match res {
        1 => wire::records_encode(&[screen, face("Serif"), extension, table("de")], bytes),
        2 => wire::records_encode(&[screen, face("")], bytes),
        _ => wire::records_encode(&[screen, table("de"), table("DE")], bytes),
    }
}

const FOLDER: Option<&str> = Some("plugins/factory");

fn fresh() -> crate::Kernel {
    reset(NORMAL);
    DESTROYED.with(|value| value.set(0));
    crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap()
}

fn nothing_published(kernel: &crate::Kernel) {
    let shared = &kernel.shared;
    assert!(!shared.registry.screens.contains_key("factory.screen"));
    assert!(!shared.fonts.contains("factory.serif"));
    assert!(!shared.fonts.contains("factory.cyrillic"));
    assert!(shared.hyphenation.card_table("de").is_none());
    assert!(shared.native_instances.find("factory").is_none());
}

#[test]
fn fonts_and_hyphenation_are_published_and_go_with_the_plugin() {
    let mut kernel = fresh();
    install_resources(&mut kernel, 1, FOLDER).unwrap();
    let shared = &kernel.shared;
    assert!(shared.fonts.contains("factory.serif"));
    assert!(shared.fonts.contains("factory.cyrillic"));
    let table = shared.hyphenation.card_table("de").unwrap();
    assert_eq!(table.plugin(), "factory");
    assert!(kernel.unload_plugin(&Id::new_static("factory")));
    nothing_published(&kernel);
}

#[test]
fn files_without_a_folder_refuse_the_whole_plugin() {
    let mut kernel = fresh();
    let result = install_resources(&mut kernel, 1, None);
    assert!(
        matches!(result, Err(Error::Status(host_glue::ERR_UNSUPPORTED))),
        "{result:?}"
    );
    nothing_published(&kernel);
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn a_face_with_no_family_is_published_outside_any_family() {
    let mut kernel = fresh();
    install_resources(&mut kernel, 2, FOLDER).unwrap();
    let shared = &kernel.shared;
    assert!(shared.fonts.contains("factory.serif"));
    assert!(!shared.fonts.families().contains(&""));
    assert!(shared.fonts.families().is_empty());
    assert!(kernel.unload_plugin(&Id::new_static("factory")));
    nothing_published(&kernel);
}

#[test]
fn a_language_twice_refuses_the_whole_plugin() {
    let mut kernel = fresh();
    let result = install_resources(&mut kernel, 3, FOLDER);
    assert!(
        matches!(result, Err(Error::Status(host_glue::ERR_INVALID))),
        "{result:?}"
    );
    nothing_published(&kernel);
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn a_font_id_already_taken_refuses_the_whole_plugin_and_its_tables() {
    let mut kernel = fresh();
    kernel
        .shared
        .fonts
        .add_face(paperpaw_sdk::FontFace::new(
            Id::new_static("factory.serif"),
            "other/serif.ppfp",
            "Other",
            false,
            false,
        ))
        .unwrap();
    let result = install_resources(&mut kernel, 1, FOLDER);
    assert!(result.is_err());
    assert!(kernel.shared.hyphenation.card_table("de").is_none());
    assert!(!kernel.shared.fonts.contains("factory.cyrillic"));
    assert!(
        !kernel
            .shared
            .registry
            .screens
            .contains_key("factory.screen")
    );
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn every_allocation_of_an_install_with_resources_refuses_whole() {
    let mut reached_success = false;
    for allocation in 1..600 {
        let mut kernel = fresh();
        INSTALL_FAIL_NTH.with(|value| value.set(allocation));
        let result = install_resources(&mut kernel, 1, FOLDER);
        arm_failure(0);
        INSTALL_FAIL_NTH.with(|value| value.set(0));
        if result.is_ok() {
            reached_success = true;
            let shared = &kernel.shared;
            assert!(shared.fonts.contains("factory.serif"));
            assert!(shared.fonts.contains("factory.cyrillic"));
            assert!(shared.hyphenation.card_table("de").is_some());
            break;
        }
        nothing_published(&kernel);
        assert_eq!(DESTROYED.with(Cell::get), 1, "{allocation}");
    }
    assert!(
        reached_success,
        "the fault walk reached a fully admitted owner"
    );
}
