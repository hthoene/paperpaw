//! Font and hyphenation records of a native plugin: published with the rest,
//! all or nothing, and gone with the plugin.
use super::*;

/// The batch of the resource mode `res`: the screen and
/// 1 the three kinds; 2 a face with no family; 3 a language twice; 4 an
/// action that is taken; 5 a weight that differs; 6 a font file; 7 a name twice.
pub(super) fn batch(
    res: u8,
    screen: wire::Record<'static>,
    bytes: &mut [u8],
) -> Result<usize, i32> {
    let face = |family, path, weight| {
        wire::Record::ResourceFontFace(wire::ResourceFontFace {
            name: "serif",
            path,
            family,
            style: 1,
            weight,
            role: "core.font-ui",
            priority: 5,
        })
    };
    let pack = "fonts/serif.ppfp";
    let extension = |name| {
        wire::Record::ResourceFontExtension(wire::ResourceFontExtension {
            name,
            path: "fonts/cyrillic.ppfp",
            extends: "factory.serif",
        })
    };
    let table = |language| {
        wire::Record::ResourceHyphenation(wire::ResourceHyphenation {
            language,
            path: "hyphenation/de.pphp",
        })
    };
    let action = wire::Record::ActionSpec(wire::ActionSpec {
        name: "action",
        title: "factory.action",
        token: 92,
        flags: 0,
    });
    match res {
        1 => wire::records_encode(
            &[
                screen,
                face("Serif", pack, 0),
                extension("cyrillic"),
                table("de"),
            ],
            bytes,
        ),
        2 => wire::records_encode(&[screen, face("", pack, 0)], bytes),
        3 => wire::records_encode(&[screen, table("de"), table("DE")], bytes),
        // An action the merge finds taken.
        4 => wire::records_encode(
            &[screen, action, face("Serif", pack, 0), table("de")],
            bytes,
        ),
        // A weight that the bold bit does not say (a pack has none).
        5 => wire::records_encode(&[screen, face("Serif", "fonts/serif.ttf", 400)], bytes),
        // A font file, which a device that reads packs only leaves out.
        6 => wire::records_encode(&[screen, face("Serif", "fonts/serif.ttf", 0)], bytes),
        // A face and an extension of the same name.
        _ => wire::records_encode(&[screen, face("Serif", pack, 0), extension("serif")], bytes),
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
    // Bound to its role all the same (loaded once a file is there).
    let bound: Vec<_> = shared.fonts.proposals("core.font-ui").collect();
    assert_eq!(bound.len(), 1);
    assert_eq!(bound[0].0.as_str(), "factory.serif");
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
fn a_merge_refusal_takes_the_tables_out_again() {
    let mut kernel = fresh();
    kernel.shared.actions.insert(paperpaw_sdk::ActionSpec::new(
        Id::new_static("factory.action"),
        "other",
    ));
    let result = install_resources(&mut kernel, 4, FOLDER);
    assert!(result.is_err(), "{result:?}");
    nothing_published(&kernel);
    assert_eq!(DESTROYED.with(Cell::get), 1);
}

#[test]
fn a_face_and_an_extension_of_one_name_refuse_the_whole_plugin() {
    let mut kernel = fresh();
    let result = install_resources(&mut kernel, 7, FOLDER);
    assert!(result.is_err(), "{result:?}");
    nothing_published(&kernel);
}

#[test]
fn a_face_is_proposed_for_its_role_with_its_priority() {
    let mut kernel = fresh();
    install_resources(&mut kernel, 1, FOLDER).unwrap();
    let proposals: Vec<_> = kernel.shared.fonts.proposals("core.font-ui").collect();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].0.as_str(), "factory.serif");
    assert_eq!(proposals[0].1, paperpaw_sdk::Priority(5));
    assert!(kernel.unload_plugin(&Id::new_static("factory")));
    assert_eq!(kernel.shared.fonts.proposals("core.font-ui").count(), 0);
}

#[test]
fn a_weight_that_differs_from_the_style_bits_is_noted_for_the_plugin() {
    let mut kernel = fresh();
    install_resources(&mut kernel, 5, FOLDER).unwrap();
    assert!(kernel.shared.fonts.contains("factory.serif"));
    let told = kernel.take_warnings();
    let note = told
        .iter()
        .find(|warning| warning.message.starts_with("factory.serif: the weight"))
        .expect("the weight is noted");
    assert!(matches!(&note.source, crate::WarningSource::Plugin(id) if id.as_str() == "factory"));
}

#[test]
fn a_font_file_is_left_out_where_only_packs_are_read() {
    let mut kernel = fresh();
    install_resources(&mut kernel, 6, FOLDER).unwrap();
    let outlines = paperpaw_sdk::text::OUTLINES;
    assert_eq!(kernel.shared.fonts.contains("factory.serif"), outlines);
    let told = kernel.take_warnings();
    let noted = told.iter().any(|warning| {
        warning
            .message
            .ends_with("only reads fonts made for it (.ppfp)")
    });
    assert_eq!(noted, !outlines);
}

#[test]
fn a_native_install_offers_the_family_it_brings() {
    let mut kernel = fresh();
    install_resources(&mut kernel, 1, FOLDER).unwrap();
    let spec = kernel.shared.settings.spec("core.reading-font").unwrap();
    assert!(
        spec.kind
            .accepts(&paperpaw_sdk::SettingValue::Text("Serif".into()))
    );
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
        let result = admit_resources(&mut kernel, 1, FOLDER);
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
