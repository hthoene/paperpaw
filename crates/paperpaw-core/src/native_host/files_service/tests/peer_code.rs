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
    let names = list_all(&host, 1, "plugins/sleepy").unwrap();
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
