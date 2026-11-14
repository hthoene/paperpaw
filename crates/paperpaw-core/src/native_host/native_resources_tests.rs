//! Font and hyphenation records of a native plugin: published with the rest,
//! all or nothing, and gone with the plugin.
use super::*;
use alloc::string::String;

/// The batch of the resource mode `res`: the screen and
/// 1 the three kinds; 2 a face with no family; 3 a language twice; 4 an
/// action that is taken; 5 a weight that differs; 6 a font file; 7 a name twice; 8 a slot of its own and
/// three offers for slots.
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
            // The family's regular face (a family is offered where it has
            // one); bold where the weight is to differ from the bit.
            style: u32::from(res == 5),
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
    let offer = |slot, priority| {
        wire::Record::SlotOffer(wire::SlotOffer {
            slot,
            screen: "factory.screen",
            priority,
        })
    };
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
        // A slot of its own and screens offered for slots (the offer of an
        // existing slot, of its own, and one more at another priority).
        8 => wire::records_encode(
            &[
                screen,
                wire::Record::SlotSpec(wire::SlotSpec {
                    name: "own",
                    title: "factory.own",
                }),
                offer("core.translate", 4),
                offer("factory.own", 0),
                offer("core.lookup", -2),
            ],
            bytes,
        ),
        // Offers whose priority the contract allows and the kernel's i16 does
        // not: counted as the nearest end.
        9 => wire::records_encode(
            &[
                screen,
                offer("core.translate", 100_000),
                offer("core.lookup", -100_000),
            ],
            bytes,
        ),
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
    // The face is a font file (a pack has no weight): where only packs are
    // read it is left out, and nothing is told of its weight.
    let outlines = paperpaw_sdk::text::OUTLINES;
    assert_eq!(kernel.shared.fonts.contains("factory.serif"), outlines);
    let told = kernel.take_warnings();
    let note = told
        .iter()
        .find(|warning| warning.message.starts_with("factory.serif: the weight"));
    assert_eq!(note.is_some(), outlines);
    if let Some(note) = note {
        assert!(
            matches!(&note.source, crate::WarningSource::Plugin(id) if id.as_str() == "factory")
        );
    }
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
    // A face left out proposes no font for its role.
    assert_eq!(
        kernel.shared.fonts.proposals("core.font-ui").count(),
        usize::from(outlines)
    );
}

#[test]
fn an_empty_folder_is_no_folder_and_a_trailing_slash_is_the_same_folder() {
    let mut kernel = fresh();
    for folder in [Some(""), Some("/")] {
        let result = install_resources(&mut kernel, 1, folder);
        assert!(
            matches!(result, Err(Error::Status(host_glue::ERR_UNSUPPORTED))),
            "{folder:?}: {result:?}"
        );
        nothing_published(&kernel);
    }
    install_resources(&mut kernel, 1, Some("plugins/factory/")).unwrap();
    // The file is named from the folder once (it is not on the test card).
    kernel
        .shared
        .settings
        .set(
            "core.reading-font",
            paperpaw_sdk::SettingValue::Text("Serif".into()),
        )
        .unwrap();
    kernel.apply_reading_font();
    let told = kernel.take_warnings();
    assert!(
        told.iter().any(|warning| warning.message
            == "plugins/factory/fonts/serif.ppfp: font could not be loaded"),
        "{told:?}"
    );
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

/// A reader of every format, and the format `t.txt` they read.
fn two_readers_of_a_format(kernel: &mut crate::Kernel) {
    use alloc::borrow::Cow;
    use paperpaw_sdk::FormatSpec;
    use paperpaw_sdk::readers::ReaderSpec;
    let format = FormatSpec::new(
        Id::new_static("t.txt"),
        "TXT",
        vec![Cow::Borrowed("txt")],
        Box::new(|_, _, _| Err(paperpaw_sdk::document::DocumentError::UnsupportedFormat)),
    );
    kernel.shared.formats.add(format).unwrap();
    for reader in ["t.a", "t.b"] {
        let screen = Id::new_static("t.screen");
        kernel
            .shared
            .readers
            .add(ReaderSpec::new(Id::new_static(reader), screen));
    }
}

#[test]
fn every_allocation_of_the_choices_refresh_keeps_what_was_or_makes_what_is_new() {
    use alloc::borrow::Cow;
    use alloc::string::ToString;
    use paperpaw_sdk::{SettingKind, SettingSpec, SettingValue};
    const SETTINGS: [&str; 5] = [
        "core.reading-font",
        "core.status-font",
        "core.reading-app",
        "t.mode",
        "core.reading-app.t.txt",
    ];
    // Offered by a plugin: `t.b`, among the choices of its own setting.
    static MODES: [&str; 1] = ["t.a"];
    let prepare = |kernel: &mut crate::Kernel| {
        admit_resources(kernel, 1, FOLDER).unwrap();
        two_readers_of_a_format(kernel);
        let mode = SettingSpec::new(
            Id::new_static("t.mode"),
            "t.mode",
            SettingKind::Choice((&MODES[..]).into()),
            SettingValue::Text("t.a".into()),
            paperpaw_sdk::builtin::groups::READING_BEHAVIOUR,
        );
        kernel.shared.settings.define(mode).unwrap();
        kernel.plugin_choices.push((
            Id::new_static("t"),
            Id::new_static("t.mode"),
            Cow::Borrowed("t.b"),
        ));
        // The user's choice is made before the refresh, which is what fails.
        for id in &SETTINGS[..2] {
            let value = SettingValue::Text("Serif".into());
            let _ = kernel.shared.settings.set(id, value);
        }
    };
    let refresh = |kernel: &mut crate::Kernel| {
        kernel.offer_choices();
        kernel.apply_reading_font();
        kernel.apply_status_font();
    };
    let state = |kernel: &crate::Kernel| -> Vec<Option<Vec<String>>> {
        SETTINGS
            .iter()
            .map(|id| match &kernel.shared.settings.spec(id)?.kind {
                SettingKind::Choice(choices) => Some(choices.iter().map(String::from).collect()),
                _ => None,
            })
            .collect()
    };
    // What the refresh makes where every allocation is granted.
    let mut twin = fresh();
    prepare(&mut twin);
    refresh(&mut twin);
    let made = state(&twin);
    assert!(made[0].as_ref().unwrap().iter().any(|f| f == "Serif"));
    assert_eq!(
        made[3].as_deref(),
        Some(&["t.a".to_string(), "t.b".to_string()][..])
    );
    assert!(made[4].is_some(), "the setting for the format is made");
    assert!(twin.plugin_choices.is_empty());
    let (mut kept, mut done) = (0, 0);
    for allocation in 1..600 {
        let mut kernel = fresh();
        prepare(&mut kernel);
        let before = state(&kernel);
        arm_failure(allocation);
        refresh(&mut kernel);
        arm_failure(0);
        let after = state(&kernel);
        for (at, id) in SETTINGS.into_iter().enumerate() {
            assert!(
                after[at] == before[at] || after[at] == made[at],
                "{id} at {allocation}: {:?}",
                after[at]
            );
        }
        // A choice not added is still waiting for the next refresh.
        assert_eq!(
            kernel.plugin_choices.len(),
            usize::from(after[3] == before[3]),
            "at {allocation}"
        );
        // The family in use is the one before or the one chosen, whole.
        assert!(matches!(kernel.shared.fonts.family(), None | Some("Serif")));
        kept += usize::from(after != made);
        done += usize::from(after == made);
        if after == made && allocation > 300 {
            break;
        }
    }
    assert!(kept > 0, "a refusal was reached");
    assert!(done > 0, "a refresh was made");
}

/// A font pack of a few letters.
fn pack_bytes() -> Vec<u8> {
    use paperpaw_text::{Glyph, LineMetrics, PackBuilder};
    let mut builder = PackBuilder::new();
    let glyphs = "ab".chars().map(|c| (c, Glyph::empty(5.0))).collect();
    builder.add_strike(10.0, LineMetrics::new(8.0, 2.0, 12.0), glyphs);
    builder.build()
}

/// A kernel whose card has a regular and a bold pack of `t`, the bold one
/// of `bold_family`, and which has chosen the family `Serif`.
fn kernel_with_packs(bold_family: &'static str) -> crate::Kernel {
    let mut kernel = fresh();
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with("t/regular.ppfp", pack_bytes())
        .with("t/bold.ppfp", pack_bytes());
    kernel.shared.storage = alloc::rc::Rc::new(storage);
    let face = |id, path, family, bold| {
        paperpaw_sdk::FontFace::new(Id::new_static(id), path, family, bold, false)
    };
    let fonts = &mut kernel.shared.fonts;
    fonts
        .add_face(face("t.regular", "t/regular.ppfp", "Serif", false))
        .unwrap();
    fonts
        .add_face(face("t.bold", "t/bold.ppfp", bold_family, true))
        .unwrap();
    fonts.try_set_family(Some("Serif")).unwrap();
    kernel
}

#[test]
fn a_pack_that_cannot_be_opened_closes_the_packs_the_call_opened() {
    // Both packs open where every allocation is granted.
    let mut clean = kernel_with_packs("Serif");
    let storage = clean.shared.storage.clone();
    assert_eq!(clean.shared.fonts.try_load(&storage), Ok(Vec::new()));
    assert!(clean.shared.fonts.get("t.regular").is_some());
    assert!(clean.shared.fonts.get("t.bold").is_some());
    // How many allocations the call makes up to the end of the regular
    // pack: the bold one is of another family, so it is not opened.
    let mut twin = kernel_with_packs("Sans");
    let storage = twin.shared.storage.clone();
    FAIL_NTH.with(|left| left.set(1_000_000));
    let loaded = twin.shared.fonts.try_load(&storage);
    let used = 1_000_000 - FAIL_NTH.with(Cell::get);
    arm_failure(0);
    assert_eq!(loaded, Ok(Vec::new()));
    assert!(twin.shared.fonts.get("t.regular").is_some());
    assert!(twin.shared.fonts.get("t.bold").is_none());
    // The next one, the bold pack's, is refused: the regular pack, opened
    // by this call, is closed again.
    let mut kernel = kernel_with_packs("Serif");
    let storage = kernel.shared.storage.clone();
    arm_failure(used + 1);
    let result = kernel.shared.fonts.try_load(&storage);
    arm_failure(0);
    assert_eq!(result, Err(paperpaw_sdk::FontError::OutOfMemory));
    assert!(kernel.shared.fonts.get("t.regular").is_none());
    assert!(kernel.shared.fonts.get("t.bold").is_none());
    // And it opens at the next call.
    assert_eq!(kernel.shared.fonts.try_load(&storage), Ok(Vec::new()));
    assert!(kernel.shared.fonts.get("t.regular").is_some());
    assert!(kernel.shared.fonts.get("t.bold").is_some());
}

#[test]
fn hosted_owners_share_one_host_table_and_cost_a_bounded_heap() {
    let mut kernel = kernel(0);
    let live = || crate::native_host::shell::tests::HEAP_LIVE.with(Cell::get);
    let before = live();
    let owners = ["b1", "b2", "b3", "b4", "b5", "b6", "b7", "b8"];
    for owner in owners {
        kernel
            .register_native(
                host_named(owner, 0),
                &device_depth(&kernel),
                |_, _, _, _| {},
            )
            .unwrap();
    }
    let each = (live() - before) / owners.len();
    std::println!("host heap per registered native owner (x86_64, with its test plugin): {each} B");
    // x86_64 pointers are twice the X4's; abi/heap-word.md section 9 has the
    // riscv32 sizes. Bounded here so that growth shows in review: with one
    // sorted list per owner (not a table per kind) the cost does not grow
    // with the kinds.
    assert!(each <= 2_048, "{each} B per owner");
    // Every owner's `make` was handed the same static table, not a copy.
    let tables = TABLES.with(|tables| tables.borrow().clone());
    assert!(tables.len() > owners.len());
    assert!(tables.windows(2).all(|pair| pair[0] == pair[1]));
}
