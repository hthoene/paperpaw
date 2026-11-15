//! The font key `widget:<id>` (#262): the font the reader chose for a
//! widget, by its own setting, a placement's or the one for all widgets.
#![allow(clippy::unwrap_used, reason = "fixture failures fail the test")]

use super::tests::{bytes, get, scoped};
use super::*;
use crate::font_refresh::widget_fonts::{ALL, BOOK, GLOBAL, MENUS, MOST_KEPT};
use crate::native_host::HostResources;
use alloc::borrow::Cow;
use alloc::rc::Rc;
use alloc::vec::Vec;
use paperpaw_platform::Instant;
use paperpaw_sdk::{FontFace, Id, MemoryStorage, SettingKind, SettingSpec, SettingValue};

/// The families on the card and what an `a` of each advances at 10 pixels,
/// the only size their packs have.
const FAMILIES: [(&str, &str, &str, f32); 5] = [
    ("t.slab", "t/slab.ppfp", "Slab", 5.0),
    ("t.hand", "t/hand.ppfp", "Hand", 7.0),
    ("t.wide", "t/wide.ppfp", "Wide", 8.0),
    ("t.thin", "t/thin.ppfp", "Thin", 2.0),
    ("t.tall", "t/tall.ppfp", "Tall", 3.0),
];

/// The widgets that have a font setting of their own.
const WIDGETS: [&str; 5] = ["t.clock", "t.day", "t.moon", "t.note", "t.quote"];

fn pack(advance: f32) -> Vec<u8> {
    let mut pack = paperpaw_text::PackBuilder::new();
    pack.add_strike(
        10.0,
        paperpaw_text::LineMetrics::new(8.0, 2.0, 12.0),
        alloc::vec![('a', paperpaw_text::Glyph::empty(advance))],
    );
    pack.build()
}

/// A kernel whose card has [`FAMILIES`], with a font setting for each of
/// [`WIDGETS`] as the host gives them.
fn kernel() -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    // The menus' font: the first font there is, bound to no role.
    let menus = paperpaw_sdk::Font::from_data(pack(4.0), &[]).unwrap();
    let menus = paperpaw_sdk::FontSpec::new(Id::new_static("t.menus"), menus);
    kernel.shared.fonts.add(menus).unwrap();
    let mut card = MemoryStorage::new();
    for (id, path, family, advance) in FAMILIES {
        card = card.with(path, pack(advance));
        let face = FontFace::new(Id::new_static(id), path, family, false, false);
        kernel.shared.fonts.add_face(face).unwrap();
    }
    kernel.shared.storage = Rc::new(card);
    kernel.offer_font_choices();
    for widget in WIDGETS {
        let mut choices: Vec<Cow<'static, str>> =
            alloc::vec![ALL.into(), MENUS.into(), BOOK.into()];
        choices.extend(FAMILIES.iter().map(|family| Cow::Borrowed(family.2)));
        let id = alloc::format!("{widget}.font");
        let spec = SettingSpec::new(
            Id::parse(&id).unwrap(),
            "core.setting.font",
            SettingKind::Choice(choices.into()),
            SettingValue::Text(ALL.into()),
            paperpaw_sdk::builtin::groups::OTHER,
        );
        kernel.shared.settings.define(spec).unwrap();
    }
    kernel
}

fn set(kernel: &mut crate::Kernel, id: &str, value: &str) {
    kernel.set_setting(Instant(0), id, value).unwrap();
}

/// The width of `aa` at `size` in the font of `handle`.
fn width_at(scope: &EnvironmentScope, owner: &OwnerResources, handle: u32, size: f32) -> f32 {
    let mut width = -91.0;
    // SAFETY: initialized text and an exclusive scalar output.
    let status = unsafe { measure(scope, owner, handle, size, bytes(b"aa"), &mut width) };
    assert_eq!(status, OK);
    width
}

/// The width of `aa` at 10 pixels in the font of `key`, acquired now.
fn width(kernel: &crate::Kernel, key: &str) -> f32 {
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    let mut width = 0.0;
    scoped(kernel, |scope| {
        width = width_at(scope, &owner, get(scope, &owner, key), 10.0);
    });
    width
}

#[test]
fn a_widget_is_set_in_the_menus_font_until_a_family_is_chosen_for_all() {
    let mut kernel = kernel();
    let menus = width(&kernel, "role:ui");
    assert_eq!(menus, 8.0);
    assert_eq!(width(&kernel, "widget:t.clock"), menus);
    // A widget without a font setting of its own follows the one for all.
    assert_eq!(width(&kernel, "widget:other.thing"), menus);

    set(&mut kernel, GLOBAL, "Slab");
    assert_eq!(width(&kernel, "widget:t.clock"), 10.0);
    assert_eq!(width(&kernel, "widget:t.clock@2"), 10.0);
    assert_eq!(width(&kernel, "widget:other.thing"), 10.0);
    // Only the widgets: the menus stay in theirs.
    assert_eq!(width(&kernel, "role:ui"), menus);

    set(&mut kernel, GLOBAL, MENUS);
    assert_eq!(width(&kernel, "widget:t.clock"), menus);
    assert!(kernel.shared.fonts.get("t.slab").is_none(), "closed again");
}

#[test]
fn a_widget_s_own_font_rules_and_a_placement_s_before_it() {
    let mut kernel = kernel();
    let menus = width(&kernel, "role:ui");
    set(&mut kernel, GLOBAL, "Slab");
    set(&mut kernel, "t.clock.font", "Hand");
    assert_eq!(width(&kernel, "widget:t.clock"), 14.0);
    assert_eq!(width(&kernel, "widget:t.clock@2"), 14.0);
    assert_eq!(width(&kernel, "widget:t.day"), 10.0);
    // One placement of the widget in another family, and one in the menus'.
    set(&mut kernel, "t.clock@2.font", "Wide");
    set(&mut kernel, "t.clock@3.font", MENUS);
    assert_eq!(width(&kernel, "widget:t.clock@2"), 16.0);
    assert_eq!(width(&kernel, "widget:t.clock@3"), menus);
    assert_eq!(width(&kernel, "widget:t.clock"), 14.0);
    // Back to "as all widgets".
    set(&mut kernel, "t.clock.font", ALL);
    assert_eq!(width(&kernel, "widget:t.clock"), 10.0);
    assert_eq!(width(&kernel, "widget:t.clock@2"), 16.0);
}

#[test]
fn the_book_s_font_is_the_reading_role() {
    let mut kernel = kernel();
    set(&mut kernel, "core.reading-font", "Hand");
    assert_eq!(width(&kernel, "role:reading"), 14.0);
    set(&mut kernel, "t.day.font", BOOK);
    assert_eq!(width(&kernel, "widget:t.day"), 14.0);
    set(&mut kernel, GLOBAL, BOOK);
    assert_eq!(width(&kernel, "widget:t.clock"), 14.0);
}

#[test]
fn text_larger_than_the_family_s_pack_has_is_set_in_the_menus_font() {
    let mut kernel = kernel();
    set(&mut kernel, GLOBAL, "Slab");
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    scoped(&kernel, |scope| {
        let menus = get(scope, &owner, "role:ui");
        let widget = get(scope, &owner, "widget:t.clock");
        // The pack's one size is 10 pixels: it sets text up to 2 pixels
        // larger, and the menus' font what is larger than that.
        assert_eq!(width_at(scope, &owner, widget, 10.0), 10.0);
        assert_ne!(
            width_at(scope, &owner, widget, 11.5),
            width_at(scope, &owner, menus, 11.5)
        );
        for size in [12.0, 20.0, 48.0] {
            assert_eq!(
                width_at(scope, &owner, widget, size),
                width_at(scope, &owner, menus, size),
                "{size}"
            );
            let mut of_widget = [0.0; 3];
            let mut of_menus = [0.0; 3];
            for (handle, values) in [(widget, &mut of_widget), (menus, &mut of_menus)] {
                let out = values.as_mut_ptr();
                // SAFETY: disjoint exclusive scalar outputs.
                let status =
                    unsafe { metrics(scope, &owner, handle, size, out, out.add(1), out.add(2)) };
                assert_eq!(status, OK);
            }
            assert_eq!(of_widget, of_menus, "{size}");
        }
    });
}

#[test]
fn a_key_whose_family_was_closed_since_draws_in_the_menus_font() {
    let mut kernel = kernel();
    set(&mut kernel, GLOBAL, "Slab");
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    let mut handle = 0;
    scoped(&kernel, |scope| {
        handle = get(scope, &owner, "widget:t.clock");
        assert_eq!(width_at(scope, &owner, handle, 10.0), 10.0);
    });
    set(&mut kernel, GLOBAL, MENUS);
    assert!(kernel.shared.fonts.get("t.slab").is_none());
    scoped(&kernel, |scope| {
        let menus = get(scope, &owner, "role:ui");
        assert_eq!(
            width_at(scope, &owner, handle, 10.0),
            width_at(scope, &owner, menus, 10.0)
        );
    });
}

#[test]
fn no_more_families_than_the_limit_are_open_and_the_rest_show_in_the_menus_font() {
    let mut kernel = kernel();
    let menus = width(&kernel, "role:ui");
    let open = |kernel: &crate::Kernel| {
        FAMILIES
            .iter()
            .filter(|family| kernel.shared.fonts.get(family.0).is_some())
            .count()
    };
    assert_eq!(open(&kernel), 0);
    // Two widgets in one family take one place.
    set(&mut kernel, "t.clock.font", "Slab");
    set(&mut kernel, "t.day.font", "Slab");
    assert_eq!(open(&kernel), 1);
    let _ = kernel.take_warnings();
    for (widget, family) in WIDGETS.iter().zip(FAMILIES) {
        set(&mut kernel, &alloc::format!("{widget}.font"), family.2);
    }
    assert!(FAMILIES.len() > MOST_KEPT);
    assert_eq!(open(&kernel), MOST_KEPT);
    let warnings = kernel.take_warnings();
    assert!(
        warnings
            .iter()
            .any(|warning| warning.message.contains("more fonts chosen for widgets")),
        "{warnings:?}"
    );
    let shown: Vec<f32> = WIDGETS
        .iter()
        .map(|widget| width(&kernel, &alloc::format!("widget:{widget}")))
        .collect();
    let in_menus = shown.iter().filter(|width| **width == menus).count();
    assert_eq!(in_menus, FAMILIES.len() - MOST_KEPT, "{shown:?}");
    // One widget back in the menus' font makes room for another's family.
    set(&mut kernel, "t.clock.font", MENUS);
    set(&mut kernel, "t.day.font", MENUS);
    assert_eq!(open(&kernel), MOST_KEPT);
    let shown = WIDGETS[2..]
        .iter()
        .map(|widget| width(&kernel, &alloc::format!("widget:{widget}")));
    assert!(
        shown.clone().all(|width| width != menus),
        "{:?}",
        shown.collect::<Vec<_>>()
    );
}
