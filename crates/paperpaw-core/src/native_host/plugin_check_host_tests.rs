//! A library built for this computer (`code-host.so`) checked and installed
//! where the device runs such files (#278): signed over the library by the
//! trailer, digest of the whole file, `code.ppn` another kind of device's.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::HostCode;
use crate::native_host::plugin_check::tests::{
    AUTHOR_SEED, INI, Names, OTHER_SEED, author, good_record, host,
};
use crate::native_host::plugin_install::PluginInstall;
use crate::native_host::plugin_job::{Answer, DONE_AT_NEXT_START, Finished, PluginHost, PluginJob};
use crate::native_host::plugin_rules::Evidence;
use paperpaw_native::signature::sign;
use paperpaw_sdk::{MemoryStorage, Storage, native};
use std::{format, sync::OnceLock, vec::Vec};

const SOURCE: &str = "Download/dials";
const FILE: &str = "code-host.so";

/// What a library's record is (the tests' libraries are not ELF).
fn record(bytes: &[u8]) -> Option<&[u8]> {
    static RECORD: OnceLock<Vec<u8>> = OnceLock::new();
    bytes
        .starts_with(b"\x7fELF")
        .then(|| RECORD.get_or_init(good_record).as_slice())
}

fn host_code() -> PluginHost {
    PluginHost {
        loader: None,
        host_code: Some(HostCode { file: FILE, record }),
        ..host()
    }
}

/// A library of `len` bytes with its trailer, signed by `seed`.
fn library(seed: Option<&[u8; 32]>, len: usize) -> Vec<u8> {
    let mut file = b"\x7fELF".to_vec();
    file.extend((0..len).map(|i| (i % 251) as u8));
    if let Some(seed) = seed {
        let trailer = sign(&file, seed).to_bytes();
        file.extend(trailer);
    }
    file
}

fn folder(card: &MemoryStorage, file: &[u8]) {
    card.write(&format!("{SOURCE}/plugin.ini"), INI.as_bytes())
        .unwrap();
    card.write(&format!("{SOURCE}/{FILE}"), file).unwrap();
}

fn check(card: &MemoryStorage, host: &PluginHost) -> Finished {
    let mut job = PluginCheck::new(7, 1, SOURCE).unwrap();
    let env = JobEnv {
        storage: card,
        host,
        built_ins: &Names,
        granted: false,
    };
    for _ in 0..10_000 {
        if let JobStep::Done(done) = job.step(&env) {
            return done;
        }
    }
    panic!("a check that never ends");
}

#[test]
fn a_signed_library_fits_with_its_key_and_the_digest_of_the_whole_file() {
    let card = MemoryStorage::new();
    let file = library(Some(&AUTHOR_SEED), 9000);
    folder(&card, &file);
    let done = check(&card, &host_code());
    assert_eq!(done.reason, reason::OK);
    let facts = done.facts.unwrap();
    assert_eq!(facts.key, Some(author()));
    assert_eq!(
        facts.sha256,
        Some(paperpaw_sdk::digest::sha256::digest(&file))
    );
    assert_eq!(facts.signer.as_str(), "PaperPaw release");
    // Another author's key is shown as no name.
    folder(&card, &library(Some(&OTHER_SEED), 9000));
    assert_eq!(
        check(&card, &host_code()).facts.unwrap().signer.as_str(),
        ""
    );
}

#[test]
fn a_library_that_is_unsigned_changed_or_no_plugin_is_refused() {
    let card = MemoryStorage::new();
    folder(&card, &library(None, 5000));
    assert_eq!(check(&card, &host_code()).reason, reason::UNSIGNED);
    let mut file = library(Some(&AUTHOR_SEED), 5000);
    file[3000] ^= 1;
    folder(&card, &file);
    assert_eq!(check(&card, &host_code()).reason, reason::DAMAGED);
    folder(&card, b"short");
    assert_eq!(check(&card, &host_code()).reason, reason::DAMAGED);
    // No record in it: not a plugin's library.
    let mut file = b"not an ELF".to_vec();
    let trailer = sign(&file, &AUTHOR_SEED).to_bytes();
    file.extend(trailer);
    folder(&card, &file);
    assert_eq!(check(&card, &host_code()).reason, reason::DAMAGED);
}

#[test]
fn the_files_of_another_kind_of_device_are_not_here() {
    let card = MemoryStorage::new();
    // On a device of `code.ppn`, and on one that runs libraries: each the
    // other's code of another kind.
    folder(&card, &library(Some(&AUTHOR_SEED), 5000));
    assert_eq!(check(&card, &host()).reason, reason::NOT_HERE);
    card.remove(&format!("{SOURCE}/{FILE}")).unwrap();
    card.write(&format!("{SOURCE}/code.ppn"), b"x").unwrap();
    assert_eq!(check(&card, &host_code()).reason, reason::NOT_HERE);
}

#[test]
fn a_library_installs_and_is_named_by_the_digest_of_its_file() {
    let card = MemoryStorage::new();
    let file = library(Some(&AUTHOR_SEED), 9000);
    folder(&card, &file);
    let mut job = PluginInstall::new(7, 3, SOURCE, 0, Evidence::NONE).unwrap();
    let host = host_code();
    let env = JobEnv {
        storage: &card,
        host: &host,
        built_ins: &Names,
        granted: false,
    };
    let mut asked = None;
    let done = (0..10_000)
        .find_map(|_| match job.step(&env) {
            JobStep::Ask(question) => {
                asked = Some(question);
                job.answer(Answer::Yes);
                None
            }
            JobStep::Done(done) => Some(done),
            _ => None,
        })
        .expect("an install that never ends");
    assert_eq!(done.reason, reason::OK);
    assert_eq!(done.flags, DONE_AT_NEXT_START);
    // The features the library asks for are shown, as a `.ppn`'s are.
    let asked = asked.unwrap();
    assert_eq!(asked.uses.count, 1);
    assert_eq!(asked.uses.keys[0].as_str(), "uses.registry");
    assert_eq!(
        native::installed_code(&card, "dials"),
        Some(paperpaw_sdk::digest::sha256::digest(&file))
    );
    assert_eq!(native::known_author(&card, "dials"), Some(author()));
    assert!(card.exists(&format!("plugins/dials/{FILE}")));
}
