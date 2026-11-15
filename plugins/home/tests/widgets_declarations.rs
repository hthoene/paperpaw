//! The widgets the native declarations make (the date, the clock and the
//! status line) are those pinned in `tests/pinned/widgets.txt` (written while
//! the interface-6 registration was still compared with them): their ids,
//! titles and categories, the group of settings of each, and each setting's id,
//! kind, default, choices, title and help, in the same order; the status
//! line's four pickers name home's own screen `status-items`.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the plugin read the same file"
)]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_sdk::native::declare::Declarations;

fn batches(
    declare: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>),
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    out
}

#[test]
fn the_widgets_are_the_pinned_ones() {
    let native = declared::native("home", &batches(|d| declarations::widgets(d).unwrap()));
    declared::assert_pinned("widgets", &native);
    // The five widgets (date, clock, status line and the two others) are the
    // `WidgetSpec` records.
    assert_eq!(native.others.len(), 5, "{:?}", native.others);
    let pickers = native
        .extras
        .iter()
        .filter(|e| e.starts_with("status.") && e.contains("|1:status-items|"))
        .count();
    assert_eq!(pickers, 4, "{:?}", native.extras);
}
