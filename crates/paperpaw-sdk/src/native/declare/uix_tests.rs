//! UIX declaration builders preserve caps, flags and latched batch submission.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::native::{Error, ui::HintInput};
use alloc::vec::Vec;
use core::cell::RefCell;

#[test]
fn all_uix_declarations_round_trip_with_private_and_own_metadata() {
    let batches = RefCell::new(Vec::new());
    let mut declarations = Declarations::new(|bytes: &[u8]| {
        batches.borrow_mut().push(bytes.to_vec());
        Ok(())
    });
    declarations
        .input(
            InputDecl::new("press", "Press", InputKind::Input)
                .unwrap()
                .need(crate::MenuNeed::Wifi)
                .unwrap(),
        )
        .unwrap();
    declarations
        .help_hint(
            HelpHintDecl::new("foreign.screen", "Hint", HintInput::Touch)
                .unwrap()
                .zone(paperpaw_platform::Rect::new(0, 0, 1000, 1000))
                .unwrap(),
        )
        .unwrap();
    declarations
        .widget(
            WidgetDecl::new("clock", "Clock", "clock", 7)
                .unwrap()
                .live()
                .private()
                .cells(2, 3),
        )
        .unwrap();
    declarations
        .widget_place(
            WidgetPlaceDecl::new("home", "Home", "fixture.widgets", WidgetLayout::Grid).unwrap(),
        )
        .unwrap();
    declarations
        .status_item(
            StatusItemDecl::new("battery", "Battery", 8)
                .unwrap()
                .private()
                .priority(255),
        )
        .unwrap();
    declarations
        .status_line(
            StatusLineDecl::new("line", "Line", 1000, 9)
                .unwrap()
                .private(),
        )
        .unwrap();
    declarations
        .status_place(
            StatusPlaceDecl::new(
                "place",
                "Place",
                "fixture.left",
                "fixture.center",
                "fixture.right",
            )
            .unwrap(),
        )
        .unwrap();
    declarations.submit().unwrap();
    let batches = batches.borrow();
    let mut tags = Vec::new();
    for bytes in batches.iter() {
        for row in records_wire::records_decode(bytes).unwrap().iter() {
            tags.push(row.tag());
            match row {
                records_wire::Record::WidgetSpec(r) => assert_eq!((r.flags, r.cells), (3, 0x302)),
                records_wire::Record::StatusItem(r) => assert_eq!((r.flags, r.priority), (2, 255)),
                records_wire::Record::StatusLine(r) => assert_eq!(r.flags, 2),
                _ => {}
            }
        }
    }
    assert_eq!(tags, [29, 30, 224, 225, 226, 227, 228]);
}
#[test]
fn builders_refuse_over_caps_without_cutting_text_or_keys() {
    let text = "界".repeat(43);
    assert!(matches!(
        HelpHintDecl::new("fixture.screen", &text, HintInput::Any),
        Err(Error::NoSpace)
    ));
    assert!(matches!(
        WidgetDecl::new(&"x".repeat(97), "Title", "clock", 1),
        Err(Error::NoSpace)
    ));
    assert!(matches!(
        HelpHintDecl::new("fixture.screen", "Hint", HintInput::Any)
            .unwrap()
            .zone(paperpaw_platform::Rect::new(999, 0, 2, 10)),
        Err(Error::Invalid)
    ));
    let keys = core::iter::repeat_n("fixture.widgets", 9)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(matches!(
        WidgetPlaceDecl::new("home", "Home", &keys, WidgetLayout::Grid),
        Err(Error::NoSpace)
    ));
    assert!(matches!(
        WidgetPlaceDecl::new("home", "Home", "fixture.widgets", WidgetLayout::Grid)
            .unwrap()
            .shown_if("fixture.show", ""),
        Err(Error::Invalid)
    ));
}
#[test]
fn screen_flags_include_uix_policy_without_changing_refresh_bit() {
    let batches = RefCell::new(Vec::new());
    let mut declarations = Declarations::new(|bytes: &[u8]| {
        batches.borrow_mut().push(bytes.to_vec());
        Ok(())
    });
    let mut screen = ScreenDecl::new("screen", "Screen", 1)
        .guards()
        .overlay()
        .waits_ready()
        .secret()
        .holds_edges();
    screen.full_refresh_on_open = true;
    declarations.screen(screen).unwrap();
    declarations.submit().unwrap();
    let batches = batches.borrow();
    let rows = records_wire::records_decode(&batches[0]).unwrap();
    assert!(matches!(rows.iter().next(),Some(records_wire::Record::ScreenSpec(r))if r.flags==63));
}
