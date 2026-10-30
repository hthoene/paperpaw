//! The first-run setup's offer with the real kernel on an X4 that loads
//! code plugins: the plugins chosen are fetched one after another from
//! the plugin library and installed, said once at the end; one that
//! cannot come is named, and the store has it later.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{LIST, Shopping};
use paperpaw_native::signature::{public_key, sign};
use paperpaw_native::{Chip, Header, LINK_BASE};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::native::{Fingerprint, Native, installed_code};
use paperpaw_sdk::{Id, Instant, Params, PluginKind, Storage};

/// The build of this PaperPaw.
const BUILD: Fingerprint = Fingerprint(0x0123_4567_89ab_cdef);
/// The seed of PaperPaw's key for its own plugins.
const PAPERPAW: [u8; 32] = [5; 32];

fn native() -> Native {
    let mut native = Native::new("code.ppn");
    native.fingerprint = BUILD;
    native
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A plugin file built for [`BUILD`], signed by PaperPaw.
fn ppn() -> Vec<u8> {
    let image = [0u8; 128];
    let mut header = Header {
        chip: Chip::Esp32C3,
        fingerprint: BUILD.0,
        base: LINK_BASE,
        image_len: image.len() as u32,
        code_at: 96,
        entry: 0,
        api: 32,
        relocs: 0,
        align: 16,
        compiler: 1,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        needs_api: 0,
        families: [0; 8],
        interface: paperpaw_sdk::native::INTERFACE,
        sha256: [0; 32],
    };
    let mut sha = header.digest();
    sha.update(&image);
    header.sha256 = sha.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&image);
    file.extend_from_slice(&sign(&file, &PAPERPAW).to_bytes());
    file
}

fn manifest(id: &str) -> String {
    format!("[plugin]\nid = {id}\nname = {id}\nversion = 0.4.0\n")
}

/// Plugin `id`'s page at the library.
fn page(id: &str, code: &[u8]) -> String {
    let ini = manifest(id);
    format!(
        r#"{{"format": 2, "plugin": {{"id": "{id}", "name": "{id}",
  "summary": "PaperPaw's own.", "group": "reading",
  "author": "PaperPaw", "licence": "Apache-2.0", "kind": "code", "author_key": "{}",
  "latest": {{"version": "0.4.0", "sdk": "0.4", "formats": 1, "size": {},
    "files": [{{"path": "plugin.ini", "url": "{id}/0.4.0/plugin.ini",
                "size": {}, "sha1": "{}"}}],
    "builds": [{{"device": "x4", "abi": "{BUILD}", "url": "{id}/0.4.0/x4.ppn",
                 "size": {}, "sha256": "{}"}}]}}}}}}"#,
        hex(&public_key(&PAPERPAW)),
        ini.len(),
        ini.len(),
        paperpaw_digest::sha1::hex(ini.as_bytes()),
        code.len(),
        paperpaw_digest::sha256::hex(code),
    )
}

fn page_url(id: &str) -> String {
    format!("https://paperpaw.org/api/v1/plugins/{id}.json?device=x4&lang=en")
}

/// The offer opened as the setup opens it, on an X4 loading code plugins.
fn offer() -> Shopping {
    let mut shopping = Shopping::on(
        Some("x4"),
        Some(native()),
        r#"{"format": 2, "plugins": []}"#,
    );
    shopping.now += 100;
    shopping.kernel.open_screen(
        Instant(shopping.now),
        Id::new_static("store.offer"),
        Params::new(),
    );
    shopping
}

/// Plugin `id`'s page and files answered.
fn serve(shopping: &mut Shopping, id: &str, code: &[u8]) {
    shopping.serve(&page_url(id), page(id, code).as_bytes());
    let files = "https://paperpaw.org/api/v1/plugins";
    shopping.serve(
        &format!("{files}/{id}/0.4.0/plugin.ini"),
        manifest(id).as_bytes(),
    );
    shopping.serve(&format!("{files}/{id}/0.4.0/x4.ppn"), code);
}

#[test]
fn the_plugins_chosen_are_installed_and_said_once() {
    let mut shopping = offer();
    let shown = shopping.shown();
    for name in [
        "Reading statistics",
        "Home widgets",
        "Calibre connection",
        "KOReader sync",
    ] {
        assert!(shown.contains(name), "{shown}");
    }
    // *Done* there with nothing chosen too.
    assert!(shown.contains("| Done"), "{shown}");
    shopping.choose("Reading statistics");
    shopping.choose("Calibre connection");
    // Past KOReader sync to *Done*.
    shopping.run(actions::DOWN);
    shopping.run(actions::DOWN);
    let shown = shopping.shown();
    assert!(shown.contains("Done"), "{shown}");
    shopping.run(actions::CONFIRM);
    let code = ppn();
    serve(&mut shopping, "stats", &code);
    // No word of each on its own: the next one comes.
    assert!(
        !shopping.shown().contains("Installed"),
        "{}",
        shopping.shown()
    );
    serve(&mut shopping, "calibre", &code);
    for id in ["stats", "calibre"] {
        let plugin = shopping.kernel.plugins().get(id).unwrap();
        assert_eq!(plugin.kind, PluginKind::Code);
        assert!(shopping.storage.exists(&format!("plugins/{id}/code.ppn")));
        assert!(installed_code(&shopping.storage, id).is_some());
    }
    // The notice, as its lines show.
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains("Added: stats, calibre. They start the next time PaperPaw starts."),
        "{shown}"
    );
    assert!(
        !shopping
            .storage
            .exists(".paperpaw/plugins/store/queue-quiet.txt")
    );
}

/// The requests asked for since, by address.
fn asked(shopping: &mut Shopping) -> Vec<String> {
    shopping
        .requests()
        .into_iter()
        .map(|call| call.url)
        .collect()
}

/// Leaving the page with plugins chosen adds them as *Done* does: the
/// switches are the choice. With none chosen, nothing.
#[test]
fn back_adds_those_chosen_and_nothing_else() {
    let mut shopping = offer();
    shopping.choose("Reading statistics");
    shopping.run(actions::BACK);
    assert_ne!(
        shopping.kernel.active_screen().map(Id::as_str),
        Some("store.offer")
    );
    assert_eq!(asked(&mut shopping), [page_url("stats")]);
    assert!(
        shopping
            .storage
            .exists(".paperpaw/plugins/store/offer-chosen.txt")
    );
    let shown = shopping.shown().replace(" | ", " ");
    assert!(shown.contains("added in the background"), "{shown}");
    // Nothing chosen: left as it was.
    let mut shopping = offer();
    shopping.run(actions::BACK);
    assert!(asked(&mut shopping).is_empty());
    assert!(
        !shopping
            .storage
            .exists(".paperpaw/plugins/store/offer-chosen.txt")
    );
    assert!(!shopping.storage.exists(".paperpaw/plugins/store/queue.txt"));
}

/// By touch: the header's arrow back is *Back*, a plugin chosen by a tap
/// added once (not again by the *Back* that *Done* sends).
#[test]
fn the_headers_arrow_adds_those_chosen_too() {
    let mut shopping = Shopping::touch_with(Some(native()), r#"{"format": 2, "plugins": []}"#);
    shopping.now += 100;
    shopping.kernel.open_screen(
        Instant(shopping.now),
        Id::new_static("store.offer"),
        Params::new(),
    );
    let _ = shopping.shown();
    let row = shopping
        .kernel
        .describe()
        .into_iter()
        .find(|p| p.label.starts_with("Calibre"))
        .unwrap()
        .area;
    shopping.touch_at(i32::from(row.x) + 40, i32::from(row.y) + 20, false);
    // The arrow at the header's start.
    shopping.touch_at(30, 30, false);
    assert_ne!(
        shopping.kernel.active_screen().map(Id::as_str),
        Some("store.offer")
    );
    let page = page_url("calibre").replace("device=x4&", "device=x4-pro&");
    assert_eq!(asked(&mut shopping), [page]);
    let queue = String::from_utf8(
        paperpaw_sdk::read_file(&shopping.storage, ".paperpaw/plugins/store/queue.txt", 4096)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(queue, "> ~calibre\n");
}

/// *Done* with nothing chosen goes back to the setup, adding nothing.
#[test]
fn done_with_nothing_chosen_adds_nothing() {
    let mut shopping = offer();
    for _ in 0..4 {
        shopping.run(actions::DOWN);
    }
    shopping.run(actions::CONFIRM);
    assert_ne!(
        shopping.kernel.active_screen().map(Id::as_str),
        Some("store.offer")
    );
    assert!(asked(&mut shopping).is_empty());
    assert!(
        !shopping
            .storage
            .exists(".paperpaw/plugins/store/offer-chosen.txt")
    );
}

#[test]
fn one_that_cannot_come_is_named() {
    let mut shopping = offer();
    shopping.choose("Home widgets");
    shopping.run(actions::DOWN);
    shopping.run(actions::DOWN);
    shopping.run(actions::DOWN);
    shopping.run(actions::DOWN);
    shopping.run(actions::CONFIRM);
    let call = shopping.request(&page_url("widgets"));
    shopping.answer(&call, 404, b"not found");
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains(
            "Could not be added: Home widgets. They are in the store, under More plugins."
        ),
        "{shown}"
    );
    assert!(shopping.kernel.plugins().get("widgets").is_none());
}

#[test]
fn the_list_and_the_store_are_left_as_they_were() {
    // The list asked when the store opened is not asked again.
    let mut shopping = offer();
    assert!(shopping.requests().iter().all(|c| c.url != LIST));
}

/// The statistics built in, as on Android (by its id).
struct BuiltInStats;

impl paperpaw_sdk::Plugin for BuiltInStats {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("stats"),
            "Reading statistics",
            paperpaw_sdk::Version::new(0, 4, 0),
        )
        .description("Reading statistics.")
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, _: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        Ok(())
    }
}

#[test]
fn built_in_ones_are_switched_on() {
    let mut kernel = paperpaw_core::Kernel::builder(common::DEVICE)
        .storage(paperpaw_sdk::MemoryStorage::new())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_menu::Menu)
        .plugin(paperpaw_plugin_store::Store)
        .plugin_off(BuiltInStats)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.open_screen(Instant(100), Id::new_static("store.offer"), Params::new());
    kernel.render(&mut |_, _| {});
    let shown: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
    // Only what can be added here: no code plugins without a network.
    assert!(
        shown.iter().any(|l| l.starts_with("Reading statistics")),
        "{shown:?}"
    );
    assert!(!shown.iter().any(|l| l.starts_with("Calibre")), "{shown:?}");
    // Chosen, then *Done*.
    kernel.run_action(Instant(200), actions::CONFIRM);
    kernel.run_action(Instant(300), actions::DOWN);
    kernel.run_action(Instant(400), actions::CONFIRM);
    let stats = kernel.plugins().get("stats").unwrap();
    assert!(stats.enabled, "{stats:?}");
    assert!(
        kernel
            .take_io()
            .iter()
            .all(|io| !matches!(io, paperpaw_core::Io::Http(_)))
    );
}

/// The kernel restarted on `storage`, as on an X4 loading code plugins:
/// the store not opened.
fn restarted(storage: common::Shared) -> paperpaw_core::Kernel {
    let mut kernel = paperpaw_core::Kernel::builder(common::DEVICE)
        .storage(storage)
        .network(true)
        .catalog_device("x4")
        .code_plugins(native(), Box::new(common::NoCode))
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_menu::Menu)
        .plugin(paperpaw_plugin_store::Store)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

#[test]
fn the_setups_plugins_go_on_after_a_restart_unasked_ones_do_not() {
    let mut shopping = offer();
    shopping.choose("Reading statistics");
    for _ in 0..4 {
        shopping.run(actions::DOWN);
    }
    shopping.run(actions::CONFIRM);
    // Its page asked for, then a restart before anything came.
    shopping.request(&page_url("stats"));
    let storage = shopping.storage.clone();
    let queue = String::from_utf8(
        paperpaw_sdk::read_file(&storage, ".paperpaw/plugins/store/queue.txt", 4096).unwrap(),
    )
    .unwrap();
    // Marked quiet one by one: an *Update all* put in later is not.
    assert_eq!(queue, "> ~stats\n");
    let mut kernel = restarted(storage.clone());
    let asked: Vec<String> = kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            paperpaw_core::Io::Http(call) => Some(call.url),
            _ => None,
        })
        .collect();
    assert_eq!(asked, [page_url("stats")], "the store not opened");
    // An update put in the queue the usual way waits for the store.
    storage
        .write(".paperpaw/plugins/store/queue.txt", b"> weather\n")
        .unwrap();
    let mut kernel = restarted(storage);
    assert!(
        kernel
            .take_io()
            .iter()
            .all(|io| !matches!(io, paperpaw_core::Io::Http(_)))
    );
}
