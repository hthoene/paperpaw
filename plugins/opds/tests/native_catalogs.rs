//! The catalog list and the passwords, without a host: the file as interface 6
//! kept it, a list that cannot be read, the move of old passwords to the
//! secrets, the secret that moves with an address.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
mod fakes;
use fakes::{Mem, MemVault};
use paperpaw_plugin_opds::native_catalogs::{
    self as catalogs, Catalog, MAX_FILE, Vault, max_catalogs, move_passwords, move_secret,
    password, prune,
};
use paperpaw_plugin_opds::native_keep::{Secret, name_of};
use paperpaw_sdk::native::Error;

const V1: &str = include_str!("../testdata/catalogs.txt.v1");

fn label(name: &str) -> Result<String, Error> {
    Ok(format!("Catalog: {name}"))
}

#[test]
fn the_list_of_version_1_is_read_and_written_back_the_same() {
    let list = catalogs::parse(V1, 100).unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].name, "Project Gutenberg");
    assert!(!list[0].sync && !list[0].webdav);
    assert_eq!(list[1].user, "reader");
    assert_eq!(list[1].folder, "Calibre");
    assert!(list[1].sync && !list[1].webdav);
    assert!(list[2].sync && list[2].webdav);
    assert!(list.iter().all(|c| c.legacy.is_empty()));
    assert_eq!(catalogs::encode(&list).unwrap(), V1);
}

#[test]
fn a_list_with_no_file_starts_with_gutenberg() {
    let mem = Mem::default();
    let list = catalogs::load(&mem, 100).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Project Gutenberg");
    assert_eq!(list[0].url, "https://www.gutenberg.org/ebooks.opds/");
    // Nothing was written by reading.
    assert!(!mem.has("catalogs.txt"));
}

#[test]
fn a_catalog_is_kept_without_its_password_and_with_clean_fields() {
    let mut catalog = Catalog::new("https://www.example.org/opds").unwrap();
    assert_eq!(catalog.name, "example.org");
    assert!(!catalog.webdav);
    catalog.user = "me".into();
    catalog.folder = "Books/Example".into();
    catalog.sync = true;
    catalog.name = "Tab\there\nand more".into();
    let mem = Mem::default();
    catalogs::save(&mem, &[catalog]).unwrap();
    let text = mem.text("catalogs.txt").unwrap();
    assert_eq!(text.lines().count(), 1, "{text:?}");
    assert_eq!(text.matches('\t').count(), 6);
    let back = catalogs::load(&mem, 100).unwrap();
    assert_eq!(back[0].name, "Tab here and more");
    assert_eq!(back[0].user, "me");
    assert!(back[0].sync);
    assert!(
        Catalog::new("https://cloud.example/remote.php/dav/files/me")
            .unwrap()
            .webdav
    );
}

#[test]
fn odd_lines_are_passed_over() {
    let text = "\n\tno name\nName only\nNo address\t\nGood\thttps://a.example\n\t\t\t\n";
    let list = catalogs::parse(text, 100).unwrap();
    assert_eq!(list.len(), 2, "{list:?}");
    assert_eq!(list[0].url, "no name");
    assert_eq!(list[1].name, "Good");
}

#[test]
fn a_list_that_cannot_be_read_is_an_error_and_never_an_empty_list() {
    let mem = Mem::default();
    // Saved on a computer in another encoding.
    mem.put("catalogs.txt", b"B\xfccher\thttps://a.example\n");
    assert_eq!(catalogs::load(&mem, 100), Err(Error::Utf8));
    let long = "Mine\thttps://a.example\n".repeat(MAX_FILE / 10);
    mem.put("catalogs.txt", long.as_bytes());
    assert_eq!(catalogs::load(&mem, 100), Err(Error::NoSpace));
    // More catalogs than the class holds: an error, not a cut list.
    let many = "Mine\thttps://a.example\n".repeat(max_catalogs(true) + 1);
    mem.put("catalogs.txt", many.as_bytes());
    assert_eq!(
        catalogs::load(&mem, max_catalogs(true)),
        Err(Error::NoSpace)
    );
    assert_eq!(
        catalogs::load(&mem, max_catalogs(false)).unwrap().len(),
        max_catalogs(true) + 1
    );
}

#[test]
fn a_list_that_would_pass_the_cap_is_not_written() {
    let mut one = Catalog::new("https://a.example").unwrap();
    one.name = "x".repeat(1000);
    let list = vec![one; 100];
    let mem = Mem::default();
    mem.put("catalogs.txt", b"kept\thttps://k.example\n");
    assert_eq!(catalogs::save(&mem, &list), Err(Error::NoSpace));
    assert_eq!(
        mem.text("catalogs.txt").unwrap(),
        "kept\thttps://k.example\n"
    );
}

#[test]
fn a_card_that_is_full_leaves_the_list_as_it_was() {
    let mem = Mem::default();
    mem.put("catalogs.txt", V1.as_bytes());
    mem.full_after.set(Some(0));
    let list = catalogs::parse(V1, 100).unwrap();
    assert_eq!(catalogs::save(&mem, &list), Err(Error::Full));
    assert_eq!(mem.text("catalogs.txt").unwrap(), V1);
}

#[test]
fn folders_are_below_the_library_folder() {
    let mut c = Catalog::new("https://www.example.org/opds").unwrap();
    assert_eq!(c.sync_folder("Books").unwrap(), "example.org");
    assert_eq!(c.shown_folder("Books").unwrap(), "Books/example.org");
    assert_eq!(c.shown_folder("").unwrap(), "example.org");
    c.name = "A/B: C?".into();
    assert_eq!(c.sync_folder("Books").unwrap(), "A B C");
    c.name = "///".into();
    assert_eq!(c.sync_folder("Books").unwrap(), "OPDS");
    // As the card's own path, as interface 6 had it, or from the library.
    c.folder = " /Books/Calibre/ ".into();
    assert_eq!(c.sync_folder("Books").unwrap(), "Calibre");
    c.folder = "Calibre".into();
    assert_eq!(c.sync_folder("Books").unwrap(), "Calibre");
    c.folder = "Books".into();
    assert_eq!(c.sync_folder("Books").unwrap(), "");
    c.folder = "Booksellers/X".into();
    assert_eq!(c.sync_folder("Books").unwrap(), "Booksellers/X");
}

fn with_old_password(url: &str, password: &str) -> Catalog {
    let mut catalog = Catalog::new(url).unwrap();
    catalog.legacy = Secret::copy(password).unwrap();
    catalog
}

#[test]
fn old_passwords_move_to_the_secrets_and_leave_the_list() {
    let mut list = vec![
        with_old_password("https://a.example", "pw-a"),
        Catalog::new("https://b.example").unwrap(),
        with_old_password("https://c.example", "pw-c"),
    ];
    let vault = MemVault::with(&name_of("https://c.example").unwrap(), "later choice");
    assert!(move_passwords(&vault, &mut list, &mut label).unwrap());
    assert!(list.iter().all(|c| c.legacy.is_empty()));
    assert_eq!(
        vault.value(&name_of("https://a.example").unwrap()).unwrap(),
        "pw-a"
    );
    // The secret the user chose later stays.
    assert_eq!(
        vault.value(&name_of("https://c.example").unwrap()).unwrap(),
        "later choice"
    );
    let mem = Mem::default();
    catalogs::save(&mem, &list).unwrap();
    assert!(!mem.text("catalogs.txt").unwrap().contains("pw-"));
    assert!(!move_passwords(&vault, &mut list, &mut label).unwrap());
}

#[test]
fn where_no_secret_can_be_kept_the_old_password_stays_in_the_list_and_works() {
    let mut list = vec![with_old_password("https://a.example", "pw-a")];
    let vault = MemVault {
        unsupported: true,
        ..MemVault::default()
    };
    assert!(!move_passwords(&vault, &mut list, &mut label).unwrap());
    assert_eq!(list[0].legacy.as_str(), "pw-a");
    assert!(catalogs::encode(&list).unwrap().contains("pw-a"));
    assert_eq!(
        password(&vault, &list[0]).unwrap().unwrap().as_str(),
        "pw-a"
    );
    // The same where the store is full.
    let vault = MemVault {
        room: std::cell::Cell::new(Some(0)),
        ..MemVault::default()
    };
    assert!(!move_passwords(&vault, &mut list, &mut label).unwrap());
    assert_eq!(list[0].legacy.as_str(), "pw-a");
}

#[test]
fn a_password_is_the_secret_else_the_old_one_else_none() {
    let c = Catalog::new("https://a.example").unwrap();
    let vault = MemVault::default();
    assert!(password(&vault, &c).unwrap().is_none());
    vault
        .keep(&name_of("https://a.example").unwrap(), "l", "s3cret")
        .unwrap();
    assert_eq!(password(&vault, &c).unwrap().unwrap().as_str(), "s3cret");
    let old = with_old_password("https://b.example", "old");
    assert_eq!(password(&vault, &old).unwrap().unwrap().as_str(), "old");
}

#[test]
fn secrets_of_catalogs_that_are_gone_go_with_them() {
    let list = vec![Catalog::new("https://a.example").unwrap()];
    let vault = MemVault::default();
    for url in ["https://a.example", "https://gone.example"] {
        vault.keep(&name_of(url).unwrap(), "l", "x").unwrap();
    }
    prune(&vault, &list).unwrap();
    assert_eq!(vault.names(), [name_of("https://a.example").unwrap()]);
}

#[test]
fn a_new_address_takes_the_password_along() {
    let vault = MemVault::default();
    vault
        .keep(&name_of("https://a.example").unwrap(), "l", "x")
        .unwrap();
    assert!(move_secret(&vault, "https://a.example", "https://b.example", "l").unwrap());
    assert_eq!(
        vault.value(&name_of("https://b.example").unwrap()).unwrap(),
        "x"
    );
    // Pruned with the old address gone from the list.
    prune(&vault, &[Catalog::new("https://b.example").unwrap()]).unwrap();
    assert_eq!(vault.names().len(), 1);
    // Nothing to move, or no room: said.
    assert!(move_secret(&vault, "https://none.example", "https://c.example", "l").unwrap());
    let full = MemVault::with(&name_of("https://a.example").unwrap(), "x");
    full.room.set(Some(1));
    assert!(!move_secret(&full, "https://a.example", "https://b.example", "l").unwrap());
}
