//! Code plugins in the store with the real kernel on an X4: listed where a
//! build fits this PaperPaw, said once what they are before the first,
//! installed with their code checked whole and signed by their author
//! (remembered from then on, also after a removal), starting after a
//! restart; an update or a new install signed by another key is refused;
//! code that is damaged,
//! unsigned, made for another build or not by the author the library
//! names is not installed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Shared, Shopping};
use paperpaw_native::signature::{public_key, sign, sign_message};
use paperpaw_native::{Chip, Header, LINK_BASE};
use paperpaw_plugin_store::OPEN;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::native::keys::{
    Revocation, is_revoked, keep_revoked, known_recovery, revocation_message, revoked,
    rotation_message,
};
use paperpaw_sdk::native::{Fingerprint, Native, installed_code, known_author};
use paperpaw_sdk::{PluginKind, Storage, Version};

/// The addresses of a device that loads code plugins of [`BUILD`]: they
/// name it (`abi`), so that the library gives the builds for it (#148).
const PAGE: &str =
    "https://paperpaw.org/api/v1/plugins/clock.json?device=x4&lang=en&abi=0006456789abcdef";
const LIST: &str =
    "https://paperpaw.org/api/v1/plugins.json?device=x4&lang=en&abi=0006456789abcdef&order=fair";
const FILES: &str = "https://paperpaw.org/api/v1/plugins/clock";
/// The build of this PaperPaw.
const BUILD: Fingerprint = Fingerprint::of(paperpaw_sdk::native::INTERFACE, 0x4567_89ab_cdef);
/// The seeds of the keys of the plugin's author and of someone else.
const AUTHOR: [u8; 32] = [7; 32];
const OTHER: [u8; 32] = [9; 32];
/// The seed of the recovery key the author named.
const RECOVERY: [u8; 32] = [5; 32];

fn native() -> Native {
    let mut native = Native::new("code.ppn");
    native.fingerprint = BUILD;
    native
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A plugin file built for `build`, signed with the key of `seed` (or not
/// at all).
fn ppn(build: Fingerprint, seed: Option<&[u8; 32]>) -> Vec<u8> {
    let image = [0u8; 128];
    let mut header = Header {
        chip: Chip::Esp32C3,
        fingerprint: build.0,
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
    if let Some(seed) = seed {
        let signature = sign(&file, seed);
        file.extend_from_slice(&signature.to_bytes());
    }
    file
}

fn manifest(version: &str) -> String {
    format!("[plugin]\nid = clock\nname = Clock\nversion = {version}\n")
}

/// The list: the clock in `version`, by the author.
fn list(version: &str) -> String {
    format!(
        r#"{{"format": 2, "plugins": [
  {{"id": "clock", "name": "Clock", "summary": "A clock on the home screen.",
    "group": "reading", "author": "Someone", "licence": "MIT", "kind": "code",
    "author_key": "{}",
    "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": 40,
      "builds": [{{"device": "x4", "abi": "{BUILD}", "url": "clock/{version}/x4.ppn",
                   "size": 1, "sha256": "{}"}}]}}}}]}}"#,
        hex(&public_key(&AUTHOR)),
        "0".repeat(64),
    )
}

/// The clock's page at the library: its plugin.ini and `code`.
fn page(version: &str, code: &[u8]) -> String {
    let ini = manifest(version);
    format!(
        r#"{{"format": 2, "plugin": {{"id": "clock", "name": "Clock",
  "summary": "A clock on the home screen.", "group": "reading",
  "author": "Someone", "licence": "MIT", "kind": "code", "author_key": "{}",
  "latest": {{"version": "{version}", "sdk": "0.4", "formats": 1, "size": {},
    "files": [{{"path": "plugin.ini", "url": "clock/{version}/plugin.ini",
                "size": {}, "sha1": "{}"}}],
    "builds": [{{"device": "x4-pro", "abi": "{BUILD}", "url": "clock/{version}/x4-pro.ppn",
                 "size": 1, "sha256": "{}"}},
               {{"device": "x4", "abi": "{BUILD}", "url": "clock/{version}/x4.ppn",
                 "size": {}, "sha256": "{}"}}]}}}}}}"#,
        hex(&public_key(&AUTHOR)),
        ini.len(),
        ini.len(),
        paperpaw_digest::sha1::hex(ini.as_bytes()),
        "0".repeat(64),
        code.len(),
        paperpaw_digest::sha256::hex(code),
    )
}

/// The store on an X4 that loads code plugins of [`BUILD`].
fn store(version: &str) -> Shopping {
    Shopping::on(Some("x4"), Some(native()), &list(version))
}

/// The clock's page opened from its group's list.
fn open_clock(shopping: &mut Shopping, version: &str, code: &[u8]) {
    shopping.choose("Reading");
    shopping.choose("Clock");
    shopping.serve(PAGE, page(version, code).as_bytes());
}

/// The clock's files answered: its plugin.ini, then `code`.
fn download(shopping: &mut Shopping, version: &str, code: &[u8]) {
    let ini = manifest(version);
    shopping.serve(&format!("{FILES}/{version}/plugin.ini"), ini.as_bytes());
    shopping.serve(&format!("{FILES}/{version}/x4.ppn"), code);
}

/// The clock installed the first time: its page, the notice, its files.
fn first_install(shopping: &mut Shopping, version: &str, code: &[u8]) {
    open_clock(shopping, version, code);
    let shown = shopping.shown();
    assert!(
        shown.contains("Code plugin for PaperPaw 0.4: runs with full access to this device."),
        "{shown}"
    );
    // *Install*: first what code plugins are.
    shopping.run(actions::CONFIRM);
    let shown = shopping.shown();
    assert!(shown.contains("Code plugins are programs"), "{shown}");
    assert!(shown.contains("I understand, install | Not now"), "{shown}");
    shopping.run(actions::CONFIRM);
    download(shopping, version, code);
}

fn installed(shopping: &Shopping) -> Option<Version> {
    shopping
        .kernel
        .plugins()
        .get("clock")
        .map(|p| p.update.unwrap_or(p.manifest.version))
}

#[test]
fn a_code_plugin_is_installed_and_starts_after_a_restart() {
    let mut shopping = store("1.0.0");
    let code = ppn(BUILD, Some(&AUTHOR));
    // Its page, and what code plugins are, said before the first.
    open_clock(&mut shopping, "1.0.0", &code);
    shopping.check("code-page");
    shopping.run(actions::CONFIRM);
    shopping.check("code-notice");
    // *Back*: the plugin again, nothing installed.
    shopping.run(actions::BACK);
    let shown = shopping.shown();
    assert!(shown.contains("Install, 405 B"), "{shown}");
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, list("1.0.0").as_bytes());
    first_install(&mut shopping, "1.0.0", &code);
    let clock = shopping.kernel.plugins().get("clock").unwrap();
    assert_eq!(clock.kind, PluginKind::Code);
    assert!(clock.restart_needed());
    assert!(shopping.storage.exists("plugins/clock/code.ppn"));
    // Its author remembered for its updates.
    let storage = &shopping.storage;
    assert_eq!(known_author(storage, "clock"), Some(public_key(&AUTHOR)));
    // Its code named as installed: only that file runs.
    assert_eq!(installed_code(storage, "clock"), Some(digest(&code)));
    let shown = shopping.shown();
    assert!(shown.contains("Restart now"), "{shown}");
    assert!(
        shown.contains("The change applies once PaperPaw starts again."),
        "{shown}"
    );
}

/// `page` with the author's recovery key named, as a library does.
fn with_recovery(page: &str, seed: &[u8; 32]) -> String {
    let author = format!("\"author_key\": \"{}\"", hex(&public_key(&AUTHOR)));
    let named = format!("{author}, \"recovery_key\": \"{}\"", hex(&public_key(seed)));
    assert!(page.contains(&author));
    page.replace(&author, &named)
}

/// The clock's page, with `recovery` named, opened and installed.
fn install_naming(shopping: &mut Shopping, version: &str, code: &[u8], recovery: &[u8; 32]) {
    shopping.choose("Reading");
    shopping.choose("Clock");
    let text = with_recovery(&page(version, code), recovery);
    shopping.serve(PAGE, text.as_bytes());
    // *Install*; what code plugins are, said once, then taken.
    shopping.run(actions::CONFIRM);
    shopping.run(actions::CONFIRM);
    download(shopping, version, code);
}

#[test]
fn the_recovery_key_is_pinned_at_the_first_install() {
    let mut shopping = store("1.0.0");
    let code = ppn(BUILD, Some(&AUTHOR));
    install_naming(&mut shopping, "1.0.0", &code, &RECOVERY);
    let storage = &shopping.storage;
    assert_eq!(known_author(storage, "clock"), Some(public_key(&AUTHOR)));
    assert_eq!(
        known_recovery(storage, "clock"),
        Some(public_key(&RECOVERY))
    );
}

#[test]
fn a_recovery_key_named_by_an_update_is_not_pinned() {
    let mut shopping = store("1.0.0");
    first_install(&mut shopping, "1.0.0", &ppn(BUILD, Some(&AUTHOR)));
    assert_eq!(known_recovery(&shopping.storage, "clock"), None);
    // A newer version by the same author, the library now naming a
    // recovery key (a taken-over library could).
    let newer = ppn(BUILD, Some(&AUTHOR));
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, list("1.1.0").as_bytes());
    shopping.choose("Reading");
    shopping.choose("Clock");
    let text = with_recovery(&page("1.1.0", &newer), &RECOVERY);
    shopping.serve(PAGE, text.as_bytes());
    shopping.run(actions::DOWN);
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.1.0", &newer);
    assert_eq!(installed(&shopping), Some(Version::new(1, 1, 0)));
    assert_eq!(known_recovery(&shopping.storage, "clock"), None);
}

/// The clock's page for an update, the library naming `signer` as its
/// author's key and carrying these rotation steps (`to`, signature).
fn rotated_page(
    version: &str,
    code: &[u8],
    signer: &[u8; 32],
    steps: &[([u8; 32], [u8; 64])],
) -> String {
    let author = format!("\"author_key\": \"{}\"", hex(&public_key(&AUTHOR)));
    let steps: Vec<String> = steps
        .iter()
        .map(|(to, signature)| {
            format!(
                "{{\"to\": \"{}\", \"signature\": \"{}\"}}",
                hex(to),
                hex(signature)
            )
        })
        .collect();
    let named = format!(
        "\"author_key\": \"{}\", \"rotations\": [{}]",
        hex(&public_key(signer)),
        steps.join(", ")
    );
    page(version, code).replace(&author, &named)
}

/// What the recovery key signs to move the clock from the key of seed
/// `from` to `new`.
fn moved_from(from: &[u8; 32], new: &[u8; 32]) -> [u8; 64] {
    let message = rotation_message("clock", &public_key(from), new);
    sign_message(message.as_bytes(), &RECOVERY)
}

/// What the recovery key signs to move the clock from [`AUTHOR`] to `new`.
fn moved(new: &[u8; 32]) -> [u8; 64] {
    moved_from(&AUTHOR, new)
}

/// The clock installed by [`AUTHOR`], naming [`RECOVERY`] if `pinned`,
/// then its update 1.1.0, signed by the key of seed `signer`, offered with
/// these rotation steps; whether it was installed.
fn update_signed_by(
    pinned: bool,
    signer: &[u8; 32],
    steps: &[([u8; 32], [u8; 64])],
) -> (Shopping, bool) {
    update_signed_by_on(pinned, signer, steps, false)
}

/// As [`update_signed_by`], with an update of PaperPaw on trial (the kernel's
/// mark) when the update of the clock is fetched, if `on_trial`.
fn update_signed_by_on(
    pinned: bool,
    signer: &[u8; 32],
    steps: &[([u8; 32], [u8; 64])],
    on_trial: bool,
) -> (Shopping, bool) {
    let mut shopping = store("1.0.0");
    let first = ppn(BUILD, Some(&AUTHOR));
    if pinned {
        install_naming(&mut shopping, "1.0.0", &first, &RECOVERY);
    } else {
        first_install(&mut shopping, "1.0.0", &first);
    }
    if on_trial {
        shopping
            .storage
            .write(".paperpaw/update-on-trial.txt", b"")
            .unwrap();
    }
    let signed = ppn(BUILD, Some(signer));
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, list("1.1.0").as_bytes());
    shopping.choose("Reading");
    shopping.choose("Clock");
    let text = rotated_page("1.1.0", &signed, signer, steps);
    shopping.serve(PAGE, text.as_bytes());
    shopping.run(actions::DOWN);
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.1.0", &signed);
    let done = installed(&shopping) == Some(Version::new(1, 1, 0));
    (shopping, done)
}

#[test]
fn a_rotation_by_the_recovery_key_moves_the_plugin_to_the_new_key() {
    let to = public_key(&OTHER);
    let (shopping, done) = update_signed_by(true, &OTHER, &[(to, moved(&to))]);
    assert!(done);
    let storage = &shopping.storage;
    assert_eq!(known_author(storage, "clock"), Some(to));
    assert_eq!(
        known_recovery(storage, "clock"),
        Some(public_key(&RECOVERY))
    );
    let code = ppn(BUILD, Some(&OTHER));
    assert_eq!(installed_code(storage, "clock"), Some(digest(&code)));
}

#[test]
fn nothing_of_a_code_plugin_changes_while_an_update_is_on_trial() {
    // The update of PaperPaw may go back: what the firmware before finds,
    // the plugin's code, its installed digest and its author, stays as it
    // was, also where the library carries a valid rotation of the author's
    // key, which taking would change them (#164).
    let to = public_key(&OTHER);
    let (mut shopping, done) = update_signed_by_on(true, &OTHER, &[(to, moved(&to))], true);
    assert!(!done);
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
    let shown = shopping.shown().replace(" | ", " ");
    assert!(shown.contains("update is not confirmed"), "{shown}");
    let storage = &shopping.storage;
    assert_eq!(known_author(storage, "clock"), Some(public_key(&AUTHOR)));
    let first = ppn(BUILD, Some(&AUTHOR));
    assert_eq!(installed_code(storage, "clock"), Some(digest(&first)));
    assert_eq!(
        known_recovery(storage, "clock"),
        Some(public_key(&RECOVERY)),
        "the pinned recovery key too"
    );
    // Not the plugin's folder, nor the code waiting beside it.
    assert!(!storage.exists("plugins/.clock.next/code.ppn"));
}

#[test]
fn a_device_behind_follows_the_steps_from_the_key_it_knows() {
    let third_seed = [3u8; 32];
    let (b, c) = (public_key(&OTHER), public_key(&third_seed));
    // The author moved A to B, then B to C; the device is still on A.
    let steps = [(b, moved(&b)), (c, moved_from(&OTHER, &c))];
    let (shopping, done) = update_signed_by(true, &third_seed, &steps);
    assert!(done);
    assert_eq!(known_author(&shopping.storage, "clock"), Some(c));
    // Code signed by B only goes to B, though the library names C too.
    let (shopping, done) = update_signed_by(true, &OTHER, &steps);
    assert!(done);
    assert_eq!(known_author(&shopping.storage, "clock"), Some(b));
}

#[test]
fn steps_that_do_not_reach_the_signer_move_nothing() {
    let third_seed = [3u8; 32];
    let (b, c) = (public_key(&OTHER), public_key(&third_seed));
    // The library names a step to B only; the code is signed by C.
    let (shopping, done) = update_signed_by(true, &third_seed, &[(b, moved(&b))]);
    assert!(!done);
    assert_eq!(
        known_author(&shopping.storage, "clock"),
        Some(public_key(&AUTHOR))
    );
    // A step to C that does not hold after the first one that does.
    let bad = (c, [0u8; 64]);
    let (shopping, done) = update_signed_by(true, &third_seed, &[(b, moved(&b)), bad]);
    assert!(!done);
    assert_eq!(known_author(&shopping.storage, "clock"), Some(b));
}

#[test]
fn a_rotation_that_does_not_hold_is_refused_as_before() {
    let to = public_key(&OTHER);
    let stranger = sign_message(
        rotation_message("clock", &public_key(&AUTHOR), &to).as_bytes(),
        &OTHER,
    );
    let third = public_key(&[3; 32]);
    let cases = [
        // Signed by a key that is not the recovery key.
        (true, to, stranger),
        // The statement is for another key than the code's.
        (true, third, moved(&third)),
        // No recovery key was pinned: the library's rotation is no use.
        (false, to, moved(&to)),
    ];
    for (pinned, to, signature) in cases {
        let (mut shopping, done) = update_signed_by(pinned, &OTHER, &[(to, signature)]);
        assert!(!done);
        assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
        let storage = &shopping.storage;
        assert_eq!(known_author(storage, "clock"), Some(public_key(&AUTHOR)));
        let first = ppn(BUILD, Some(&AUTHOR));
        assert_eq!(installed_code(storage, "clock"), Some(digest(&first)));
        let shown = shopping.shown().replace(" | ", " ");
        assert!(
            shown.contains("signed by another key than before"),
            "{shown}"
        );
    }
}

/// One entry of the library's `revoked_keys`: `plugin`'s `key`, with
/// `signature` as given (hex).
fn revocation(plugin: &str, key: &[u8; 32], signature: &[u8; 64]) -> String {
    format!(
        r#"{{"plugin": "{plugin}", "key": "{}", "signature": "{}"}}"#,
        hex(key),
        hex(signature)
    )
}

/// What `seed` signs to revoke `key` of `plugin`.
fn revoking(seed: &[u8; 32], plugin: &str, key: &[u8; 32]) -> [u8; 64] {
    sign_message(revocation_message(plugin, key).as_bytes(), seed)
}

/// The clock installed (naming [`RECOVERY`]), then the library's list
/// read with these `revoked_keys` entries.
fn list_revoking(entries: &[String]) -> Shopping {
    let mut shopping = store("1.0.0");
    install_naming(
        &mut shopping,
        "1.0.0",
        &ppn(BUILD, Some(&AUTHOR)),
        &RECOVERY,
    );
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    let text = list("1.0.0");
    let text = text.strip_suffix('}').unwrap();
    let text = format!("{text}, \"revoked_keys\": [{}]}}", entries.join(", "));
    shopping.serve(LIST, text.as_bytes());
    shopping
}

#[test]
fn revocations_signed_by_the_key_or_the_recovery_key_are_handed_over() {
    let author = public_key(&AUTHOR);
    for seed in [AUTHOR, RECOVERY] {
        let entry = revocation("clock", &author, &revoking(&seed, "clock", &author));
        let shopping = list_revoking(&[entry]);
        let storage = &shopping.storage;
        assert!(is_revoked(storage, "clock"));
        assert_eq!(revoked(storage).len(), 1);
    }
}

#[test]
fn revocations_that_do_not_hold_stop_nothing() {
    let author = public_key(&AUTHOR);
    let other = public_key(&OTHER);
    let entries = [
        // Signed by neither the key nor the recovery key.
        revocation("clock", &author, &revoking(&OTHER, "clock", &author)),
        // A key that is not the pinned author's, though signed by itself.
        revocation("clock", &other, &revoking(&OTHER, "clock", &other)),
        // A plugin nothing is known of.
        revocation("weather", &other, &revoking(&OTHER, "weather", &other)),
        // Damaged: a key too short, no signature, no plugin.
        r#"{"plugin": "clock", "key": "abcd", "signature": "00"}"#.to_string(),
        format!(r#"{{"plugin": "clock", "key": "{}"}}"#, hex(&author)),
        revocation("", &author, &revoking(&AUTHOR, "", &author)),
    ];
    let shopping = list_revoking(&entries);
    let storage = &shopping.storage;
    assert!(!is_revoked(storage, "clock"));
    assert!(revoked(storage).is_empty());
    // The plugin goes on as installed.
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
}

#[test]
fn a_bad_entry_does_not_cost_the_good_ones_beside_it() {
    let author = public_key(&AUTHOR);
    let entries = [
        r#"{"plugin": "clock", "key": "zz", "signature": "zz"}"#.to_string(),
        revocation("clock", &author, &revoking(&AUTHOR, "clock", &author)),
    ];
    let shopping = list_revoking(&entries);
    assert!(is_revoked(&shopping.storage, "clock"));
}

/// `count` well formed entries, each of a plugin that is not here.
fn junk(count: usize) -> Vec<String> {
    (0..count)
        .map(|n| {
            let seed = [(n % 250 + 1) as u8; 32];
            let key = public_key(&seed);
            let plugin = format!("p{n}");
            revocation(&plugin, &key, &revoking(&seed, &plugin, &key))
        })
        .collect()
}

#[test]
fn entries_for_plugins_not_here_take_no_place_from_those_that_are() {
    let author = public_key(&AUTHOR);
    let mut entries = junk(300);
    entries.push(revocation(
        "clock",
        &author,
        &revoking(&AUTHOR, "clock", &author),
    ));
    let shopping = list_revoking(&entries);
    assert!(is_revoked(&shopping.storage, "clock"));
    assert_eq!(revoked(&shopping.storage).len(), 1);
}

#[test]
fn a_list_is_read_only_so_far() {
    let author = public_key(&AUTHOR);
    let mut entries = junk(4096);
    entries.push(revocation(
        "clock",
        &author,
        &revoking(&AUTHOR, "clock", &author),
    ));
    let shopping = list_revoking(&entries);
    assert!(!is_revoked(&shopping.storage, "clock"));
}

#[test]
fn a_revocation_listed_twice_is_one() {
    let author = public_key(&AUTHOR);
    let entry = revocation("clock", &author, &revoking(&AUTHOR, "clock", &author));
    let shopping = list_revoking(&[entry.clone(), entry]);
    assert_eq!(revoked(&shopping.storage).len(), 1);
}

#[test]
fn a_stopped_plugin_says_so_on_its_page_and_offers_removal() {
    let mut first = store("1.0.0");
    let code = ppn(BUILD, Some(&AUTHOR));
    first_install(&mut first, "1.0.0", &code);
    let storage = first.storage.clone();
    // Its author revokes the key; the kernel, started again, does not run it.
    let author = public_key(&AUTHOR);
    let revocation = Revocation::new("clock", author, revoking(&AUTHOR, "clock", &author));
    assert!(keep_revoked(&storage, &[revocation]).is_ok());
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &list("1.0.0"), storage);
    shopping.choose("Reading");
    shopping.choose("Clock");
    shopping.serve(PAGE, page("1.0.0", &code).as_bytes());
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains("Stopped: the key its author signed it with"),
        "{shown}"
    );
    assert!(shown.contains("Remove"), "{shown}");
}

#[test]
fn an_update_by_another_key_is_refused() {
    let mut shopping = store("1.0.0");
    first_install(&mut shopping, "1.0.0", &ppn(BUILD, Some(&AUTHOR)));
    // A newer version, its code signed by someone else.
    let other = ppn(BUILD, Some(&OTHER));
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, list("1.1.0").as_bytes());
    open_clock(&mut shopping, "1.1.0", &other);
    let shown = shopping.shown();
    assert!(
        shown.contains("Restart now | Update, 1.1.0 | Remove"),
        "{shown}"
    );
    shopping.run(actions::DOWN);
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.1.0", &other);
    // Refused at once, with a short warning: no way to install it anyway.
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains("signed by another key than before"),
        "{shown}"
    );
    assert!(!shown.contains("anyway"), "{shown}");
    assert!(!shopping.storage.exists("plugins/.clock.part/code.ppn"));
    let storage = &shopping.storage;
    assert_eq!(known_author(storage, "clock"), Some(public_key(&AUTHOR)));
}

#[test]
fn a_new_install_after_a_removal_keeps_the_first_key() {
    // A device that had the clock, signed by its author, and removed it:
    // the key stays known.
    let storage = Shared::default();
    assert!(paperpaw_sdk::native::remember_author(
        &storage,
        "clock",
        &public_key(&AUTHOR)
    ));
    // The library now names someone else's key for it.
    let theirs = list("1.0.0").replace(&hex(&public_key(&AUTHOR)), &hex(&public_key(&OTHER)));
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &theirs, storage);
    let code = ppn(BUILD, Some(&OTHER));
    shopping.choose("Reading");
    shopping.choose("Clock");
    let page = page("1.0.0", &code).replace(&hex(&public_key(&AUTHOR)), &hex(&public_key(&OTHER)));
    shopping.serve(PAGE, page.as_bytes());
    shopping.run(actions::CONFIRM);
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.0.0", &code);
    assert_eq!(installed(&shopping), None);
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains("signed by another key than before"),
        "{shown}"
    );
    assert_eq!(
        known_author(&shopping.storage, "clock"),
        Some(public_key(&AUTHOR))
    );
}

/// The SHA-256 a plugin file's header carries.
fn digest(file: &[u8]) -> [u8; 32] {
    Header::read(file).unwrap().sha256
}

/// The clock's first install with `code`: not installed, the reader told
/// `why`.
fn refused(code: &[u8], why: &str) {
    let mut shopping = store("1.0.0");
    first_install(&mut shopping, "1.0.0", code);
    assert_eq!(installed(&shopping), None, "{why}");
    assert!(!shopping.storage.exists("plugins/clock/code.ppn"), "{why}");
    assert!(
        !shopping.storage.exists("plugins/.clock.part/code.ppn"),
        "{why}"
    );
    assert_eq!(known_author(&shopping.storage, "clock"), None, "{why}");
    assert_eq!(installed_code(&shopping.storage, "clock"), None, "{why}");
    // Said in the notice over the page, its lines joined.
    let shown = shopping.shown().replace(" | ", " ");
    assert!(shown.contains(why), "{why}: {shown}");
}

#[test]
fn code_that_cannot_be_trusted_is_not_installed() {
    refused(
        &ppn(BUILD, Some(&OTHER)),
        "not signed by the author the library names",
    );
    refused(&ppn(BUILD, None), "not signed by its author");
    refused(
        &ppn(Fingerprint(1), Some(&AUTHOR)),
        "built for another plugin interface or compiler",
    );
    let mut damaged = ppn(BUILD, Some(&AUTHOR));
    damaged[100] ^= 0xFF;
    refused(&damaged, "its code is damaged");
}

#[test]
fn without_code_plugins_none_is_listed() {
    let mut shopping = Shopping::on(Some("x4"), None, &list("1.0.0"));
    let shown = shopping.shown();
    assert!(!shown.contains("Clock"), "{shown}");
}

/// A storage where code plugins have `total` bytes, in pages of 4 KiB,
/// and another code plugin takes one page.
fn with_room(total: u64) -> Shared {
    with_room_for(total, 83)
}

/// As [`with_room`], for `most` code plugins at most.
fn with_room_for(total: u64, most: u32) -> Shared {
    with_room_line(&format!("{total} 4096 4096 {most} 1 0\n"))
}

/// A storage whose shell kept the room as `room` says, another code
/// plugin taking one page.
fn with_room_line(room: &str) -> Shared {
    let storage = Shared::default();
    storage
        .write(".paperpaw/code-room.txt", room.as_bytes())
        .unwrap();
    let other = ppn(BUILD, Some(&OTHER));
    storage.write("plugins/other/code.ppn", &other).unwrap();
    storage
}

/// Where the shell says a full flash gives more room (a smaller layout
/// than PaperPaw's own), the web flasher is named; elsewhere it is not.
#[test]
fn the_web_flasher_is_named_only_where_the_shell_says_it_gives_more() {
    for (room, named) in [
        ("8192 4096 4096 83 1 0 2400000\n", true),
        ("8192 4096 4096 83 1 0 0\n", false),
        ("8192 4096 4096 83 1 0\n", false),
    ] {
        let storage = with_room_line(room);
        let mut shopping = Shopping::with(Some("x4"), Some(native()), &list("1.0.0"), storage);
        shopping.choose("Reading");
        shopping.choose("Clock");
        let code = ppn(BUILD, Some(&AUTHOR));
        let body = page("1.0.0", &code).replace(
            r#""device": "x4", "abi""#,
            r#""device": "x4", "image_len": 8192, "abi""#,
        );
        shopping.serve(PAGE, body.as_bytes());
        let shown = shopping.shown();
        assert!(shown.contains("Not enough room"), "{shown}");
        assert_eq!(shown.contains("web flasher"), named, "{room}: {shown}");
    }
}

#[test]
fn code_that_does_not_fit_is_not_offered() {
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &list("1.0.0"), with_room(8192));
    shopping.choose("Reading");
    shopping.choose("Clock");
    // The library says its code takes two pages; one is left.
    let code = ppn(BUILD, Some(&AUTHOR));
    let body = page("1.0.0", &code).replace(
        r#""device": "x4", "abi""#,
        r#""device": "x4", "image_len": 8192, "abi""#,
    );
    shopping.serve(PAGE, body.as_bytes());
    let shown = shopping.shown();
    assert!(
        shown.contains("Needs 8.0 KB for its code, only 4.0 KB left."),
        "{shown}"
    );
    assert!(shown.contains("Not enough room"), "{shown}");
    assert!(!shown.contains("Install"), "{shown}");
    // Where the library does not say, it is offered with what is left.
    shopping.run(actions::BACK);
    shopping.choose("Clock");
    shopping.serve(PAGE, page("1.0.0", &code).as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("4.0 KB left for code plugins"), "{shown}");
    assert!(shown.contains("Install"), "{shown}");
}

#[test]
fn code_that_does_not_fit_is_refused_once_here() {
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &list("1.0.0"), with_room(4096));
    first_install(&mut shopping, "1.0.0", &ppn(BUILD, Some(&AUTHOR)));
    assert_eq!(installed(&shopping), None);
    assert!(!shopping.storage.exists("plugins/clock/code.ppn"));
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains("not enough room left for code plugins"),
        "{shown}"
    );
}

/// The Installed list's line says, on the stock layout only, that a full
/// install via USB gives more, with the room it would give.
#[test]
fn the_installed_list_names_the_bigger_room_of_a_full_install_on_the_stock_layout() {
    for (room, said) in [
        ("1126400 4096 4096 83 1 0 3000000\n", true),
        ("1126400 4096 4096 83 1 0 0\n", false),
    ] {
        let storage = with_room_line(room);
        let mut shopping = Shopping::with(Some("x4"), Some(native()), &list("1.0.0"), storage);
        first_install(&mut shopping, "1.0.0", &ppn(BUILD, Some(&AUTHOR)));
        shopping.run(actions::BACK);
        shopping.run(actions::BACK);
        shopping.run(actions::BACK);
        shopping.run(OPEN);
        shopping.serve(LIST, list("1.0.0").as_bytes());
        shopping.choose("Installed");
        let shown = shopping.shown();
        assert!(shown.contains("code plugins:"), "{shown}");
        assert_eq!(
            shown.contains("paperpaw.org/flash"),
            said,
            "{room}: {shown}"
        );
        if said {
            assert!(shown.contains("about 2.9 MB"), "{shown}");
            assert!(shown.contains("used. A full install"), "{shown}");
        }
    }
}

#[test]
fn code_that_just_fits_is_installed() {
    // Room for exactly two pages and two plugins: the other one's and
    // the clock's. The clock's download, put together beside it, is not
    // counted as one more.
    let storage = with_room_for(8192, 2);
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &list("1.0.0"), storage);
    first_install(&mut shopping, "1.0.0", &ppn(BUILD, Some(&AUTHOR)));
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
    assert!(shopping.storage.exists("plugins/clock/code.ppn"));
    // Now the room is full: the Installed list says so.
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, list("1.0.0").as_bytes());
    shopping.choose("Installed");
    let shown = shopping.shown();
    assert!(
        shown.contains("code plugins: 8.0 KB of 8.0 KB used"),
        "{shown}"
    );
}

/// PaperPaw's own plugins say so on their row and their page; the list's
/// word alone, with another author's key, gives no such mark.
#[test]
fn paperpaws_own_plugins_say_so() {
    const OURS: &str = "148ccf3ebc30506b355c6b5cb2baf87d9265606cc6a81f085bb3b9740a95aebd";
    let author = hex(&public_key(&AUTHOR));
    let claimed = |text: String| {
        text.replace(
            r#""author": "Someone","#,
            r#""author": "Someone", "official": true,"#,
        )
    };
    let code = ppn(BUILD, Some(&AUTHOR));

    let mut shopping = Shopping::on(
        Some("x4"),
        Some(native()),
        &claimed(list("1.0.0")).replace(&author, OURS),
    );
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(
        shown.contains("By PaperPaw · A clock on the home screen."),
        "{shown}"
    );
    shopping.choose("Clock");
    let ours = claimed(page("1.0.0", &code)).replace(&author, OURS);
    shopping.serve(PAGE, ours.as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("By PaperPaw"), "{shown}");
    assert!(!shown.contains("By Someone"), "{shown}");

    // The same word with the author's own key: no mark.
    let mut shopping = Shopping::on(Some("x4"), Some(native()), &claimed(list("1.0.0")));
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(!shown.contains("By PaperPaw"), "{shown}");
    shopping.choose("Clock");
    shopping.serve(PAGE, claimed(page("1.0.0", &code)).as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("By Someone"), "{shown}");
    assert!(!shown.contains("By PaperPaw"), "{shown}");
}

/// `text` (the list or a page) as the library gives a community plugin,
/// which nobody reviewed.
fn unreviewed(text: &str) -> String {
    text.replace(
        r#""author": "Someone","#,
        r#""author": "Someone", "unreviewed": true,
           "repository": "https://github.com/someone/clock","#,
    )
}

#[test]
fn a_community_plugin_is_marked_and_its_key_may_not_change() {
    let mut shopping = Shopping::on(Some("x4"), Some(native()), &unreviewed(&list("1.0.0")));
    shopping.choose("Reading");
    let shown = shopping.shown();
    assert!(shown.contains("Unreviewed · A clock"), "{shown}");
    let code = ppn(BUILD, Some(&AUTHOR));
    shopping.choose("Clock");
    shopping.serve(PAGE, unreviewed(&page("1.0.0", &code)).as_bytes());
    let shown = shopping.shown();
    assert!(
        shown.contains("Unreviewed, from github.com/someone/clock"),
        "{shown}"
    );
    // Its key, which its updates must have too.
    let key = hex(&public_key(&AUTHOR));
    let short = format!(
        "Key {} {} {} {}",
        &key[..4],
        &key[4..8],
        &key[8..12],
        &key[12..16]
    );
    assert!(shown.contains(&short), "{shown}");
    shopping.run(actions::CONFIRM);
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.0.0", &code);
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));

    // An update signed by someone else: refused.
    let other = ppn(BUILD, Some(&OTHER));
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, unreviewed(&list("1.1.0")).as_bytes());
    shopping.choose("Reading");
    shopping.choose("Clock");
    shopping.serve(PAGE, unreviewed(&page("1.1.0", &other)).as_bytes());
    shopping.run(actions::DOWN);
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.1.0", &other);
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
    let shown = shopping.shown().replace(" | ", " ");
    assert!(
        shown.contains("signed by another key than before"),
        "{shown}"
    );
    assert!(!shown.contains("anyway"), "{shown}");
    let storage = &shopping.storage;
    assert_eq!(known_author(storage, "clock"), Some(public_key(&AUTHOR)));
    assert!(!storage.exists("plugins/.clock.part/code.ppn"));
}

/// This PaperPaw's firmware: families 3 and 2 of the shared functions, and
/// a table of 400 bytes.
fn native_with_families() -> Native {
    let mut native = native();
    native.toolchain = "rustc 1.99.0, riscv32imc-unknown-none-elf".into();
    native.api_size = 400;
    native.family_revs = [3, 2, 0, 0, 0, 0, 0, 0];
    native.family_min = [1, 1, 0, 0, 0, 0, 0, 0];
    native
}

/// A library text whose builds say they need `needs` of the firmware.
fn needing(text: &str, needs: &str) -> String {
    let abi = format!("\"abi\": \"{BUILD}\"");
    assert!(text.contains(&abi));
    text.replace(&abi, &format!("{abi}, {needs}"))
}

#[test]
fn a_plugin_made_for_a_newer_paperpaw_is_not_offered_to_install() {
    let needs = "\"interface\": 5, \"needs_api\": 400, \"families\": [4, 0, 0, 0, 0, 0, 0, 0]";
    let mut shopping = Shopping::on(
        Some("x4"),
        Some(native_with_families()),
        &needing(&list("1.0.0"), needs),
    );
    let code = ppn(BUILD, Some(&AUTHOR));
    shopping.choose("Reading");
    shopping.choose("Clock");
    shopping.serve(PAGE, needing(&page("1.0.0", &code), needs).as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("Needs a newer PaperPaw"), "{shown}");
    assert!(!shown.contains("Install"), "{shown}");
    // Chosen: said why, nothing downloaded.
    shopping.run(actions::DOWN);
    shopping.run(actions::CONFIRM);
    assert!(installed(&shopping).is_none());
    assert!(shopping.requests().is_empty());
}

#[test]
fn an_update_made_for_a_newer_paperpaw_is_not_offered_and_the_old_version_stays() {
    let mut shopping = Shopping::on(
        Some("x4"),
        Some(native_with_families()),
        &needing(&list("1.0.0"), "\"families\": [3, 2, 0, 0, 0, 0, 0, 0]"),
    );
    let first = ppn(BUILD, Some(&AUTHOR));
    first_install(&mut shopping, "1.0.0", &first);
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
    // The library has 1.1.0, rebuilt after the shared functions grew.
    let needs = "\"families\": [4, 0, 0, 0, 0, 0, 0, 0]";
    let newer = ppn(BUILD, Some(&AUTHOR));
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, needing(&list("1.1.0"), needs).as_bytes());
    shopping.choose("Reading");
    shopping.choose("Clock");
    shopping.serve(PAGE, needing(&page("1.1.0", &newer), needs).as_bytes());
    let shown = shopping.shown();
    assert!(shown.contains("Needs a newer PaperPaw"), "{shown}");
    assert!(!shown.contains("Update"), "{shown}");
    assert!(shown.contains("Remove"), "{shown}");
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
    // The same update for a firmware that has the family: offered.
    let mut roomy = native_with_families();
    roomy.family_revs[0] = 4;
    let mut shopping = Shopping::on(Some("x4"), Some(roomy), &needing(&list("1.0.0"), needs));
    first_install(&mut shopping, "1.0.0", &first);
    shopping.run(actions::BACK);
    shopping.run(actions::BACK);
    shopping.run(OPEN);
    shopping.serve(LIST, needing(&list("1.1.0"), needs).as_bytes());
    shopping.choose("Reading");
    shopping.choose("Clock");
    shopping.serve(PAGE, needing(&page("1.1.0", &newer), needs).as_bytes());
    let shown = shopping.shown();
    assert!(
        shown.contains("Update") && !shown.contains("Needs a newer"),
        "{shown}"
    );
}

#[test]
fn families_do_not_hold_back_a_build_on_a_chip_that_does_not_read_them() {
    // The X4 Pro's firmware reads no families (`Native::problem`): a build
    // that names one past its list is still for it.
    let mut pro = native_with_families();
    pro.toolchain = "esp 1.97.0.0, xtensa-esp32s3-none-elf".into();
    let needs = "\"families\": [4, 0, 0, 0, 0, 0, 0, 0]";
    let mut shopping = Shopping::on(Some("x4"), Some(pro), &needing(&list("1.0.0"), needs));
    let code = ppn(BUILD, Some(&AUTHOR));
    shopping.choose("Reading");
    shopping.choose("Clock");
    shopping.serve(PAGE, needing(&page("1.0.0", &code), needs).as_bytes());
    let shown = shopping.shown();
    assert!(!shown.contains("Needs a newer"), "{shown}");
    assert!(shown.contains("Install"), "{shown}");
}

/// A storage where code plugins have `total` bytes, in pages of 4 KiB,
/// and another code plugin, `other` (one page), is installed: switched
/// off where `off` says (the next start skips its code).
fn with_other(total: u64, off: bool) -> Shared {
    let storage = with_room(total);
    storage
        .write(
            "plugins/other/plugin.ini",
            b"[plugin]\nid = other\nname = Other\nversion = 1.0.0\n",
        )
        .unwrap();
    if off {
        storage
            .write("paperpaw.ini", b"[plugins]\nother = off\n")
            .unwrap();
    }
    storage
}

/// The clock's page on a device whose library says its code takes two
/// pages.
fn clock_page_needing_two_pages(storage: Shared) -> Shopping {
    let mut shopping = Shopping::with(Some("x4"), Some(native()), &list("1.0.0"), storage);
    shopping.choose("Reading");
    shopping.choose("Clock");
    let body = page("1.0.0", &ppn(BUILD, Some(&AUTHOR))).replace(
        r#""device": "x4", "abi""#,
        r#""device": "x4", "image_len": 8192, "abi""#,
    );
    shopping.serve(PAGE, body.as_bytes());
    shopping
}

/// Room enough: the other code plugin is on and takes a page, three
/// are left for the clock's two.
#[test]
fn room_enough_with_the_others_on_is_said_plainly() {
    let mut shopping = clock_page_needing_two_pages(with_other(16384, false));
    let shown = shopping.shown();
    assert!(
        shown.contains("Needs 8.0 KB for its code, 12 KB left for code plugins"),
        "{shown}"
    );
    assert!(!shown.contains("switched off"), "{shown}");
    assert!(shown.contains("Install"), "{shown}");
}

/// The next start skips code that is switched off and its room is free:
/// the clock fits, and the page says for how long.
#[test]
fn room_only_while_a_code_plugin_stays_off_is_offered_and_said() {
    let storage = with_other(8192, true);
    let mut shopping = clock_page_needing_two_pages(storage);
    let shown = shopping.shown();
    assert!(
        shown.contains(
            "Needs 8.0 KB for its code, 8.0 KB left while the code plugins you switched off stay off"
        ),
        "{shown}"
    );
    assert!(!shown.contains("Not enough room"), "{shown}");
    assert!(shown.contains("Install"), "{shown}");
    // It is installed, not refused for the room of the plugin that is off.
    shopping.run(actions::CONFIRM);
    let shown = shopping.shown();
    assert!(shown.contains("Code plugins are programs"), "{shown}");
    shopping.run(actions::CONFIRM);
    download(&mut shopping, "1.0.0", &ppn(BUILD, Some(&AUTHOR)));
    assert_eq!(installed(&shopping), Some(Version::new(1, 0, 0)));
}

/// The same room with the other code plugin on: not enough, as before.
#[test]
fn room_not_enough_with_the_other_on_is_refused() {
    let mut shopping = clock_page_needing_two_pages(with_other(8192, false));
    let shown = shopping.shown();
    assert!(
        shown.contains("Needs 8.0 KB for its code, only 4.0 KB left."),
        "{shown}"
    );
    assert!(shown.contains("Not enough room"), "{shown}");
    assert!(!shown.contains("Install"), "{shown}");
}

/// Not enough even with the code plugin that is off left out: it is not
/// what is missing.
#[test]
fn room_not_enough_even_without_the_one_that_is_off_is_refused() {
    let mut shopping = clock_page_needing_two_pages(with_other(4096, true));
    let shown = shopping.shown();
    assert!(
        shown.contains("Needs 8.0 KB for its code, only 4.0 KB left."),
        "{shown}"
    );
    assert!(shown.contains("Not enough room"), "{shown}");
    assert!(!shown.contains("Install"), "{shown}");
}

#[test]
fn the_library_is_asked_for_the_builds_of_the_paperpaw_that_runs() {
    assert!(PAGE.ends_with(&format!("abi={BUILD}")), "{PAGE}");
    assert!(LIST.contains(&format!("&abi={BUILD}")), "{LIST}");
    // The list was asked for with it (the helper that opens the store
    // answers that address only), and so is the plugin's page.
    let mut shopping = store("1.0.0");
    shopping.choose("Reading");
    shopping.choose("Clock");
    shopping.request(PAGE);
}
