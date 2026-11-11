//! The plugin install job against a card in memory: the whole install, every
//! refusal reason, the confirmation (and the grant that skips only it), a
//! source that changes after its copy, a cut after each card action, an
//! abort and an allocation that fails.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::plugin_check::tests::{
    AUTHOR_SEED, Counting, INI, Names, OTHER_SEED, RECOVERY_SEED, Shielded, author, code_file,
    good_record, host, record, snapshot,
};
use crate::native_host::plugin_job::{FLAG_DATA_PLUGIN, PluginHost};
use crate::native_host::plugin_rules::{Author, Rotation};
use crate::native_host::plugin_swap::{self, Replayed, replay, tests::Cut};
use crate::native_host::shell::tests::FAIL_NTH;
use paperpaw_native::signature::{public_key, rotation_message, sign_message};
use paperpaw_sdk::{MemoryStorage, native};
use std::{format, vec, vec::Vec};

const SOURCE: &str = "Download/dials";
const REPLACE: u32 = FLAG_REPLACE;

fn ini(version: &str) -> String {
    format!("[plugin]\nid = dials\nname = Dials\nversion = {version}\n")
}

fn recovery() -> [u8; 32] {
    public_key(&RECOVERY_SEED)
}

/// A data plugin source of `version`.
fn data_source(card: &MemoryStorage, version: &str) {
    card.write(&format!("{SOURCE}/plugin.ini"), ini(version).as_bytes())
        .unwrap();
    card.write(&format!("{SOURCE}/strings.ini"), b"hello\n")
        .unwrap();
    card.write(&format!("{SOURCE}/art/logo.txt"), b"abc")
        .unwrap();
}

/// A code plugin source of `version` signed by `seed`; the code's digest.
fn code_source(card: &MemoryStorage, version: &str, seed: &[u8; 32]) -> [u8; 32] {
    code_source_with(card, version, &good_record(), seed)
}

fn code_source_with(
    card: &MemoryStorage,
    version: &str,
    record: &[u8],
    seed: &[u8; 32],
) -> [u8; 32] {
    let code = code_file(record, Some(seed), 4096);
    card.write(&format!("{SOURCE}/plugin.ini"), ini(version).as_bytes())
        .unwrap();
    card.write(&format!("{SOURCE}/code.ppn"), &code).unwrap();
    code[64..96].try_into().unwrap()
}

/// The plugin installed at `version`, with its code, pinned to `pinned`.
fn installed(card: &MemoryStorage, version: &str, code: Option<&[u8]>, pinned: Option<[u8; 32]>) {
    card.write("plugins/dials/plugin.ini", ini(version).as_bytes())
        .unwrap();
    card.write("plugins/dials/old.txt", b"old").unwrap();
    if let Some(code) = code {
        card.write("plugins/dials/code.ppn", code).unwrap();
    }
    if let Some(key) = pinned {
        assert!(native::keys::remember_first_install(
            card,
            "dials",
            &key,
            &recovery()
        ));
    }
}

fn catalog(key: [u8; 32], digest: [u8; 32]) -> Evidence {
    Evidence::new(
        Some(Author {
            author: Some(key),
            recovery: Some(recovery()),
            sha256: Some(digest),
        }),
        &[],
    )
    .unwrap()
}

/// What running a job to its end showed.
struct Ran {
    done: Finished,
    asked: Vec<PluginQuestion>,
}

fn drive(card: &dyn Storage, mut job: PluginInstall, granted: bool, answer: Answer) -> Ran {
    drive_with(card, &host(), &mut job, granted, answer)
}

fn drive_with(
    card: &dyn Storage,
    host: &PluginHost,
    job: &mut PluginInstall,
    granted: bool,
    answer: Answer,
) -> Ran {
    let env = JobEnv {
        storage: card,
        host,
        built_ins: &Names,
        granted,
    };
    let mut asked = Vec::new();
    for _ in 0..10_000 {
        match job.step(&env) {
            JobStep::More => {}
            JobStep::Wait => panic!("waited with no question"),
            JobStep::Unload(_) => panic!("an install closes no plugin"),
            JobStep::Forget(_) => panic!("an install forgets nothing"),
            JobStep::Ask(question) => {
                // The harness's own allocation is not the job's.
                let armed = FAIL_NTH.with(|n| n.replace(0));
                asked.push(question);
                FAIL_NTH.with(|n| n.set(armed));
                job.answer(answer);
            }
            JobStep::Done(done) => return Ran { done, asked },
        }
    }
    panic!("an install that never ends");
}

fn job(flags: u32, evidence: Evidence) -> PluginInstall {
    PluginInstall::new(7, 3, SOURCE, flags, evidence).unwrap()
}

fn install(card: &dyn Storage, flags: u32, evidence: Evidence, answer: Answer) -> Ran {
    drive(card, job(flags, evidence), false, answer)
}

fn text(card: &MemoryStorage, path: &str) -> Option<String> {
    let size = card.size(path).ok()? as usize;
    let mut data = vec![0; size];
    card.read_at(path, 0, &mut data).ok()?;
    String::from_utf8(data).ok()
}

fn clean(card: &MemoryStorage) -> bool {
    !card.exists("plugins/.journal")
        && !card.exists("plugins/.dials.part/plugin.ini")
        && !card.exists("plugins/.old-dials/plugin.ini")
}

#[test]
fn a_data_plugin_is_copied_checked_asked_about_and_swapped_in() {
    let card = MemoryStorage::new();
    data_source(&card, "1.0.0");
    let ran = install(&card, 0, Evidence::NONE, Answer::Yes);
    assert_eq!(ran.done.reason, reason::OK);
    assert_eq!(ran.done.changed.unwrap().as_str(), "dials");
    assert_eq!(ran.done.flags, 0, "a data plugin loads at once");
    let [question] = ran.asked[..] else {
        panic!("{:?}", ran.asked)
    };
    assert_eq!(
        (
            question.id.as_str(),
            question.name.as_str(),
            question.version.as_str()
        ),
        ("dials", "Dials", "1.0.0")
    );
    assert!(!question.sideload && question.key.is_none() && question.uses.count == 0);
    assert_eq!((question.owner, question.request, question.kind), (7, 3, 0));
    assert_eq!(
        text(&card, "plugins/dials/strings.ini").as_deref(),
        Some("hello\n")
    );
    assert_eq!(
        text(&card, "plugins/dials/art/logo.txt").as_deref(),
        Some("abc")
    );
    assert!(!card.exists("plugins/dials/.install-marks"), "settled");
    assert!(clean(&card));
    // The source stays as it was.
    assert!(card.exists(&format!("{SOURCE}/plugin.ini")));
}

#[test]
fn a_sideloaded_code_plugin_asks_for_the_key_and_pins_it_at_commit() {
    let card = MemoryStorage::new();
    let digest = code_source(&card, "1.0.0", &AUTHOR_SEED);
    let ran = install(&card, 0, Evidence::NONE, Answer::Yes);
    assert_eq!(ran.done.reason, reason::OK);
    assert_eq!(
        ran.done.flags, DONE_AT_NEXT_START,
        "code loads at the next start"
    );
    let q = ran.asked[0];
    assert!(q.sideload, "not from a catalog");
    assert_eq!(q.key, Some(author()));
    assert_eq!(q.signer.as_str(), "PaperPaw release");
    assert_eq!(q.uses.count, 1);
    assert_eq!(q.uses.keys[0].as_str(), "uses.registry");
    assert_eq!(native::known_author(&card, "dials"), Some(author()));
    assert_eq!(native::installed_code(&card, "dials"), Some(digest));
    assert_eq!(native::keys::known_recovery(&card, "dials"), None);
    assert!(card.exists("plugins/dials/code.ppn") && clean(&card));
}

#[test]
fn a_catalog_install_pins_the_author_and_its_recovery_key() {
    let card = MemoryStorage::new();
    let digest = code_source(&card, "1.0.0", &AUTHOR_SEED);
    let ran = install(&card, 0, catalog(author(), digest), Answer::Yes);
    assert_eq!(ran.done.reason, reason::OK);
    assert!(!ran.asked[0].sideload);
    assert_eq!(native::known_author(&card, "dials"), Some(author()));
    assert_eq!(
        native::keys::known_recovery(&card, "dials"),
        Some(recovery())
    );
}

#[test]
fn an_update_through_a_rotation_moves_the_pin_only_at_commit() {
    let card = MemoryStorage::new();
    let old = code_file(&good_record(), Some(&OTHER_SEED), 4096);
    installed(&card, "1.0.0", Some(&old), Some(public_key(&OTHER_SEED)));
    assert!(native::remember_code(
        &card,
        "dials",
        &old[64..96].try_into().unwrap()
    ));
    let digest = code_source(&card, "1.1.0", &AUTHOR_SEED);
    let rotation = Rotation {
        to: author(),
        signature: sign_message(
            rotation_message("dials", &public_key(&OTHER_SEED), &author()).as_bytes(),
            &RECOVERY_SEED,
        ),
    };
    let evidence = Evidence::new(None, &[rotation]).unwrap();
    // Declined: nothing moved.
    let before = snapshot(&card);
    let ran = install(&card, REPLACE, evidence, Answer::No);
    assert_eq!(ran.done.reason, reason::DECLINED);
    assert_eq!(
        snapshot(&card),
        before,
        "declined leaves everything as before"
    );
    // Confirmed.
    let ran = install(&card, REPLACE, evidence, Answer::Yes);
    assert_eq!(ran.done.reason, reason::OK);
    assert_eq!(ran.asked[0].relation, Relation::Newer);
    assert_eq!(native::known_author(&card, "dials"), Some(author()));
    assert_eq!(native::installed_code(&card, "dials"), Some(digest));
    assert!(
        !card.exists("plugins/dials/old.txt"),
        "the old folder is gone"
    );
    assert!(clean(&card));
}

#[test]
fn declined_and_dismissed_leave_the_card_as_it_was() {
    for (answer, why) in [
        (Answer::No, reason::DECLINED),
        (Answer::Dismissed, reason::DISMISSED),
    ] {
        let card = MemoryStorage::new();
        code_source(&card, "1.0.0", &AUTHOR_SEED);
        let before = snapshot(&card);
        assert_eq!(install(&card, 0, Evidence::NONE, answer).done.reason, why);
        assert_eq!(snapshot(&card), before);
        assert_eq!(native::known_author(&card, "dials"), None);
    }
}

/// A removed plugin whose secrets stayed (`plugin_unforgotten`): a plugin of
/// its id is not installed until the host has forgotten them.
#[test]
fn an_id_whose_secrets_stayed_after_a_removal_is_not_installed() {
    let card = MemoryStorage::new();
    data_source(&card, "1.0.0");
    assert!(crate::native_host::plugin_unforgotten::add(&card, "dials"));
    let before = snapshot(&card);
    assert_eq!(
        install(&card, 0, Evidence::NONE, Answer::Yes).done.reason,
        reason::ID_TAKEN
    );
    assert_eq!(snapshot(&card), before);
    // The host forgot: the id is free again.
    crate::native_host::plugin_unforgotten::retry(&card, &mut |_| true);
    assert_eq!(
        install(&card, 0, Evidence::NONE, Answer::Yes).done.reason,
        reason::OK
    );
}

#[test]
fn the_source_is_refused_before_anything_is_copied() {
    let card = MemoryStorage::new();
    data_source(&card, "1.0.0");
    // An installed plugin of the id needs the replace flag.
    installed(&card, "0.9.0", None, None);
    let before = snapshot(&card);
    assert_eq!(
        install(&card, 0, Evidence::NONE, Answer::Yes).done.reason,
        reason::ID_TAKEN
    );
    assert_eq!(snapshot(&card), before);
    // A bare `.ppn`, a path naming nothing.
    card.write("Download/x.ppn", b"PPNP").unwrap();
    for path in ["Download/x.ppn", "Download/none"] {
        let job = PluginInstall::new(7, 3, path, 0, Evidence::NONE).unwrap();
        assert_eq!(
            drive(&card, job, false, Answer::Yes).done.reason,
            reason::DAMAGED,
            "{path}"
        );
    }
    // Built in.
    let card = MemoryStorage::new();
    card.write("Download/clock/plugin.ini", b"[plugin]\nid = clock\n")
        .unwrap();
    let job = PluginInstall::new(7, 3, "Download/clock", 0, Evidence::NONE).unwrap();
    assert_eq!(
        drive(&card, job, false, Answer::Yes).done.reason,
        reason::ID_TAKEN
    );
    // The reserved id.
    card.write("Download/core/plugin.ini", b"[plugin]\nid = core\n")
        .unwrap();
    let job = PluginInstall::new(7, 3, "Download/core", 0, Evidence::NONE).unwrap();
    assert_eq!(
        drive(&card, job, false, Answer::Yes).done.reason,
        reason::ID_TAKEN
    );
    assert!(!card.exists("plugins/.core.part/plugin.ini"));
}

#[test]
fn a_plugin_of_the_other_kind_is_none_to_replace() {
    let card = MemoryStorage::new();
    let code = code_file(&good_record(), Some(&AUTHOR_SEED), 4096);
    installed(&card, "0.9.0", Some(&code), Some(author()));
    data_source(&card, "1.0.0");
    assert_eq!(
        install(&card, REPLACE, Evidence::NONE, Answer::Yes)
            .done
            .reason,
        reason::ID_TAKEN
    );
    assert!(clean(&card) && card.exists("plugins/dials/code.ppn"));
    // And the other way round.
    let card = MemoryStorage::new();
    installed(&card, "0.9.0", None, None);
    code_source(&card, "1.0.0", &AUTHOR_SEED);
    assert_eq!(
        install(&card, REPLACE, Evidence::NONE, Answer::Yes)
            .done
            .reason,
        reason::ID_TAKEN
    );
    assert!(clean(&card));
}

#[test]
fn an_older_or_the_same_version_needs_the_flag_and_is_named_in_the_question() {
    for (installed_version, new, relation) in [
        ("1.0.0", "1.0.0", Relation::Same),
        ("2.0.0", "1.0.0", Relation::Older),
    ] {
        let card = MemoryStorage::new();
        installed(&card, installed_version, None, None);
        data_source(&card, new);
        let before = snapshot(&card);
        let ran = install(&card, REPLACE, Evidence::NONE, Answer::Yes);
        assert_eq!(ran.done.reason, reason::OLDER_NOT_ALLOWED, "{new}");
        assert_eq!(snapshot(&card), before, "the stage is removed");
        let ran = install(&card, REPLACE | FLAG_OLDER, Evidence::NONE, Answer::Yes);
        assert_eq!(ran.done.reason, reason::OK);
        assert_eq!(ran.asked[0].relation, relation);
    }
}

#[test]
fn the_trust_rules_refuse_with_their_reasons_and_leave_the_card_alone() {
    let new_key = public_key(&OTHER_SEED);
    // 5: a new id whose catalog names another key.
    let card = MemoryStorage::new();
    let digest = code_source(&card, "1.0.0", &AUTHOR_SEED);
    let before = snapshot(&card);
    assert_eq!(
        install(&card, 0, catalog(new_key, digest), Answer::Yes)
            .done
            .reason,
        5
    );
    assert_eq!(snapshot(&card), before);
    // 18: a digest other than the named one.
    assert_eq!(
        install(&card, 0, catalog(author(), [0; 32]), Answer::Yes)
            .done
            .reason,
        18
    );
    assert_eq!(snapshot(&card), before);
    // 6: pinned to another key, no rotation.
    let card = MemoryStorage::new();
    installed(
        &card,
        "0.9.0",
        Some(&code_file(&good_record(), Some(&OTHER_SEED), 4096)),
        Some(new_key),
    );
    code_source(&card, "1.0.0", &AUTHOR_SEED);
    assert_eq!(
        install(&card, REPLACE, Evidence::NONE, Answer::Yes)
            .done
            .reason,
        6
    );
    assert_eq!(
        native::known_author(&card, "dials"),
        Some(new_key),
        "the pin is unchanged"
    );
    // 7: the pinned key is revoked.
    let card = MemoryStorage::new();
    installed(
        &card,
        "0.9.0",
        Some(&code_file(&good_record(), Some(&AUTHOR_SEED), 4096)),
        Some(author()),
    );
    code_source(&card, "1.0.0", &AUTHOR_SEED);
    let line = format!(
        "dials {} {}\n",
        paperpaw_sdk::digest::hex::encode(&author()),
        paperpaw_sdk::digest::hex::encode(&sign_message(
            paperpaw_native::signature::revocation_message("dials", &author()).as_bytes(),
            &AUTHOR_SEED
        ))
    );
    card.write(".paperpaw/revoked-keys.txt", line.as_bytes())
        .unwrap();
    assert_eq!(
        install(&card, REPLACE, Evidence::NONE, Answer::Yes)
            .done
            .reason,
        7
    );
    assert!(clean(&card));
}

#[test]
fn what_the_stage_check_refuses_ends_the_install() {
    for (what, source, why) in [
        (
            "unsigned",
            code_file(&good_record(), None, 4096),
            reason::UNSIGNED,
        ),
        (
            "a service the device lacks",
            code_file(
                &record(&["core.plugin", "core.ui"], &[], 1),
                Some(&AUTHOR_SEED),
                4096,
            ),
            reason::NEEDS_NEWER,
        ),
        (
            "damaged",
            {
                let mut code = code_file(&good_record(), Some(&AUTHOR_SEED), 4096);
                code[3000] ^= 1;
                code
            },
            reason::DAMAGED,
        ),
    ] {
        let card = MemoryStorage::new();
        card.write(&format!("{SOURCE}/plugin.ini"), ini("1.0.0").as_bytes())
            .unwrap();
        card.write(&format!("{SOURCE}/code.ppn"), &source).unwrap();
        let before = snapshot(&card);
        assert_eq!(
            install(&card, 0, Evidence::NONE, Answer::Yes).done.reason,
            why,
            "{what}"
        );
        assert_eq!(snapshot(&card), before, "{what}: the stage is removed");
    }
}

#[test]
fn the_grant_skips_only_the_prompt() {
    // An update of a pinned plugin whose features did not grow: no prompt.
    let card = MemoryStorage::new();
    let old = code_file(&good_record(), Some(&AUTHOR_SEED), 4096);
    installed(&card, "1.0.0", Some(&old), Some(author()));
    let digest = code_source(&card, "1.1.0", &AUTHOR_SEED);
    let ran = drive(
        &card,
        job(REPLACE, catalog(author(), digest)),
        true,
        Answer::No,
    );
    assert!(ran.asked.is_empty());
    assert_eq!(ran.done.reason, reason::OK);
    // Without the grant it asks, and a No is honoured.
    let card = MemoryStorage::new();
    installed(&card, "1.0.0", Some(&old), Some(author()));
    code_source(&card, "1.1.0", &AUTHOR_SEED);
    let ran = drive(&card, job(REPLACE, Evidence::NONE), false, Answer::No);
    assert_eq!((ran.asked.len(), ran.done.reason), (1, reason::DECLINED));
    // A version that declares a feature the installed one did not: asked.
    let card = MemoryStorage::new();
    installed(&card, "1.0.0", Some(&old), Some(author()));
    let wider = record(
        &["core.plugin"],
        &[("uses.registry", false), ("uses.wifi", false)],
        1,
    );
    code_source_with(&card, "1.1.0", &wider, &AUTHOR_SEED);
    let ran = drive(&card, job(REPLACE, Evidence::NONE), true, Answer::Yes);
    assert_eq!(ran.asked.len(), 1, "a new feature is shown to the user");
    assert_eq!(ran.asked[0].uses.count, 2);
    // A sideload (a new id without evidence) is never skipped.
    let card = MemoryStorage::new();
    code_source(&card, "1.0.0", &AUTHOR_SEED);
    let ran = drive(&card, job(0, Evidence::NONE), true, Answer::Yes);
    assert_eq!(ran.asked.len(), 1);
    assert!(ran.asked[0].sideload);
    // A data plugin's first install is asked about too (nothing to compare).
    let card = MemoryStorage::new();
    data_source(&card, "1.0.0");
    assert_eq!(
        drive(&card, job(0, Evidence::NONE), true, Answer::Yes)
            .asked
            .len(),
        1
    );
}

#[test]
fn a_source_changed_after_the_copy_has_no_effect() {
    let card = MemoryStorage::new();
    let digest = code_source(&card, "1.0.0", &AUTHOR_SEED);
    let mut job = job(0, catalog(author(), digest));
    let env = JobEnv {
        storage: &card,
        host: &host(),
        built_ins: &Names,
        granted: false,
    };
    while job.phase != Phase::Check {
        assert!(matches!(job.step(&env), JobStep::More));
    }
    // The source is replaced by other code, after its copy.
    let other = code_file(&good_record(), Some(&OTHER_SEED), 8192);
    card.write(&format!("{SOURCE}/code.ppn"), &other).unwrap();
    card.write(&format!("{SOURCE}/plugin.ini"), ini("9.9.9").as_bytes())
        .unwrap();
    let ran = drive_with(&card, &host(), &mut job, false, Answer::Yes);
    assert_eq!(ran.done.reason, reason::OK);
    assert_eq!(ran.asked[0].version.as_str(), "1.0.0");
    assert_eq!(native::known_author(&card, "dials"), Some(author()));
    assert_eq!(native::installed_code(&card, "dials"), Some(digest));
}

#[test]
fn the_copy_reads_and_lists_in_bounded_pieces() {
    let inner = MemoryStorage::new();
    for n in 0..40 {
        inner
            .write(&format!("{SOURCE}/f{n}.txt"), &vec![b'x'; 9000])
            .unwrap();
    }
    inner
        .write(&format!("{SOURCE}/plugin.ini"), ini("1.0.0").as_bytes())
        .unwrap();
    let card = Counting::new(inner);
    let ran = install(&card, 0, Evidence::NONE, Answer::Yes);
    assert_eq!(ran.done.reason, reason::OK);
    assert!(card.most_read.get() <= 4096, "{}", card.most_read.get());
    assert!(card.most_listed.get() <= 16, "{}", card.most_listed.get());
    assert_eq!(card.inner.size("plugins/dials/f39.txt").unwrap(), 9000);
    // More files than a plugin has.
    let inner = MemoryStorage::new();
    data_source(&inner, "1.0.0");
    for n in 0..70 {
        inner.write(&format!("{SOURCE}/g{n}.txt"), b"x").unwrap();
    }
    assert_eq!(
        install(&inner, 0, Evidence::NONE, Answer::Yes).done.reason,
        reason::DAMAGED
    );
    assert!(!inner.exists("plugins/.dials.part/plugin.ini"));
}

#[test]
fn a_cut_after_each_card_action_leaves_the_old_plugin_or_the_new_one() {
    let build = || {
        let card = MemoryStorage::new();
        let old = code_file(&good_record(), Some(&OTHER_SEED), 4096);
        installed(&card, "1.0.0", Some(&old), Some(public_key(&OTHER_SEED)));
        assert!(native::remember_code(
            &card,
            "dials",
            &old[64..96].try_into().unwrap()
        ));
        code_source(&card, "1.1.0", &AUTHOR_SEED);
        card
    };
    let rotation = Rotation {
        to: author(),
        signature: sign_message(
            rotation_message("dials", &public_key(&OTHER_SEED), &author()).as_bytes(),
            &RECOVERY_SEED,
        ),
    };
    let evidence = Evidence::new(None, &[rotation]).unwrap();
    let total = {
        let card = build();
        let cut = Cut::after(&card, usize::MAX);
        assert_eq!(
            drive(&cut, job(REPLACE, evidence), false, Answer::Yes)
                .done
                .reason,
            0
        );
        cut.done.get()
    };
    assert!(total > 20, "{total} card actions");
    let (mut old_seen, mut new_seen) = (0, 0);
    for allowed in 0..=total {
        let card = build();
        // The cut ends the job where it stands (its steps fail from there).
        let cut = Cut::after(&card, allowed);
        let _ = drive(&cut, job(REPLACE, evidence), false, Answer::Yes);
        // The next start.
        let _ = replay(&card);
        let ini = text(&card, "plugins/dials/plugin.ini").unwrap();
        let pinned = native::known_author(&card, "dials");
        let code = native::installed_code(&card, "dials");
        match ini.as_str() {
            s if s == ini_of("1.0.0") => {
                old_seen += 1;
                assert_eq!(pinned, Some(public_key(&OTHER_SEED)), "{allowed}");
                assert_ne!(code, None, "{allowed}");
                assert_eq!(
                    text(&card, "plugins/dials/old.txt").as_deref(),
                    Some("old"),
                    "{allowed}"
                );
            }
            s if s == ini_of("1.1.0") => {
                new_seen += 1;
                assert_eq!(pinned, Some(author()), "{allowed}");
                assert!(code.is_some(), "{allowed}");
                assert!(!card.exists("plugins/dials/old.txt"), "{allowed}");
            }
            other => panic!("a mix after {allowed}: {other:?}"),
        }
        assert!(
            !card.exists("plugins/.journal") && !card.exists("plugins/.old-dials/plugin.ini"),
            "{allowed}"
        );
        // A stage that is left is cleaned by the install that follows.
        let again = install(
            &card,
            REPLACE | FLAG_OLDER,
            evidence_for(&card),
            Answer::Yes,
        );
        assert!(
            matches!(again.done.reason, 0 | 6),
            "{allowed}: {}",
            again.done.reason
        );
    }
    assert!(
        old_seen >= 5 && new_seen >= 5,
        "old {old_seen}, new {new_seen}"
    );
}

fn ini_of(version: &str) -> String {
    ini(version)
}

/// The evidence that lets the 1.1.0 source install over whichever is there.
fn evidence_for(card: &MemoryStorage) -> Evidence {
    match native::known_author(card, "dials") {
        Some(key) if key == author() => Evidence::NONE,
        _ => Evidence::new(
            None,
            &[Rotation {
                to: author(),
                signature: sign_message(
                    rotation_message("dials", &public_key(&OTHER_SEED), &author()).as_bytes(),
                    &RECOVERY_SEED,
                ),
            }],
        )
        .unwrap(),
    }
}

#[test]
fn an_aborted_job_removes_its_stage_before_the_journal_and_leaves_the_swap_after() {
    let card = MemoryStorage::new();
    data_source(&card, "1.0.0");
    let mut job = job(0, Evidence::NONE);
    let env = JobEnv {
        storage: &card,
        host: &host(),
        built_ins: &Names,
        granted: false,
    };
    while job.phase != Phase::Check {
        job.step(&env);
    }
    assert!(card.exists("plugins/.dials.part/plugin.ini"));
    job.abort(reason::STOPPED);
    assert!(
        job.buf.capacity() == 0 && job.pending.capacity() == 0,
        "holds nothing"
    );
    assert_eq!(
        job.step(&env),
        JobStep::Done(Finished::refused(reason::STOPPED))
    );
    assert!(!card.exists("plugins/.dials.part/plugin.ini"));
    // Aborted once the journal is written: the job must finish, steps on to
    // its end and tells the real outcome (the swap is done, not "stopped").
    let mut job = self::job(0, Evidence::NONE);
    while job.phase != Phase::Swapping {
        if let JobStep::Ask(_) = job.step(&env) {
            job.answer(Answer::Yes);
        }
    }
    assert!(job.must_finish());
    job.abort(reason::STOPPED);
    let done = loop {
        if let JobStep::Done(done) = job.step(&env) {
            break done;
        }
    };
    assert_eq!(done.reason, reason::OK);
    assert!(!card.exists("plugins/.journal"));
    assert_eq!(
        text(&card, "plugins/dials/strings.ini").as_deref(),
        Some("hello\n")
    );
    assert!(clean(&card));
}

#[test]
fn the_asking_plugin_going_away_stops_nothing_after_the_yes_and_everything_before() {
    let card = MemoryStorage::new();
    data_source(&card, "1.0.0");
    let mut job = job(0, Evidence::NONE);
    let env = JobEnv {
        storage: &card,
        host: &host(),
        built_ins: &Names,
        granted: false,
    };
    loop {
        if let JobStep::Ask(_) = job.step(&env) {
            break;
        }
    }
    // Asked, no answer yet: it goes with its sender.
    assert!(!job.keep_without_sender() && !job.must_finish());
    job.answer(Answer::Yes);
    assert!(job.keep_without_sender());
    while job.phase != Phase::Swapping {
        job.step(&env);
    }
    assert!(job.keep_without_sender() && job.must_finish());
}

#[test]
fn a_card_that_fails_during_the_copy_ends_with_a_card_error_and_removes_the_stage() {
    let inner = MemoryStorage::new();
    data_source(&inner, "1.0.0");
    let card = Counting::new(inner);
    *card.fail.borrow_mut() = Some("strings.ini");
    assert_eq!(
        install(&card, 0, Evidence::NONE, Answer::Yes).done.reason,
        reason::CARD
    );
    assert!(!card.inner.exists("plugins/.dials.part/plugin.ini"));
    assert!(!card.inner.exists("plugins/dials/plugin.ini"));
}

#[test]
fn an_allocation_that_fails_ends_the_install_without_a_panic_or_a_half_install() {
    let build = || {
        let card = MemoryStorage::new();
        data_source(&card, "1.0.0");
        card
    };
    let mut refused = 0;
    for nth in 1..400_u32 {
        let card = build();
        let shield = Shielded(&card);
        FAIL_NTH.with(|n| n.set(nth));
        let ran = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
            let mut job = PluginInstall::new(7, 3, SOURCE, 0, Evidence::NONE)?;
            let env = JobEnv {
                storage: &shield,
                host: &host(),
                built_ins: &Names,
                granted: false,
            };
            for _ in 0..10_000 {
                // The origin marks and the pins are the SDK's Beta 3 writers,
                // whose allocations are not fallible (an open point for the SDK):
                // the steps that run them are not failed.
                let sdk = matches!(job.phase, Phase::Marks | Phase::Swapping);
                let armed = FAIL_NTH.with(|n| if sdk { n.replace(0) } else { n.get() });
                let step = job.step(&env);
                if sdk {
                    FAIL_NTH.with(|n| n.set(armed));
                }
                match step {
                    JobStep::More | JobStep::Wait => {}
                    JobStep::Unload(_) => panic!("an install closes no plugin"),
                    JobStep::Forget(_) => panic!("an install forgets nothing"),
                    JobStep::Ask(_) => job.answer(Answer::Yes),
                    JobStep::Done(done) => return Some(done),
                }
            }
            panic!("an install that never ends");
        }));
        FAIL_NTH.with(|n| n.set(0));
        let Some(done) = ran.expect("an allocation that fails never panics") else {
            continue;
        };
        if done.reason == reason::OTHER {
            refused += 1;
            // Nothing half done: either the journal is there and the next start
            // finishes the swap, or no plugin and no stage; or all of it.
            if card.exists("plugins/.journal") {
                assert!(matches!(
                    replay(&card),
                    Replayed::Finished {
                        rolled_back: false,
                        ..
                    }
                ));
            }
            let installed = card.exists("plugins/dials/plugin.ini");
            assert_eq!(
                installed,
                card.exists("plugins/dials/strings.ini"),
                "allocation {nth}"
            );
            if !installed {
                assert!(
                    !card.exists("plugins/.dials.part/plugin.ini"),
                    "allocation {nth}"
                );
            }
        } else {
            assert_eq!(done.reason, reason::OK, "allocation {nth}");
        }
    }
    assert!(refused >= 8, "{refused} allocations were refused");
    let _ = (FLAG_DATA_PLUGIN, INI, plugin_swap::JOURNAL);
}

#[test]
fn a_folder_that_appears_while_the_user_decides_is_not_replaced() {
    let card = MemoryStorage::new();
    data_source(&card, "1.0.0");
    let mut job = job(0, Evidence::NONE);
    let env = JobEnv {
        storage: &card,
        host: &host(),
        built_ins: &Names,
        granted: false,
    };
    let done = loop {
        match job.step(&env) {
            JobStep::Ask(_) => {
                // Someone put a plugin of the id there meanwhile.
                installed(&card, "0.9.0", None, None);
                job.answer(Answer::Yes);
            }
            JobStep::Done(done) => break done,
            _ => {}
        }
    };
    assert_eq!(done.reason, reason::CHANGED);
    assert_eq!(text(&card, "plugins/dials/old.txt").as_deref(), Some("old"));
    assert!(!card.exists("plugins/.dials.part/plugin.ini") && !card.exists("plugins/.journal"));
}

#[test]
fn the_plugin_always_has_its_folder_between_two_steps() {
    let card = MemoryStorage::new();
    installed(&card, "1.0.0", None, None);
    data_source(&card, "1.1.0");
    let mut job = job(REPLACE, Evidence::NONE);
    let env = JobEnv {
        storage: &card,
        host: &host(),
        built_ins: &Names,
        granted: false,
    };
    loop {
        assert!(card.exists("plugins/dials/plugin.ini"), "between two steps");
        match job.step(&env) {
            JobStep::Ask(_) => job.answer(Answer::Yes),
            JobStep::Done(done) => {
                assert_eq!(done.reason, reason::OK);
                break;
            }
            _ => {}
        }
    }
}

#[test]
fn an_allocation_that_fails_never_lets_the_grant_skip_the_question_for_a_new_feature() {
    let wider = record(
        &["core.plugin"],
        &[("uses.registry", false), ("uses.wifi", false)],
        1,
    );
    let old = code_file(&good_record(), Some(&AUTHOR_SEED), 4096);
    let mut asked_and_installed = 0;
    for nth in 1..600_u32 {
        let card = MemoryStorage::new();
        installed(&card, "1.0.0", Some(&old), Some(author()));
        code_source_with(&card, "1.1.0", &wider, &AUTHOR_SEED);
        let shield = Shielded(&card);
        FAIL_NTH.with(|n| n.set(nth));
        let ran = std::panic::catch_unwind(core::panic::AssertUnwindSafe(|| {
            let mut job = PluginInstall::new(7, 3, SOURCE, REPLACE, Evidence::NONE)?;
            let env = JobEnv {
                storage: &shield,
                host: &host(),
                built_ins: &Names,
                granted: true,
            };
            let mut asked = 0;
            for _ in 0..10_000 {
                let sdk = matches!(job.phase, Phase::Marks | Phase::Swapping);
                let armed = FAIL_NTH.with(|n| if sdk { n.replace(0) } else { n.get() });
                let step = job.step(&env);
                if sdk {
                    FAIL_NTH.with(|n| n.set(armed));
                }
                match step {
                    JobStep::Ask(_) => {
                        asked += 1;
                        job.answer(Answer::Yes);
                    }
                    JobStep::Done(done) => return Some((done, asked)),
                    _ => {}
                }
            }
            panic!("an install that never ends");
        }));
        FAIL_NTH.with(|n| n.set(0));
        let Some((done, asked)) = ran.expect("never a panic") else {
            continue;
        };
        if done.reason == reason::OK {
            assert_eq!(asked, 1, "allocation {nth}: installed without asking");
            asked_and_installed += 1;
        }
    }
    assert!(asked_and_installed > 0);
}

#[test]
fn the_job_is_small_enough_to_pass_by_value_on_the_x4() {
    // Frames hold 2 KiB there; the big values live in boxes.
    assert!(
        core::mem::size_of::<PluginInstall>() <= 1024,
        "{} bytes",
        core::mem::size_of::<PluginInstall>()
    );
}
