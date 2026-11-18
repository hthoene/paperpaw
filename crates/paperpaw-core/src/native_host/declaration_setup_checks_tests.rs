//! A setup step's name against the plugin's other names (`registry.md`
//! section 6): one item-state push never sets two things.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use super::*;
use crate::native_host::{
    raw_declarations::RawDeclarations,
    registrations::{
        ActionSpec, MenuSection, REGISTRATION_MAX_BYTES, ScreenSpec, SettingGroup, SettingSpec,
        SetupStep, records_decode, records_encode,
    },
    staged::Declarations,
};
use alloc::vec;
use paperpaw_native::host_glue::ERR_INVALID;

fn step(name: &str) -> Record<'_> {
    Record::SetupStep(SetupStep {
        name,
        title: "Step",
        help: "",
        order: 0,
        page_kind: 1,
        page: "reader.page",
        applies_key: "",
        applies_value: "",
        need: 0,
        capability: 0,
        flags: 1,
    })
}

fn screen(name: &str) -> Record<'_> {
    Record::ScreenSpec(ScreenSpec {
        name,
        title: "Screen",
        token: 1,
        flags: 0,
    })
}

fn action(name: &str) -> Record<'_> {
    Record::ActionSpec(ActionSpec {
        name,
        title: "Action",
        token: 2,
        flags: 0,
    })
}

fn section(name: &str) -> Record<'_> {
    Record::MenuSection(MenuSection {
        name,
        title: "Section",
        menu: "core.settings",
        order: 0,
    })
}

fn setting(name: &str) -> [Record<'_>; 2] {
    [
        Record::SettingGroup(SettingGroup {
            name: "g",
            title: "Group",
            order: 0,
            parent: "",
            page: "",
        }),
        Record::SettingSpec(SettingSpec {
            name,
            kind: 0,
            flags: 0,
            group: "reader.g",
            title: "Setting",
            help: "",
            default: &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
            min: 0,
            max: 0,
            choices: "",
        }),
    ]
}

/// The plugin `reader`'s batches, committed and checked at the end: kept as
/// owned copies, or (`image`) as offsets into one image that holds them all.
fn finish_as(batches: &[&[Record<'_>]], image: bool) -> Result<(), i32> {
    let encoded: alloc::vec::Vec<alloc::vec::Vec<u8>> = batches
        .iter()
        .map(|records| {
            let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
            let len = records_encode(records, &mut bytes).unwrap();
            bytes.truncate(len);
            bytes
        })
        .collect();
    let whole: alloc::vec::Vec<u8> = encoded.concat();
    let mut raw = RawDeclarations::default();
    let mut at = 0;
    for bytes in &encoded {
        let (image, wire) = if image {
            (Some(&whole[..]), &whole[at..at + bytes.len()])
        } else {
            (None, &bytes[..])
        };
        at += bytes.len();
        let pending = raw
            .prepare(image, Some(wire), &records_decode(wire).unwrap())
            .unwrap();
        raw.commit(pending);
    }
    raw.sort_index();
    let owned = Declarations::default();
    View {
        raw: &raw,
        owned: &owned,
        image: image.then_some(&whole[..]),
        plugin: "reader",
        refused: Default::default(),
    }
    .finish()
}

/// Both storage paths must answer alike.
fn finish(batches: &[&[Record<'_>]]) -> Result<(), i32> {
    let owned = finish_as(batches, false);
    assert_eq!(finish_as(batches, true), owned, "the image path differs");
    owned
}

#[test]
fn a_step_named_like_nothing_else_is_fine_and_a_name_is_a_step_once() {
    assert_eq!(finish(&[&[step("setup"), screen("page")]]), Ok(()));
    assert_eq!(
        finish(&[&[step("setup")], &[step("setup")]]),
        Err(ERR_INVALID)
    );
    assert_eq!(finish(&[&[step("one"), step("two")]]), Ok(()));
}

#[test]
fn a_step_named_like_a_screen_action_setting_or_menu_section_is_refused() {
    for other in [screen("setup"), action("setup"), section("setup")] {
        // In the same batch or another, before or after the step.
        assert_eq!(finish(&[&[step("setup"), other]]), Err(ERR_INVALID));
        assert_eq!(finish(&[&[other], &[step("setup")]]), Err(ERR_INVALID));
        assert_eq!(finish(&[&[step("setup")], &[other]]), Err(ERR_INVALID));
    }
    let [group, spec] = setting("setup");
    assert_eq!(finish(&[&[step("setup"), group, spec]]), Err(ERR_INVALID));
    // Another name of the same kinds does not clash.
    let [group, spec] = setting("other");
    assert_eq!(
        finish(&[&[
            step("setup"),
            screen("a"),
            action("b"),
            section("c"),
            group,
            spec
        ]]),
        Ok(())
    );
}
