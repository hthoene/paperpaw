//! The book menu's *Open with…*: the kernel's own entry in the build with
//! interface 6's readers; none with the linked native reader
//! (`native-reading`), where the file manager's one-time choice stays.
use super::*;

fn book_menu_has_open_with() -> bool {
    let kernel = Kernel::builder(DEVICE).build().unwrap();
    kernel
        .menus()
        .items(paperpaw_sdk::builtin::menus::BOOK.as_str())
        .iter()
        .any(|item| *item.id == paperpaw_sdk::builtin::actions::OPEN_WITH)
}

#[cfg(not(feature = "native-reading"))]
#[test]
fn the_book_menu_offers_open_with_with_interface_6_readers() {
    assert!(book_menu_has_open_with());
}

#[cfg(feature = "native-reading")]
#[test]
fn the_book_menu_has_no_open_with_with_the_native_reader() {
    assert!(!book_menu_has_open_with());
}
