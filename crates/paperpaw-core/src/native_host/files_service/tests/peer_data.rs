//! Root 1 shows a running data plugin's folder, read only (`files.md` 1.1):
//! `plugins/<folder>/…` of a data plugin reads, that of any other plugin stays
//! hidden, and every change below `plugins/` is refused.
use super::*;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID, ERR_NOT_FOUND};

/// A card with a data plugin `owls` (running), a code plugin `weather` and a
/// data plugin `gone` that is not running.
fn cards() -> (Rc<Card>, Rc<TestHost>) {
    let (card, host) = fixture();
    card.file("plugins/owls/owl.png", 10);
    card.file("plugins/owls/sub/two.png", 20);
    card.file("plugins/weather/plugin.ppn", 30);
    card.file("plugins/gone/x.png", 40);
    host.data_folders.borrow_mut().push("owls");
    (card, host)
}

#[test]
fn a_data_plugins_files_read_on_root_1() {
    let (_card, host) = cards();
    let reply = call(&host, 2, path(1, "plugins/owls/owl.png")).unwrap();
    assert!(!reply.is_empty());
    assert!(call(&host, 2, path(1, "plugins/owls")).is_ok());
    let names = list_all(&host, 1, "plugins/owls").unwrap();
    assert_eq!(names, ["owl.png", "sub"]);
    // The folder name is matched case folded, as the card compares names.
    assert!(call(&host, 2, path(1, "plugins/OWLS/sub/two.png")).is_ok());
    assert_eq!(
        call(&host, 2, path(1, "plugins/owls/none.png")),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn the_folder_of_a_code_plugin_or_one_not_running_stays_hidden() {
    let (_card, host) = cards();
    for hidden in [
        "plugins/weather/plugin.ppn",
        "plugins/weather",
        "plugins/gone/x.png",
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
    // A data plugin that stops running (switched off, removed) is hidden again.
    host.data_folders.borrow_mut().clear();
    assert_eq!(
        call(&host, 2, path(1, "plugins/owls/owl.png")),
        Err(ERR_INVALID)
    );
}

#[test]
fn plugins_itself_stays_closed_and_the_card_top_does_not_list_it() {
    let (card, host) = cards();
    card.file("Books/a.txt", 1);
    // Nothing needs a listing of `plugins/`: it stays `ERR_INVALID`, as does
    // any change of it.
    assert_eq!(call(&host, 2, path(1, "plugins")), Err(ERR_INVALID));
    assert_eq!(
        call(&host, 1, list_record(1, "Plugins", 0)),
        Err(ERR_INVALID)
    );
    assert_eq!(call(&host, 4, path(1, "plugins/new")), Err(ERR_INVALID));
    assert_eq!(
        call(&host, 5, list_record(1, "plugins", 0)),
        Err(ERR_INVALID)
    );
    let top = list_all(&host, 1, "").unwrap();
    assert_eq!(top, ["Books"], "`plugins` is not in the card's listing");
}

#[test]
fn nothing_below_a_data_plugins_folder_changes() {
    let (card, host) = cards();
    assert_eq!(call(&host, 4, path(1, "plugins/owls/new")), Err(ERR_DENIED));
    assert_eq!(
        call(&host, 5, list_record(1, "plugins/owls/sub", 0)),
        Err(ERR_DENIED)
    );
    assert_eq!(
        call(
            &host,
            6,
            move_record(1, "plugins/owls/owl.png", 1, "Books/o.png", 0)
        ),
        Err(ERR_DENIED)
    );
    card.file("Books/in.png", 1);
    assert_eq!(
        call(
            &host,
            6,
            move_record(1, "Books/in.png", 1, "plugins/owls/in.png", 0)
        ),
        Err(ERR_DENIED)
    );
    assert!(card.has("plugins/owls/owl.png") && !card.has("plugins/owls/in.png"));
}

#[test]
fn a_path_cannot_leave_the_folder_or_borrow_a_neighbours_name() {
    let (_card, host) = cards();
    for bad in [
        "plugins/owls/../weather/plugin.ppn",
        "plugins/owls/..",
        "plugins/owl/owl.png",
        "plugins/owls2/owl.png",
        "plugins//owls/owl.png",
    ] {
        let status = call(&host, 2, path(1, bad)).unwrap_err();
        assert!(
            status == ERR_INVALID || status == ERR_NOT_FOUND,
            "{bad}: {status}"
        );
    }
}

#[test]
fn without_uses_storage_root_1_stays_closed() {
    let (_card, host) = cards();
    let setup = Setup {
        storage: true,
        ..NORMAL
    };
    // The feature gate is in front of everything (`granted`: the card view).
    let denied = Setup {
        granted: false,
        ..setup
    };
    assert_eq!(
        call_with(&host, denied, 2, path(1, "plugins/owls/owl.png")),
        Err(ERR_DENIED)
    );
}
