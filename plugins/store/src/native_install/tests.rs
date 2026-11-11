#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::native_item::{Digest, File, More};
use crate::native_model::Entry;
use paperpaw_sdk::MemoryStorage;

struct Fake {
    storage: MemoryStorage,
    installs: Vec<(u32, String, bool, bool, Proof)>,
    revoked: Vec<usize>,
    refuse: Option<Error>,
}

impl Fake {
    fn new() -> Self {
        let storage = MemoryStorage::new().with("stage/airy-theme/plugin.ini", "id = airy-theme\n");
        Self {
            storage,
            installs: Vec::new(),
            revoked: Vec::new(),
            refuse: None,
        }
    }
}

impl Wire for Fake {
    fn storage(&self) -> &dyn Storage {
        &self.storage
    }
    fn install(
        &mut self,
        request: u32,
        folder: &str,
        update: bool,
        allow_older: bool,
        proof: &Proof,
    ) -> Result<(), Error> {
        if let Some(error) = self.refuse {
            return Err(error);
        }
        self.installs
            .push((request, folder.into(), update, allow_older, proof.clone()));
        Ok(())
    }
    fn revoke(&mut self, revoked: &[Revoked]) -> Result<(), Error> {
        self.revoked.push(revoked.len());
        Ok(())
    }
}

fn staged() -> Staged {
    Staged {
        plugin: "airy-theme".into(),
        name: "Airy".into(),
        folder: "stage/airy-theme".into(),
    }
}

fn page(code: bool) -> Page {
    Page {
        entry: Entry {
            id: "airy-theme".into(),
            name: "Airy".into(),
            ..Entry::default()
        },
        files: alloc::vec![File {
            path: "plugin.ini".into(),
            url: "https://x/plugin.ini".into(),
            size: 1,
            digest: Digest::Sha1("0".repeat(40)),
        }],
        code,
        author_key: Some([7; 32]),
        recovery_key: Some([8; 32]),
        rotations: (0..8)
            .map(|n| Rotation {
                to: [n; 32],
                signature: [n; 64],
            })
            .collect(),
        more: More::default(),
    }
}

#[test]
fn a_code_plugin_carries_the_librarys_word_a_data_plugin_none() {
    let proof = Proof::of(&page(true)).unwrap();
    assert_eq!(proof.author, Some([7; 32]));
    assert_eq!(proof.recovery, Some([8; 32]));
    // The last six steps of eight.
    assert_eq!(proof.rotations.len(), MAX_STEPS);
    assert_eq!(proof.rotations[0].to, [2; 32]);
    assert_eq!(proof.rotations[5].to, [7; 32]);
    assert_eq!(Proof::of(&page(false)).unwrap(), Proof::default());
}

#[test]
fn the_host_is_asked_to_install_with_the_evidence_and_the_end_is_said() {
    let mut wire = Fake::new();
    let (mut download, mut installer) = (Download::new(), Installer::new());
    let have = Have {
        version: Some("1.0.0"),
        repairs: false,
    };
    let step = installer
        .finish(&mut wire, &mut download, staged(), &page(true), have)
        .unwrap();
    assert_eq!(step, Step::Installing);
    assert!(installer.busy());
    let (request, folder, update, older, proof) = wire.installs[0].clone();
    assert_eq!(
        (folder.as_str(), update, older),
        ("stage/airy-theme", true, false)
    );
    assert_eq!(proof.author, Some([7; 32]));
    // Reason 0, the next start for code: installed, kept as the store's, the
    // files gone.
    let end = End::new(request, 0, 2);
    let outcome = installer
        .done(&mut wire, &mut download, &end, "1.1.0-beta.1", false)
        .unwrap();
    assert_eq!(
        outcome,
        Outcome::Installed {
            plugin: "airy-theme".into(),
            name: "Airy".into(),
            updated: true,
            restart: true
        }
    );
    assert_eq!(download.step(), Step::Installed);
    assert!(!installer.busy());
    assert!(!wire.storage.exists("stage/airy-theme/plugin.ini"));
    let installs = Installs::read(&wire.storage).unwrap();
    assert_eq!(
        installs.release("airy-theme", "1.1.0"),
        Some("1.1.0-beta.1")
    );
    assert!(installs.of("airy-theme", "1.1.0-beta.2"));
    assert!(!installs.of("airy-theme", "1.0.0"));
    // The end of a request that is none going on is not ours.
    assert_eq!(
        installer
            .done(&mut wire, &mut download, &end, "1.1.0", false)
            .unwrap(),
        Outcome::Other
    );
}

#[test]
fn a_repair_takes_the_same_version_a_new_install_replaces_nothing() {
    let mut wire = Fake::new();
    let (mut download, mut installer) = (Download::new(), Installer::new());
    let have = Have {
        version: Some("1.0.0"),
        repairs: true,
    };
    installer
        .finish(&mut wire, &mut download, staged(), &page(false), have)
        .unwrap();
    assert_eq!((wire.installs[0].2, wire.installs[0].3), (true, true));
    installer.forget(&mut wire);
    let mut wire = Fake::new();
    installer
        .finish(
            &mut wire,
            &mut download,
            staged(),
            &page(false),
            Have::default(),
        )
        .unwrap();
    assert_eq!((wire.installs[0].2, wire.installs[0].3), (false, false));
    // The request numbers count up and are never 0.
    assert_ne!(wire.installs[0].0, 0);
}

#[test]
fn what_the_host_refuses_is_said_in_the_stores_words() {
    for (reason, want) in [
        (4, Some("store.refused.damaged")),
        (5, Some("store.refused.not-the-author")),
        (6, Some("store.refused.key-changed")),
        (7, Some("store.refused.revoked")),
        (13, Some("store.refused.no-room")),
        (14, Some("store.refused.same-version")),
        (15, Some("store.refused.taken")),
        (18, Some("store.refused.changed")),
        (99, Some("store.refused.other")),
        (2, None),
        (3, None),
        (20, None),
    ] {
        let mut wire = Fake::new();
        let (mut download, mut installer) = (Download::new(), Installer::new());
        installer
            .finish(
                &mut wire,
                &mut download,
                staged(),
                &page(true),
                Have::default(),
            )
            .unwrap();
        let end = End::new(wire.installs[0].0, reason, 0);
        let outcome = installer
            .done(&mut wire, &mut download, &end, "1.0.0", false)
            .unwrap();
        match want {
            Some(key) => assert_eq!(
                outcome,
                Outcome::Refused {
                    name: "Airy".into(),
                    key
                },
                "reason {reason}"
            ),
            None => assert_eq!(outcome, Outcome::Silent, "reason {reason}"),
        }
        // Not installed: no record, the staged files gone, the step idle.
        assert!(Installs::read(&wire.storage).unwrap().kept.is_empty());
        assert!(!wire.storage.exists("stage/airy-theme/plugin.ini"));
        assert_eq!(download.step(), Step::Idle);
    }
}

#[test]
fn a_refused_install_command_or_a_wrong_plugin_leaves_no_files() {
    let mut wire = Fake::new();
    wire.refuse = Some(Error::Denied);
    let (mut download, mut installer) = (Download::new(), Installer::new());
    let step = installer
        .finish(
            &mut wire,
            &mut download,
            staged(),
            &page(true),
            Have::default(),
        )
        .unwrap();
    assert_eq!(step, Step::Failed(Failure::Card));
    assert!(!installer.busy());
    assert!(!wire.storage.exists("stage/airy-theme/plugin.ini"));
    let mut wire = Fake::new();
    let mut other = page(true);
    other.entry.id = "other".into();
    let step = installer
        .finish(&mut wire, &mut download, staged(), &other, Have::default())
        .unwrap();
    assert_eq!(step, Step::Failed(Failure::Damaged));
    assert!(wire.installs.is_empty());
}

#[test]
fn the_installs_are_kept_four_a_plugin_and_read_back() {
    let storage = MemoryStorage::new();
    for n in 0..6 {
        let release = alloc::format!("1.{n}.0");
        assert!(Installs::remember(&storage, "airy-theme", &release).unwrap());
        assert!(Installs::remember(&storage, "other", "2.0.0").unwrap());
    }
    let installs = Installs::read(&storage).unwrap();
    let airy = installs
        .kept
        .iter()
        .filter(|(id, _)| id == "airy-theme")
        .count();
    assert_eq!(airy, VERSIONS_KEPT);
    assert!(!installs.of("airy-theme", "1.1.0"));
    assert!(installs.of("airy-theme", "1.5.0"));
    assert!(installs.of("other", "2.0.0"));
    assert!(Installs::forget(&storage, "airy-theme").unwrap());
    let installs = Installs::read(&storage).unwrap();
    assert!(!installs.of("airy-theme", "1.5.0") && installs.of("other", "2.0.0"));
    // What is no plugin or no version is not kept; damaged lines are skipped.
    assert!(!Installs::remember(&storage, "Not Ok", "1.0.0").unwrap());
    assert!(!Installs::remember(&storage, "airy-theme", "1\t0").unwrap());
    let storage =
        MemoryStorage::new().with(INSTALLS_FILE, "airy-theme\t1.0.0\nnonsense\n\t3\nx y\t1\n");
    assert!(Installs::read(&storage).unwrap().of("airy-theme", "1.0.0"));
    assert_eq!(Installs::read(&storage).unwrap().kept.len(), 1);
}

#[test]
fn the_plugins_installed_before_the_record_are_adopted_once() {
    let storage = MemoryStorage::new();
    assert!(!Installs::exists(&storage));
    assert!(Installs::adopt(&storage, [("airy-theme", "1.0.0"), ("Bad Id", "1")]).unwrap());
    assert!(Installs::read(&storage).unwrap().of("airy-theme", "1.0.0"));
    // Once kept, it does nothing.
    assert!(Installs::adopt(&storage, [("other", "2.0.0")]).unwrap());
    assert!(!Installs::read(&storage).unwrap().of("other", "2.0.0"));
}

#[test]
fn revocations_go_over_in_one_go_at_most_sixteen() {
    let mut wire = Fake::new();
    hand_over(&mut wire, &[]).unwrap();
    assert!(wire.revoked.is_empty());
    let many: Vec<Revoked> = (0..20)
        .map(|_| Revoked {
            plugin: "airy-theme".into(),
            key: [1; 32],
            signature: [2; 64],
        })
        .collect();
    hand_over(&mut wire, &many).unwrap();
    assert_eq!(wire.revoked, [MAX_REVOKED]);
}

#[test]
fn the_queue_goes_on_with_the_next_plugin_and_is_kept() {
    use crate::native_queue::Mark;
    let storage = MemoryStorage::new();
    let mut queue = Queue::default();
    queue.add(["alpha", "Bad id", "beta"], Mark::Quiet).unwrap();
    assert_eq!(
        next(&mut queue, &storage).unwrap(),
        Next::FetchPage("alpha".into())
    );
    // The kept queue is the one a restart finds: alpha's turn is on.
    assert_eq!(Queue::read(&storage).unwrap().lines().len(), 2);
    assert_eq!(
        next(&mut queue, &storage).unwrap(),
        Next::FetchPage("beta".into())
    );
    assert_eq!(next(&mut queue, &storage).unwrap(), Next::Idle);
    assert!(Queue::read(&storage).unwrap().lines().is_empty());
}
