//! Root 1 shows a running code plugin's install folder read only, as a data
//! plugin's, but never its code or the host's files in it (`files.md` 1.1;
//! sleep's preset pictures, todo 581).
use super::*;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID};

/// A card with the running code plugin `sleepy` (in the kernel's set of
/// shown folders, as every running plugin installed on the card).
fn cards() -> (Rc<Card>, Rc<TestHost>) {
    let (card, host) = fixture();
    card.file("plugins/sleepy/owl.png", 10);
    card.file("plugins/sleepy/art/moon.png", 20);
    card.file("plugins/sleepy/code.ppn", 30);
    card.file("plugins/sleepy/.install-marks", 5);
    host.data_folders.borrow_mut().push("sleepy");
    (card, host)
}

#[test]
fn a_code_plugins_pictures_read_and_its_code_is_never_shown() {
    let (_card, host) = cards();
    assert!(call(&host, 2, path(1, "plugins/sleepy/owl.png")).is_ok());
    assert!(call(&host, 2, path(1, "plugins/sleepy/art/moon.png")).is_ok());
    // In the storage's order (`list` does not sort).
    let mut names = list_all(&host, 1, "plugins/sleepy").unwrap();
    names.sort();
    assert_eq!(names, ["art", "owl.png"], "the code and the marks left out");
    for hidden in [
        "plugins/sleepy/code.ppn",
        "plugins/SLEEPY/CODE.PPN",
        "plugins/sleepy/.install-marks",
    ] {
        assert_eq!(
            call(&host, 2, path(1, hidden)),
            Err(ERR_INVALID),
            "{hidden}"
        );
        assert_eq!(
            call(&host, 1, list_record(1, hidden, 0)),
            Err(ERR_INVALID),
            "{hidden}"
        );
    }
    // Nothing changes there, the code least of all.
    assert_eq!(
        call(&host, 4, path(1, "plugins/sleepy/new")),
        Err(ERR_DENIED)
    );
    assert_eq!(
        call(
            &host,
            6,
            move_record(1, "plugins/sleepy/code.ppn", 1, "Books/c.ppn", 0)
        ),
        Err(ERR_DENIED)
    );
}

#[test]
fn an_8_3_alias_of_the_code_is_left_out_of_list_and_refused_by_stat() {
    let (card, host) = cards();
    // The card's short name for `code.ppn` (FAT keeps both spellings).
    card.file("plugins/sleepy/CODE~1.PPN", 30);
    let mut names = list_all(&host, 1, "plugins/sleepy").unwrap();
    names.sort();
    assert_eq!(names, ["art", "owl.png"], "the alias left out as the code");
    assert_eq!(
        call(&host, 2, path(1, "plugins/sleepy/CODE~1.PPN")),
        Err(ERR_INVALID)
    );
}

#[test]
fn a_short_name_in_a_folder_without_code_is_shown() {
    let (card, host) = fixture();
    card.file("plugins/sleepy/OWL~1.PNG", 10);
    host.data_folders.borrow_mut().push("sleepy");
    assert_eq!(list_all(&host, 1, "plugins/sleepy").unwrap(), ["OWL~1.PNG"]);
    assert!(call(&host, 2, path(1, "plugins/sleepy/OWL~1.PNG")).is_ok());
}
