//! The plugin check (`check_file` kind 0) against a card in memory: what
//! each source folder ends with, every reason it can end with, the trust
//! flags, the bounds of a step, an abort and an allocation that fails.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::firmware_install::BuiltIns;
use crate::native_host::plugin_job::PluginHost;
use crate::native_host::shell::tests::FAIL_NTH;
use paperpaw_native::{
    header5::{Host as Loader, X4_PROFILE},
    metadata::{Feature, Host as Offered, Service},
    signature::{public_key, revocation_message, sign, sign_message},
};
use paperpaw_platform::DirEntry;
use paperpaw_sdk::{MemoryStorage, native::keys};
use std::{format, string::String, vec, vec::Vec};

pub(in crate::native_host) const AUTHOR_SEED: [u8; 32] = [3; 32];
pub(in crate::native_host) const OTHER_SEED: [u8; 32] = [4; 32];
pub(in crate::native_host) const RECOVERY_SEED: [u8; 32] = [7; 32];
const SOURCE: &str = "Download/dials";

static SERVICES: [Service<'static>; 1] = [Service {
    key: "core.plugin",
    major: 1,
    semantic_revision: 0,
    min_table_bytes: 0,
}];
static FEATURES: [Feature<'static>; 1] = [Feature {
    key: "uses.registry",
    revision: 1,
}];

pub(in crate::native_host) fn author() -> [u8; 32] {
    public_key(&AUTHOR_SEED)
}

pub(in crate::native_host) fn host() -> PluginHost {
    static NAMED: std::sync::OnceLock<[(&'static str, [u8; 32]); 1]> = std::sync::OnceLock::new();
    PluginHost {
        loader: Some(Loader::x4(1)),
        offered: Offered {
            abi_major: 7,
            minor: 1,
            profile: X4_PROFILE,
            services: &SERVICES,
            features: &FEATURES,
        },
        signers: NAMED.get_or_init(|| [("PaperPaw release", author())]),
    }
}

/// Built into the running image: stats, weather, clock.
pub(in crate::native_host) struct Names;
impl BuiltIns for Names {
    fn count(&self) -> usize {
        3
    }
    fn index_of(&self, id: &str) -> Option<usize> {
        ["stats", "weather", "clock"]
            .iter()
            .position(|known| *known == id)
    }
}

/// A metadata record (`abi/generated/metadata-layout.json`) of the X4 profile.
pub(in crate::native_host) fn record(
    services: &[&str],
    features: &[(&str, bool)],
    minor: u16,
) -> Vec<u8> {
    let target = "esp32c3-riscv32imc-ilp32-soft-le";
    let mut body = Vec::new();
    let export = "PAPERPAW_ENTRY";
    body.extend((10 + export.len() as u16).to_le_bytes());
    body.extend([1, 0]);
    body.extend(20_u32.to_le_bytes());
    body.extend((export.len() as u16).to_le_bytes());
    body.extend(export.as_bytes());
    for key in services {
        body.extend((12 + key.len() as u16).to_le_bytes());
        body.extend((key.len() as u16).to_le_bytes());
        body.extend(1_u16.to_le_bytes());
        body.extend(0_u16.to_le_bytes());
        body.extend(0_u32.to_le_bytes());
        body.extend(key.as_bytes());
    }
    for (key, required) in features {
        body.extend((8 + key.len() as u16).to_le_bytes());
        body.extend((key.len() as u16).to_le_bytes());
        body.extend(1_u16.to_le_bytes());
        body.extend(u16::from(*required).to_le_bytes());
        body.extend(key.as_bytes());
    }
    let total = 32 + target.len() + body.len();
    let mut out = Vec::new();
    out.extend(b"PPAM");
    out.extend(2_u16.to_le_bytes());
    out.extend(32_u16.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend(7_u16.to_le_bytes());
    out.extend(minor.to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(20_u16.to_le_bytes());
    out.extend(0_u32.to_le_bytes());
    out.extend((target.len() as u16).to_le_bytes());
    out.extend((services.len() as u16).to_le_bytes());
    out.extend((features.len() as u16).to_le_bytes());
    out.extend(1_u16.to_le_bytes());
    out.extend(target.as_bytes());
    out.extend(body);
    out
}

pub(in crate::native_host) fn good_record() -> Vec<u8> {
    record(&["core.plugin"], &[("uses.registry", false)], 1)
}

/// A format-5 code file around `record`, signed by `seed` when given.
pub(in crate::native_host) fn code_file(
    record: &[u8],
    seed: Option<&[u8; 32]>,
    image_len: u32,
) -> Vec<u8> {
    let mut header = Header5 {
        target: paperpaw_native::Chip::Esp32C3,
        abi_major: 7,
        needs_minor: 1,
        base: 0x3C00_0000,
        image_len,
        code_at: 1024,
        entry: 64,
        meta_at: 128,
        meta_len: record.len() as u32,
        relocs: 10,
        align: 16,
        hot_at: 0,
        hot_len: 0,
        patches: 0,
        heap_at: None,
        sha256: [0; 32],
        signed: seed.is_some(),
    };
    let mut rest = vec![0_u8; image_len as usize];
    rest[128..128 + record.len()].copy_from_slice(record);
    // Something to tell one file from another, past the record.
    for (i, byte) in rest.iter_mut().enumerate().skip(2048) {
        *byte = (i % 251) as u8;
    }
    rest.extend([0_u8; 40]);
    let mut sha = header.digest();
    sha.update(&rest);
    header.sha256 = sha.finish();
    let mut file = header.to_bytes().to_vec();
    file.extend(rest);
    if let Some(seed) = seed {
        file.extend(sign(&file, seed).to_bytes());
    }
    file
}

pub(in crate::native_host) const INI: &str =
    "[plugin]\nid = dials\nname = Dials\nversion = 1.2.0\n";

/// The folder `dir` of a data plugin.
pub(in crate::native_host) fn data_folder(card: &MemoryStorage, dir: &str) {
    card.write(&format!("{dir}/plugin.ini"), INI.as_bytes())
        .unwrap();
    card.write(&format!("{dir}/strings.ini"), b"hello\n")
        .unwrap();
    card.write(&format!("{dir}/art/logo.txt"), b"abc").unwrap();
}

/// The folder `dir` of a code plugin with `code`.
pub(in crate::native_host) fn code_folder(card: &MemoryStorage, dir: &str, code: &[u8]) {
    card.write(&format!("{dir}/plugin.ini"), INI.as_bytes())
        .unwrap();
    card.write(&format!("{dir}/code.ppn"), code).unwrap();
}

pub(in crate::native_host) fn signed_folder(card: &MemoryStorage, dir: &str) -> Vec<u8> {
    let code = code_file(&good_record(), Some(&AUTHOR_SEED), 4096);
    code_folder(card, dir, &code);
    code
}

/// Runs a check to its end, with the number of steps it took.
fn run(card: &dyn Storage, source: &str) -> (Finished, u32) {
    run_with(card, &host(), source)
}

fn run_with(card: &dyn Storage, host: &PluginHost, source: &str) -> (Finished, u32) {
    let mut job = PluginCheck::new(7, 1, source).unwrap();
    let env = JobEnv {
        storage: card,
        host,
        built_ins: &Names,
        granted: false,
    };
    for steps in 1..10_000 {
        if let JobStep::Done(finished) = job.step(&env) {
            return (finished, steps);
        }
    }
    panic!("a check that never ends");
}

fn reason_of(card: &dyn Storage, source: &str) -> u8 {
    run(card, source).0.reason
}

#[test]
fn a_job_is_a_check_of_the_asker() {
    let job = PluginCheck::new(9, 4, SOURCE).unwrap();
    assert_eq!((job.owner(), job.request(), job.kind()), (9, 4, 3));
}

#[test]
fn a_data_plugin_fits_and_says_what_it_is() {
    let card = MemoryStorage::new();
    data_folder(&card, SOURCE);
    let (done, _) = run(&card, SOURCE);
    assert_eq!(done.reason, reason::OK);
    let facts = done.facts.unwrap();
    assert_eq!(facts.id.as_str(), "dials");
    assert_eq!(facts.version.as_str(), "1.2.0");
    assert_eq!(facts.flags, FLAG_DATA_PLUGIN);
    assert_eq!((facts.key, facts.sha256), (None, None));
    assert_eq!(facts.signer.as_str(), "");
    // The sum of the files an install would copy.
    assert_eq!(facts.bytes, (INI.len() + 6 + 3) as u64);
}

#[test]
fn a_folder_without_a_name_has_the_id_of_its_folder() {
    let card = MemoryStorage::new();
    card.write("Download/plain/plugin.ini", b"[plugin]\nname = Plain\n")
        .unwrap();
    let facts = run(&card, "Download/plain").0.facts.unwrap();
    assert_eq!(facts.id.as_str(), "plain");
    assert_eq!(facts.version.as_str(), "0.1.0");
}

#[test]
fn a_signed_code_plugin_fits_with_its_key_and_digest() {
    let card = MemoryStorage::new();
    let code = signed_folder(&card, SOURCE);
    let (done, _) = run(&card, SOURCE);
    assert_eq!(done.reason, reason::OK);
    let facts = done.facts.unwrap();
    assert_eq!(facts.flags, 0, "a new id: not pinned");
    assert_eq!(facts.key, Some(author()));
    assert_eq!(facts.sha256.unwrap(), code[64..96]);
    assert_eq!(facts.bytes, (INI.len() + code.len()) as u64);
    // The key's name is one the firmware knows; another is shown by its
    // fingerprint (empty here).
    assert_eq!(facts.signer.as_str(), "PaperPaw release");
    let other = code_file(&good_record(), Some(&OTHER_SEED), 4096);
    code_folder(&card, "Download/other", &other);
    let facts = run(&card, "Download/other").0.facts.unwrap();
    assert_eq!(facts.signer.as_str(), "");
    assert_eq!(facts.key, Some(public_key(&OTHER_SEED)));
}

#[test]
fn the_pins_of_beta_3_say_whose_key_it_is() {
    let card = MemoryStorage::new();
    signed_folder(&card, SOURCE);
    // Pinned to the signing key, with the files Beta 3 wrote.
    assert!(paperpaw_sdk::native::remember_author(
        &card,
        "dials",
        &author()
    ));
    let done = run(&card, SOURCE).0;
    assert_eq!(done.reason, 0);
    assert_eq!(done.facts.unwrap().flags, FLAG_PINNED_HERE);
    // Pinned to another key: an install needs a rotation, the check says so.
    assert!(paperpaw_sdk::native::remember_author(
        &card, "dials", &[9; 32]
    ));
    let done = run(&card, SOURCE).0;
    assert_eq!(done.reason, 0);
    assert_eq!(done.facts.unwrap().flags, FLAG_PINNED_OTHER);
}

#[test]
fn a_revocation_that_holds_refuses_and_one_that_does_not_is_ignored() {
    let card = MemoryStorage::new();
    signed_folder(&card, SOURCE);
    let recovery = public_key(&RECOVERY_SEED);
    assert!(keys::remember_first_install(
        &card,
        "dials",
        &author(),
        &recovery
    ));
    let sign_by =
        |seed: &[u8; 32]| sign_message(revocation_message("dials", &author()).as_bytes(), seed);
    // Signed by a key that is neither the author's nor the recovery key: it
    // does not hold (Beta 3 would not keep it; a line written by hand is
    // checked again where it is read).
    let forged = format!(
        "dials {} {}\n",
        paperpaw_sdk::digest::hex::encode(&author()),
        paperpaw_sdk::digest::hex::encode(&sign_by(&OTHER_SEED))
    );
    card.write(".paperpaw/revoked-keys.txt", forged.as_bytes())
        .unwrap();
    assert_eq!(run(&card, SOURCE).0.reason, reason::OK);
    // Signed by the pinned recovery key: it holds.
    let holds = format!(
        "other 00 00\n{}dials {} {}\n",
        forged,
        paperpaw_sdk::digest::hex::encode(&author()),
        paperpaw_sdk::digest::hex::encode(&sign_by(&RECOVERY_SEED))
    );
    card.write(".paperpaw/revoked-keys.txt", holds.as_bytes())
        .unwrap();
    let done = run(&card, SOURCE).0;
    assert_eq!(done.reason, reason::REVOKED);
    // The facts are told with the verdict: the key is the pinned one.
    assert_eq!(done.facts.unwrap().flags, FLAG_PINNED_HERE);
    // The key's own signature holds too.
    let own = format!(
        "dials {} {}",
        paperpaw_sdk::digest::hex::encode(&author()),
        paperpaw_sdk::digest::hex::encode(&sign_by(&AUTHOR_SEED))
    );
    card.write(".paperpaw/revoked-keys.txt", own.as_bytes())
        .unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::REVOKED);
}

#[test]
fn a_revocation_of_another_key_than_the_pinned_one_is_none_of_this() {
    let card = MemoryStorage::new();
    signed_folder(&card, SOURCE);
    // Pinned to another key, whose revocation holds: this plugin's signing
    // key is not it, so the check does not say revoked (it says pinned to
    // another).
    let pinned = public_key(&OTHER_SEED);
    assert!(paperpaw_sdk::native::remember_author(
        &card, "dials", &pinned
    ));
    let line = format!(
        "dials {} {}\n",
        paperpaw_sdk::digest::hex::encode(&pinned),
        paperpaw_sdk::digest::hex::encode(&sign_message(
            revocation_message("dials", &pinned).as_bytes(),
            &OTHER_SEED
        ))
    );
    card.write(".paperpaw/revoked-keys.txt", line.as_bytes())
        .unwrap();
    let done = run(&card, SOURCE).0;
    assert_eq!(done.reason, reason::OK);
    assert_eq!(done.facts.unwrap().flags, FLAG_PINNED_OTHER);
}

#[test]
fn unreadable_pins_are_a_card_error_not_no_pin() {
    let card = MemoryStorage::new();
    signed_folder(&card, SOURCE);
    // A pin that is not a key.
    card.write(".paperpaw/plugin-authors.txt", b"dials\tnot-hex\n")
        .unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::CARD);
    // A pin file larger than any this device writes.
    card.write(".paperpaw/plugin-authors.txt", &vec![b'x'; 100 * 1024])
        .unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::CARD);
    // The first line of a plugin decides, others' lines are not its.
    let hex = paperpaw_sdk::digest::hex::encode(&author());
    card.write(
        ".paperpaw/plugin-authors.txt",
        format!("dials2\t{hex}\ndials\t{hex}\ndials\tgarbage\n").as_bytes(),
    )
    .unwrap();
    assert_eq!(run(&card, SOURCE).0.facts.unwrap().flags, FLAG_PINNED_HERE);
    // A line longer than the buffer, a missing last line end, a CR.
    let mut long = vec![b'y'; 5000];
    long.push(b'\n');
    long.extend(format!("dials\t{hex}\r").bytes());
    card.write(".paperpaw/plugin-authors.txt", &long).unwrap();
    assert_eq!(run(&card, SOURCE).0.facts.unwrap().flags, FLAG_PINNED_HERE);
}

#[test]
fn what_is_not_a_plugin_folder_is_refused() {
    let card = MemoryStorage::new();
    // A bare `.ppn`: reason 4, whatever it holds.
    card.write(
        "Download/dials.ppn",
        &code_file(&good_record(), Some(&AUTHOR_SEED), 4096),
    )
    .unwrap();
    assert_eq!(reason_of(&card, "Download/dials.ppn"), reason::DAMAGED);
    // Nothing at all.
    assert_eq!(reason_of(&card, "Download/missing"), reason::DAMAGED);
    // A folder without its manifest.
    card.write("Download/bare/code.ppn", b"x").unwrap();
    assert_eq!(reason_of(&card, "Download/bare"), reason::DAMAGED);
}

#[test]
fn the_files_of_the_source_are_checked_by_name_and_number() {
    let card = MemoryStorage::new();
    data_folder(&card, SOURCE);
    card.write(&format!("{SOURCE}/.hidden"), b"x").unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::DAMAGED);
    card.remove(&format!("{SOURCE}/.hidden")).unwrap();
    card.write(&format!("{SOURCE}/a b"), b"x").unwrap();
    assert_eq!(
        reason_of(&card, SOURCE),
        reason::DAMAGED,
        "a name no plugin file has"
    );
    card.remove(&format!("{SOURCE}/a b")).unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::OK);
    // More files than a plugin has.
    for n in 0..70 {
        card.write(&format!("{SOURCE}/f{n}.txt"), b"x").unwrap();
    }
    assert_eq!(reason_of(&card, SOURCE), reason::DAMAGED);
    // More folders than a plugin has.
    let card = MemoryStorage::new();
    data_folder(&card, SOURCE);
    for n in 0..20 {
        card.write(&format!("{SOURCE}/d{n}/x.txt"), b"x").unwrap();
    }
    assert_eq!(reason_of(&card, SOURCE), reason::DAMAGED);
}

#[test]
fn ids_that_are_taken_or_unfit_are_refused() {
    let card = MemoryStorage::new();
    for (id, why) in [
        ("core", reason::ID_TAKEN),
        ("stats", reason::ID_TAKEN),
        ("Dials", reason::DAMAGED),
        ("-dials", reason::DAMAGED),
        ("a.b", reason::DAMAGED),
    ] {
        card.write(
            "Download/x/plugin.ini",
            format!("[plugin]\nid = {id}\n").as_bytes(),
        )
        .unwrap();
        assert_eq!(reason_of(&card, "Download/x"), why, "{id}");
    }
    // The folder's name is the id where the manifest names none.
    card.write("Download/core/plugin.ini", b"[plugin]\nname = x\n")
        .unwrap();
    assert_eq!(reason_of(&card, "Download/core"), reason::ID_TAKEN);
    // A version no plugin has.
    card.write("Download/v/plugin.ini", b"[plugin]\nversion = one\n")
        .unwrap();
    assert_eq!(reason_of(&card, "Download/v"), reason::DAMAGED);
    // An id longer than the wire's.
    let long = format!("[plugin]\nid = {}\n", "a".repeat(65));
    card.write("Download/l/plugin.ini", long.as_bytes())
        .unwrap();
    assert_eq!(reason_of(&card, "Download/l"), reason::DAMAGED);
    // A manifest too large to read.
    card.write("Download/m/plugin.ini", &vec![b'#'; 5000])
        .unwrap();
    assert_eq!(reason_of(&card, "Download/m"), reason::DAMAGED);
}

#[test]
fn code_of_another_kind_is_not_on_this_device() {
    let card = MemoryStorage::new();
    card.write(&format!("{SOURCE}/plugin.ini"), INI.as_bytes())
        .unwrap();
    for file in [
        "code.so",
        "code-host.dll",
        "code-host.so",
        "code-host.dylib",
    ] {
        card.write(&format!("{SOURCE}/{file}"), b"x").unwrap();
        assert_eq!(reason_of(&card, SOURCE), reason::NOT_HERE, "{file}");
        card.remove(&format!("{SOURCE}/{file}")).unwrap();
    }
    // `code.ppn` wins when both are there.
    signed_folder(&card, SOURCE);
    card.write(&format!("{SOURCE}/code-host.so"), b"x").unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::OK);
}

/// `code` with the bytes of `edit` changed, as a folder.
fn edited(code: &[u8], edit: impl FnOnce(&mut Vec<u8>)) -> MemoryStorage {
    let card = MemoryStorage::new();
    let mut code = code.to_vec();
    edit(&mut code);
    code_folder(&card, SOURCE, &code);
    card
}

fn good_code() -> Vec<u8> {
    code_file(&good_record(), Some(&AUTHOR_SEED), 4096)
}

fn set16(code: &mut [u8], at: usize, value: u16) {
    code[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

/// A name, the reason and the edit that breaks the header.
type Case = (&'static str, u8, fn(&mut Vec<u8>));

#[test]
fn a_header_the_loader_refuses_ends_with_the_loaders_reason() {
    let code = good_code();
    let cases: [Case; 9] = [
        ("not a plugin", reason::DAMAGED, |c| c[0] = b'X'),
        ("truncated", reason::DAMAGED, |c| c.truncate(60)),
        ("empty", reason::DAMAGED, |c| c.clear()),
        ("older format", reason::OLDER_BUILD, |c| set16(c, 4, 4)),
        ("newer format", reason::NEEDS_NEWER, |c| set16(c, 4, 6)),
        ("another chip", reason::OTHER_CHIP, |c| set16(c, 6, 9)),
        ("older interface", reason::OLDER_BUILD, |c| set16(c, 8, 6)),
        ("newer interface", reason::NEEDS_NEWER, |c| set16(c, 8, 8)),
        ("needs a newer minor", reason::NEEDS_NEWER, |c| {
            set16(c, 10, 2)
        }),
    ];
    for (name, why, edit) in cases {
        let card = edited(&code, edit);
        assert_eq!(reason_of(&card, SOURCE), why, "{name}");
    }
    // A length the header does not say.
    let card = edited(&code, |c| c.truncate(c.len() - 5));
    assert_eq!(reason_of(&card, SOURCE), reason::DAMAGED, "length");
}

#[test]
fn needs_draft_is_the_files_needs_minor_for_a_newer_minor_and_zero_otherwise() {
    let code = good_code();
    let draft = |edit: fn(&mut Vec<u8>)| {
        let card = edited(&code, edit);
        let (finished, _) = run(&card, SOURCE);
        (finished.reason, finished.needs_draft)
    };
    // The revision the file needs (the header's `needs_minor`).
    assert_eq!(draft(|c| set16(c, 10, 2)), (reason::NEEDS_NEWER, 2));
    assert_eq!(
        draft(|c| set16(c, 10, 60_000)),
        (reason::NEEDS_NEWER, 60_000)
    );
    // A newer format or interface names no number of this interface; nor
    // does any other end.
    assert_eq!(draft(|c| set16(c, 4, 6)), (reason::NEEDS_NEWER, 0));
    assert_eq!(draft(|c| set16(c, 8, 8)), (reason::NEEDS_NEWER, 0));
    assert_eq!(draft(|c| set16(c, 6, 9)), (reason::OTHER_CHIP, 0));
    assert_eq!(draft(|c| c[0] = b'X'), (reason::DAMAGED, 0));
    assert_eq!(draft(|_| {}).1, 0);
}

#[test]
fn a_changed_byte_is_damage_whether_in_the_image_or_the_signature() {
    let code = good_code();
    for at in [100, 3000, 4000, code.len() - 60, code.len() - 1] {
        let card = edited(&code, |c| c[at] ^= 0x40);
        let why = reason_of(&card, SOURCE);
        assert_eq!(why, reason::DAMAGED, "byte {at}");
    }
    // A signature of another file: the digest holds, the signature does not.
    let mut forged = good_code();
    let other = code_file(&good_record(), Some(&AUTHOR_SEED), 8192);
    let at = forged.len() - 112;
    forged[at + 44..at + 108].copy_from_slice(&other[other.len() - 68..other.len() - 4]);
    let card = edited(&forged, |_| {});
    assert_eq!(reason_of(&card, SOURCE), reason::DAMAGED);
    // A trailer that is not one.
    let card = edited(&code, |c| {
        let at = c.len() - 112;
        c[at] = b'Z';
    });
    assert_eq!(reason_of(&card, SOURCE), reason::DAMAGED);
    // A trailer that signs another length.
    let card = edited(&code, |c| {
        let at = c.len() - 112;
        c[at + 8] ^= 1;
    });
    assert_eq!(reason_of(&card, SOURCE), reason::DAMAGED);
}

#[test]
fn unsigned_code_is_refused_as_unsigned() {
    let card = MemoryStorage::new();
    code_folder(&card, SOURCE, &code_file(&good_record(), None, 4096));
    let done = run(&card, SOURCE).0;
    assert_eq!(done.reason, reason::UNSIGNED);
    assert_eq!(done.facts, None);
    // Damage is told before the missing signature.
    let card = MemoryStorage::new();
    let mut code = code_file(&good_record(), None, 4096);
    code[3000] ^= 1;
    code_folder(&card, SOURCE, &code);
    assert_eq!(reason_of(&card, SOURCE), reason::DAMAGED);
}

#[test]
fn the_metadata_record_is_checked_against_what_the_device_offers() {
    let cases = [
        (
            "a required feature the device lacks",
            record(&["core.plugin"], &[("uses.wifi", true)], 1),
            reason::NEEDS_NEWER,
        ),
        (
            "a service the device lacks",
            record(&["core.plugin", "core.ui"], &[], 1),
            reason::NEEDS_NEWER,
        ),
        (
            "an optional feature the device lacks is fine",
            record(&["core.plugin"], &[("uses.wifi", false)], 1),
            reason::OK,
        ),
        (
            "a newer minor in the record",
            record(&["core.plugin"], &[], 2),
            reason::NEEDS_NEWER,
        ),
    ];
    for (name, record, why) in cases {
        let card = MemoryStorage::new();
        code_folder(&card, SOURCE, &code_file(&record, Some(&AUTHOR_SEED), 4096));
        let got = reason_of(&card, SOURCE);
        // A newer minor in the record than in the header is a record that
        // disagrees (damage), as the minor of the record alone is not read
        // by the header's check.
        let want = if name.contains("newer minor") {
            reason::OTHER_CHIP
        } else {
            why
        };
        assert_eq!(got, want, "{name}");
    }
    // A record that is not one.
    let card = MemoryStorage::new();
    let mut bad = good_record();
    bad[0] = b'Q';
    code_folder(&card, SOURCE, &code_file(&bad, Some(&AUTHOR_SEED), 4096));
    assert_eq!(reason_of(&card, SOURCE), reason::DAMAGED);
    // A record that says another interface than the header.
    let card = MemoryStorage::new();
    let mut other = good_record();
    other[12..14].copy_from_slice(&6_u16.to_le_bytes());
    code_folder(&card, SOURCE, &code_file(&other, Some(&AUTHOR_SEED), 4096));
    assert_eq!(reason_of(&card, SOURCE), reason::OTHER_CHIP);
}

#[test]
fn the_room_for_code_is_checked_in_place_of_the_plugins_own_version() {
    let card = MemoryStorage::new();
    signed_folder(&card, SOURCE);
    // 4,096 bytes of image in 4,096-byte pieces: one piece, one plugin.
    let room = |total, used, placed| {
        let room =
            paperpaw_sdk::native::CodeRoom::new(total, used, 4096).with_plugins(placed, Some(4));
        assert!(paperpaw_sdk::native::keep_code_room(&card, Some(&room)));
    };
    room(8192, 4096, 1);
    assert_eq!(reason_of(&card, SOURCE), reason::OK);
    room(8192, 8192, 2);
    assert_eq!(reason_of(&card, SOURCE), reason::NO_ROOM);
    // The installed plugin of this id gives its room back to its update.
    let installed = code_file(&good_record(), Some(&OTHER_SEED), 4096);
    card.write("plugins/dials/code.ppn", &installed).unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::OK);
    // Too many plugins for the index.
    room(1 << 20, 4096, 4);
    card.remove("plugins/dials/code.ppn").unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::NO_ROOM);
    // Where the shell says nothing, the code is taken to fit.
    assert!(paperpaw_sdk::native::keep_code_room(&card, None));
    assert_eq!(reason_of(&card, SOURCE), reason::OK);
}

#[test]
fn an_update_is_signed_over_its_own_header_not_the_installed_ones() {
    let card = MemoryStorage::new();
    signed_folder(&card, SOURCE);
    let room = paperpaw_sdk::native::CodeRoom::new(1 << 20, 8192, 4096).with_plugins(1, Some(4));
    assert!(paperpaw_sdk::native::keep_code_room(&card, Some(&room)));
    // The installed version is a bigger image: its header is another one.
    let installed = code_file(&good_record(), Some(&AUTHOR_SEED), 8192);
    card.write("plugins/dials/code.ppn", &installed).unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::OK);
}

#[test]
fn a_code_file_in_other_case_is_code_not_a_data_plugin() {
    // A FAT lists the case a file was stored in and finds it in any.
    let card = MemoryStorage::new();
    card.write(&format!("{SOURCE}/plugin.ini"), INI.as_bytes())
        .unwrap();
    let unsigned = code_file(&good_record(), None, 4096);
    card.write(&format!("{SOURCE}/CODE.PPN"), &unsigned)
        .unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::UNSIGNED);
    let signed = code_file(&good_record(), Some(&AUTHOR_SEED), 4096);
    card.write(&format!("{SOURCE}/CODE.PPN"), &signed).unwrap();
    let done = run(&card, SOURCE).0;
    assert_eq!(done.reason, reason::OK);
    let facts = done.facts.unwrap();
    assert_eq!(facts.key, Some(author()));
    assert_eq!(facts.flags & FLAG_DATA_PLUGIN, 0);
    // Other chips' names too.
    let card = MemoryStorage::new();
    card.write(&format!("{SOURCE}/plugin.ini"), INI.as_bytes())
        .unwrap();
    card.write(&format!("{SOURCE}/Code.So"), b"x").unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::NOT_HERE);
}

#[test]
fn the_revocations_are_read_a_line_with_a_signature_at_a_time() {
    let card = MemoryStorage::new();
    signed_folder(&card, SOURCE);
    assert!(keys::remember_first_install(
        &card,
        "dials",
        &author(),
        &public_key(&RECOVERY_SEED)
    ));
    let sign_by =
        |seed: &[u8; 32]| sign_message(revocation_message("dials", &author()).as_bytes(), seed);
    let hex = paperpaw_sdk::digest::hex::encode;
    let forged = format!("dials {} {}\n", hex(&author()), hex(&sign_by(&OTHER_SEED)));
    // 300 lines that name the key and do not hold, then one that does.
    let mut file = forged.repeat(300);
    file.push_str(&format!(
        "dials {} {}\n",
        hex(&author()),
        hex(&sign_by(&RECOVERY_SEED))
    ));
    card.write(".paperpaw/revoked-keys.txt", file.as_bytes())
        .unwrap();
    let (done, steps) = run(&card, SOURCE);
    assert_eq!(done.reason, reason::REVOKED);
    // One line a step: the trust phase took as many steps as lines.
    assert!(steps >= 300, "{steps} steps");
    // Lines of no key at all do not cost a step each.
    let mut quiet = String::from("other 00 00\n").repeat(2000);
    quiet.push_str(&forged);
    card.write(".paperpaw/revoked-keys.txt", quiet.as_bytes())
        .unwrap();
    let (done, steps) = run(&card, SOURCE);
    assert_eq!(done.reason, reason::OK);
    assert!(steps < 100, "{steps} steps");
    // A long line is skipped whole and the next one still read.
    let mut long = "x".repeat(10_000);
    long.push('\n');
    long.push_str(&format!(
        "dials {} {}",
        hex(&author()),
        hex(&sign_by(&AUTHOR_SEED))
    ));
    card.write(".paperpaw/revoked-keys.txt", long.as_bytes())
        .unwrap();
    assert_eq!(reason_of(&card, SOURCE), reason::REVOKED);
}

#[test]
fn a_check_changes_nothing_on_the_card() {
    let card = MemoryStorage::new();
    signed_folder(&card, SOURCE);
    paperpaw_sdk::native::remember_author(&card, "dials", &author());
    let before = snapshot(&card);
    for _ in 0..2 {
        let _ = run(&card, SOURCE);
    }
    assert_eq!(snapshot(&card), before);
}

pub(in crate::native_host) fn snapshot(card: &MemoryStorage) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut dirs = vec![String::new()];
    while let Some(dir) = dirs.pop() {
        let mut entries = Vec::new();
        card.list(&dir, &mut |e: DirEntry<'_>| {
            entries.push((String::from(e.name), e.is_dir));
        })
        .unwrap();
        for (name, is_dir) in entries {
            let path = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if is_dir {
                dirs.push(path);
            } else {
                let size = card.size(&path).unwrap() as usize;
                let mut data = vec![0; size];
                let _ = card.read_at(&path, 0, &mut data).unwrap();
                files.push((path, data));
            }
        }
    }
    files.sort();
    files
}

/// A card that counts what a step reads and lists, and can fail reads and
/// size questions of one path.
pub(in crate::native_host) struct Counting {
    pub(in crate::native_host) inner: MemoryStorage,
    pub(in crate::native_host) most_read: Cell<usize>,
    listed: Cell<usize>,
    pub(in crate::native_host) most_listed: Cell<usize>,
    pub(in crate::native_host) fail: RefCell<Option<&'static str>>,
    /// Reads of the paths holding the text return at most this many bytes.
    short: RefCell<Option<(&'static str, usize)>>,
}

use core::cell::{Cell, RefCell};

impl Counting {
    pub(in crate::native_host) fn new(inner: MemoryStorage) -> Self {
        Self {
            inner,
            most_read: Cell::new(0),
            listed: Cell::new(0),
            most_listed: Cell::new(0),
            fail: RefCell::new(None),
            short: RefCell::new(None),
        }
    }

    fn failing(&self, path: &str) -> bool {
        self.fail.borrow().is_some_and(|part| path.contains(part))
    }
}

impl Storage for Counting {
    fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if self.failing(path) {
            return Err(StorageError::Io);
        }
        self.most_read.set(self.most_read.get().max(buf.len()));
        let cut = (*self.short.borrow())
            .filter(|(part, _)| path.contains(part))
            .map_or(buf.len(), |(_, most)| most.min(buf.len()));
        self.inner.read_at(path, at, &mut buf[..cut])
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        if self.failing(path) {
            return Err(StorageError::Io);
        }
        self.inner.size(path)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.inner.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.inner.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(d, v)
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        let mut shown = 0;
        let next = self.inner.list_from(dir, from, &mut |entry| {
            let taken = visit(entry);
            if taken {
                shown += 1;
            }
            taken
        })?;
        self.listed.set(shown);
        self.most_listed.set(self.most_listed.get().max(shown));
        Ok(next)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.inner.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.inner.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.inner.create_dir(p)
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(p)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.inner.sync()
    }
}

#[test]
fn a_step_reads_and_lists_a_bounded_piece() {
    let inner = MemoryStorage::new();
    // 60 files in one folder, a code file of 40 KiB.
    for n in 0..60 {
        inner.write(&format!("{SOURCE}/f{n}.txt"), b"x").unwrap();
    }
    inner
        .write(&format!("{SOURCE}/plugin.ini"), INI.as_bytes())
        .unwrap();
    let code = code_file(&good_record(), Some(&AUTHOR_SEED), 40 * 1024);
    inner.write(&format!("{SOURCE}/code.ppn"), &code).unwrap();
    let card = Counting::new(inner);
    let (done, steps) = run(&card, SOURCE);
    assert_eq!(done.reason, reason::OK);
    assert!(card.most_listed.get() <= plugin_folder::PIECE);
    assert!(card.most_read.get() <= 4096, "{}", card.most_read.get());
    // The walk takes 62 entries in pieces of 16, the body 10 pieces of 4 KiB.
    assert!(steps >= 4 + 10, "{steps} steps");
    assert_eq!(
        done.facts.unwrap().bytes,
        (INI.len() + 60 + code.len()) as u64
    );
}

#[test]
fn a_card_that_fails_ends_with_a_card_error() {
    let inner = MemoryStorage::new();
    signed_folder(&inner, SOURCE);
    let card = Counting::new(inner);
    for part in ["code.ppn", "plugin.ini"] {
        *card.fail.borrow_mut() = Some(part);
        assert_eq!(reason_of(&card, SOURCE), reason::CARD, "{part}");
    }
    // A pin file that fails to read.
    card.inner
        .write(".paperpaw/plugin-authors.txt", b"x\ty\n")
        .unwrap();
    *card.fail.borrow_mut() = Some("plugin-authors");
    assert_eq!(reason_of(&card, SOURCE), reason::CARD);
}

#[test]
fn a_stopped_job_ends_with_its_reason_and_holds_nothing() {
    let card = MemoryStorage::new();
    signed_folder(&card, SOURCE);
    let mut job = PluginCheck::new(1, 1, SOURCE).unwrap();
    let env = JobEnv {
        storage: &card,
        host: &host(),
        built_ins: &Names,
        granted: false,
    };
    assert_eq!(job.step(&env), JobStep::More);
    assert_eq!(job.step(&env), JobStep::More);
    assert!(job.buf.capacity() > 0);
    job.abort(reason::STOPPED);
    assert_eq!(job.buf.capacity(), 0, "the buffer goes with the abort");
    assert_eq!(
        job.step(&env),
        JobStep::Done(Finished::refused(reason::STOPPED))
    );
    // Aborted before its first step.
    let mut job = PluginCheck::new(1, 1, SOURCE).unwrap();
    job.abort(reason::STOPPED);
    assert_eq!(
        job.step(&env),
        JobStep::Done(Finished::refused(reason::STOPPED))
    );
}

/// A card whose calls do not count for an armed allocation failure: only the
/// job's own allocations do.
pub(in crate::native_host) struct Shielded<'a>(pub(in crate::native_host) &'a MemoryStorage);

fn shielded<T>(call: impl FnOnce() -> T) -> T {
    let armed = FAIL_NTH.with(|n| n.replace(0));
    let result = call();
    FAIL_NTH.with(|n| n.set(armed));
    result
}

impl Storage for Shielded<'_> {
    fn read_at(&self, p: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        shielded(|| self.0.read_at(p, at, buf))
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        shielded(|| self.0.size(p))
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        shielded(|| self.0.write(p, d))
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        shielded(|| self.0.append(p, d))
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        shielded(|| self.0.list(d, v))
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        // The visitor allocates too: its failure counts, the card's does not.
        let armed = Cell::new(FAIL_NTH.with(|n| n.replace(0)));
        let result = self.0.list_from(dir, from, &mut |entry| {
            FAIL_NTH.with(|n| n.set(armed.get()));
            let taken = visit(entry);
            armed.set(FAIL_NTH.with(|n| n.replace(0)));
            taken
        });
        FAIL_NTH.with(|n| n.set(armed.get()));
        result
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        shielded(|| self.0.remove(p))
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        shielded(|| self.0.rename(f, t))
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        shielded(|| self.0.create_dir(p))
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        shielded(|| self.0.remove_dir_all(p))
    }
    fn sync(&self) -> Result<(), StorageError> {
        shielded(|| self.0.sync())
    }
}

#[test]
fn an_allocation_that_fails_ends_the_check_with_reason_1_and_never_panics() {
    let card = MemoryStorage::new();
    signed_folder(&card, SOURCE);
    let shield = Shielded(&card);
    let env = JobEnv {
        storage: &shield,
        host: &host(),
        built_ins: &Names,
        granted: false,
    };
    let (mut refused, mut at_submit) = (0, 0);
    // The Nth allocation of the job, for each N up to the number it makes.
    for nth in 1..300_u32 {
        FAIL_NTH.with(|n| n.set(nth));
        let result = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
            let mut job = PluginCheck::new(7, 1, SOURCE)?;
            for _ in 0..10_000 {
                if let JobStep::Done(finished) = job.step(&env) {
                    return Some(finished);
                }
            }
            panic!("a check that never ends");
        }));
        FAIL_NTH.with(|n| n.set(0));
        let done = result.expect("an allocation that fails never panics");
        let Some(done) = done else {
            at_submit += 1;
            continue;
        };
        if done.reason == reason::OTHER {
            refused += 1;
            assert_eq!(done.facts, None);
        } else {
            // Past the job's last allocation.
            assert_eq!(done.reason, reason::OK, "allocation {nth}");
        }
    }
    assert_eq!(at_submit, 1, "the job's own path is its first allocation");
    assert!(refused >= 4, "{refused} allocations were refused");
}

#[test]
fn the_facts_slot_is_not_much_larger_than_a_firmware_check_s() {
    // The per-plugin result slot holds either (`device.md` 5.1, Kernel's
    // ruling): 4 slots device-wide, so a few hundred bytes at most.
    let plugin = core::mem::size_of::<PluginFacts>();
    let firmware = core::mem::size_of::<paperpaw_platform::firmware::CheckFacts>();
    std::eprintln!("PluginFacts {plugin} B, CheckFacts {firmware} B");
    assert!(plugin <= 320, "{plugin}");
    assert!(plugin <= firmware + 96, "{plugin} against {firmware}");
}

#[test]
fn the_job_is_small_enough_to_pass_by_value_on_the_x4() {
    // Frames hold 2 KiB there; the running signature hash lives in a box.
    assert!(
        core::mem::size_of::<PluginCheck>() <= 1024,
        "{} bytes",
        core::mem::size_of::<PluginCheck>()
    );
}

#[test]
fn a_read_that_returns_less_than_asked_does_not_end_the_revocation_scan() {
    let inner = MemoryStorage::new();
    signed_folder(&inner, SOURCE);
    assert!(keys::remember_first_install(
        &inner,
        "dials",
        &author(),
        &public_key(&RECOVERY_SEED)
    ));
    let hex = paperpaw_sdk::digest::hex::encode;
    let sign_by =
        |seed: &[u8; 32]| sign_message(revocation_message("dials", &author()).as_bytes(), seed);
    // Lines of other plugins, then the one that holds.
    let mut file = String::from("other 00 00\n").repeat(300);
    file.push_str(&format!(
        "dials {} {}\n",
        hex(&author()),
        hex(&sign_by(&RECOVERY_SEED))
    ));
    inner
        .write(".paperpaw/revoked-keys.txt", file.as_bytes())
        .unwrap();
    let card = Counting::new(inner);
    // Reads of 7 bytes: a short read is not the end of the file.
    *card.short.borrow_mut() = Some(("revoked-keys", 7));
    assert_eq!(reason_of(&card, SOURCE), reason::REVOKED);
}
