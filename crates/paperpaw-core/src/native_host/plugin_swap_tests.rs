//! The plugin swap and its replay: a cut after each card action of an update
//! and of a first install leaves the old plugin with its pins or the new one
//! with its pins, never a mix; a failure before the stage is in place rolls
//! back; a damaged journal is set aside.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::plugin_rules::{Decision, Evidence, Staged, decide};
use crate::native_host::plugin_trust::Standing;
use core::cell::Cell;
use paperpaw_native::signature::{public_key, rotation_message, sign_message};
use paperpaw_platform::DirEntry;
use paperpaw_sdk::MemoryStorage;
use std::{format, vec::Vec};

const A_SEED: [u8; 32] = [3; 32];
const B_SEED: [u8; 32] = [4; 32];
const R_SEED: [u8; 32] = [7; 32];
const OLD_CODE: [u8; 32] = [0xA1; 32];
const NEW_CODE: [u8; 32] = [0xB2; 32];

fn key(seed: &[u8; 32]) -> [u8; 32] {
    public_key(seed)
}

/// A card with the plugin `dials` installed (pinned to key A with its
/// recovery key, its code named) and a stage of the new version.
fn updated_card() -> MemoryStorage {
    let card = MemoryStorage::new();
    card.write("plugins/dials/plugin.ini", b"old").unwrap();
    card.write("plugins/dials/sub/file.txt", b"old file")
        .unwrap();
    assert!(keys::remember_first_install(
        &card,
        "dials",
        &key(&A_SEED),
        &key(&R_SEED)
    ));
    assert!(native::remember_code(&card, "dials", &OLD_CODE));
    card.write("plugins/.dials.part/plugin.ini", b"new")
        .unwrap();
    card.write("plugins/.dials.part/sub/file.txt", b"new file")
        .unwrap();
    card
}

/// The pins an update to key B takes: the rotation A to B, the new code.
fn update_pins() -> Pins {
    let rotation = Rotation {
        to: key(&B_SEED),
        signature: sign_message(
            rotation_message("dials", &key(&A_SEED), &key(&B_SEED)).as_bytes(),
            &R_SEED,
        ),
    };
    let standing = Standing {
        pinned: Some(key(&A_SEED)),
        recovery: Some(key(&R_SEED)),
        revoked: false,
    };
    let staged = Staged {
        id: "dials",
        code: Some((key(&B_SEED), NEW_CODE)),
        standing,
    };
    let evidence = Evidence::new(None, &[rotation]).unwrap();
    match decide(&staged, &evidence) {
        Decision::Install { pins, sideload } => {
            assert!(!sideload);
            pins
        }
        other => panic!("{other:?}"),
    }
}

fn update_entry() -> Entry {
    Entry {
        id: Text::new("dials"),
        had_old: true,
        pins: update_pins(),
    }
}

/// A card that lets `allowed` mutating actions through and then none: the
/// power was cut. `fail_only` instead fails that one action and lets the rest
/// through (a card error).
pub(in crate::native_host) struct Cut<'a> {
    card: &'a MemoryStorage,
    pub(in crate::native_host) done: Cell<usize>,
    allowed: usize,
    fail_only: Option<usize>,
    /// Bounded tree steps taken, and whole-tree removals (`remove_dir_all`).
    pub(in crate::native_host) tree_steps: Cell<usize>,
    pub(in crate::native_host) whole_trees: Cell<usize>,
}

impl<'a> Cut<'a> {
    pub(in crate::native_host) fn after(card: &'a MemoryStorage, allowed: usize) -> Self {
        Self {
            card,
            done: Cell::new(0),
            allowed,
            fail_only: None,
            tree_steps: Cell::new(0),
            whole_trees: Cell::new(0),
        }
    }
    /// Action `nth` fails and the power goes `after` actions in all.
    fn failing_then_cut(card: &'a MemoryStorage, nth: usize, after: usize) -> Self {
        Self {
            card,
            done: Cell::new(0),
            allowed: after,
            fail_only: Some(nth),
            tree_steps: Cell::new(0),
            whole_trees: Cell::new(0),
        }
    }
    pub(in crate::native_host) fn failing(card: &'a MemoryStorage, nth: usize) -> Self {
        Self {
            card,
            done: Cell::new(0),
            allowed: usize::MAX,
            fail_only: Some(nth),
            tree_steps: Cell::new(0),
            whole_trees: Cell::new(0),
        }
    }
    /// Whether the next mutating action happens.
    fn go(&self) -> Result<(), StorageError> {
        let n = self.done.get();
        self.done.set(n + 1);
        if n >= self.allowed || self.fail_only == Some(n) {
            return Err(StorageError::Io);
        }
        Ok(())
    }
}

impl Storage for Cut<'_> {
    fn read_at(&self, p: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(p, at, buf)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.card.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.go()?;
        self.card.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.go()?;
        self.card.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.go()?;
        self.card.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        self.go()?;
        self.card.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.go()?;
        self.card.create_dir(p)
    }
    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        self.go()?;
        self.tree_steps.set(self.tree_steps.get() + 1);
        self.card.remove_tree_step(root, keep_root, place)
    }

    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.whole_trees.set(self.whole_trees.get() + 1);
        self.go()?;
        self.card.remove_dir_all(p)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.go()?;
        self.card.sync()
    }
}

/// The live install: the journal, then the swap to its end. Returns how it
/// ended (a failed action stops it, as a cut does).
fn install(storage: &dyn Storage, entry: &Entry) -> Result<SwapStep, u8> {
    begin(storage, entry)?;
    let mut swap = Swap::new(*entry, true).unwrap();
    for _ in 0..16 {
        match swap.step(storage) {
            SwapStep::More => {}
            end => return Ok(end),
        }
    }
    panic!("a swap that never ends");
}

fn text(card: &MemoryStorage, path: &str) -> Option<String> {
    let size = card.size(path).ok()? as usize;
    let mut data = std::vec![0; size];
    card.read_at(path, 0, &mut data).ok()?;
    String::from_utf8(data).ok()
}

/// The plugin folder's two files, its pinned key and its code.
type State = (
    Option<String>,
    Option<String>,
    Option<[u8; 32]>,
    Option<[u8; 32]>,
);

/// What the card says the plugin is: its folder, its pinned key, its code.
fn state(card: &MemoryStorage) -> State {
    (
        text(card, "plugins/dials/plugin.ini"),
        text(card, "plugins/dials/sub/file.txt"),
        native::known_author(card, "dials"),
        native::installed_code(card, "dials"),
    )
}

fn old_state() -> State {
    (
        Some("old".into()),
        Some("old file".into()),
        Some(key(&A_SEED)),
        Some(OLD_CODE),
    )
}

fn new_state() -> State {
    (
        Some("new".into()),
        Some("new file".into()),
        Some(key(&B_SEED)),
        Some(NEW_CODE),
    )
}

/// How many mutating actions the whole install takes.
fn actions(card: &MemoryStorage, entry: &Entry) -> usize {
    let cut = Cut::after(card, usize::MAX);
    assert_eq!(
        install(&cut, entry),
        Ok(SwapStep::Done { rolled_back: false })
    );
    cut.done.get()
}

#[test]
fn a_swap_replaces_the_plugin_and_takes_its_pins() {
    let card = updated_card();
    assert_eq!(state(&card), old_state());
    assert_eq!(
        install(&card, &update_entry()),
        Ok(SwapStep::Done { rolled_back: false })
    );
    assert_eq!(state(&card), new_state());
    assert!(!card.exists(JOURNAL) && !card.exists("plugins/.old-dials"));
    assert!(!card.exists("plugins/.dials.part"));
    assert!(!card.exists(TMP));
}

#[test]
fn a_cut_after_each_card_action_of_an_update_leaves_the_old_or_the_new_plugin() {
    let entry = update_entry();
    let total = actions(&updated_card(), &entry);
    assert!(total >= 10, "{total} card actions");
    let (mut old, mut new) = (0, 0);
    for allowed in 0..=total {
        let card = updated_card();
        let _ = install(&Cut::after(&card, allowed), &entry);
        // The next start.
        let replayed = replay(&card);
        let journal_was_written = !matches!(replayed, Replayed::Nothing);
        match state(&card) {
            now if now == old_state() => {
                old += 1;
                // Before the journal: nothing moved, the old plugin is whole.
                assert!(
                    !journal_was_written
                        || matches!(
                            replayed,
                            Replayed::Finished {
                                rolled_back: true,
                                ..
                            }
                        ),
                    "{allowed}: {replayed:?}"
                );
            }
            now if now == new_state() => {
                new += 1;
                assert!(
                    matches!(
                        replayed,
                        Replayed::Finished {
                            rolled_back: false,
                            ..
                        }
                    ) || allowed == total,
                    "{allowed}: {replayed:?}"
                );
            }
            now => panic!("a mix after {allowed} actions: {now:?} ({replayed:?})"),
        }
        assert!(!card.exists(JOURNAL), "{allowed}");
        assert!(!card.exists("plugins/.old-dials"), "{allowed}");
        // The same replay twice changes nothing more.
        assert_eq!(replay(&card), Replayed::Nothing);
    }
    assert!(old >= 3 && new >= 3, "old {old}, new {new}");
}

#[test]
fn a_cut_after_each_card_action_of_a_first_install_leaves_nothing_or_the_plugin() {
    let build = || {
        let card = MemoryStorage::new();
        card.write("plugins/.dials.part/plugin.ini", b"new")
            .unwrap();
        card.write("plugins/.dials.part/sub/file.txt", b"new file")
            .unwrap();
        card
    };
    let pins = Pins::with(
        Some((key(&B_SEED), Some(key(&R_SEED)))),
        &[],
        Some(NEW_CODE),
    )
    .unwrap();
    let entry = Entry {
        id: Text::new("dials"),
        had_old: false,
        pins,
    };
    let total = actions(&build(), &entry);
    for allowed in 0..=total {
        let card = build();
        let _ = install(&Cut::after(&card, allowed), &entry);
        let _ = replay(&card);
        let (ini, file, author, code) = state(&card);
        if ini.is_some() {
            assert_eq!(
                (ini.as_deref(), file.as_deref(), author, code),
                (
                    Some("new"),
                    Some("new file"),
                    Some(key(&B_SEED)),
                    Some(NEW_CODE)
                ),
                "{allowed}"
            );
            // A cut between the author's key and its recovery key leaves the
            // plugin without a recovery key (only the successor way), as the
            // SDK's `remember_first_install` documents; never a recovery key
            // without its author.
            let recovery = keys::known_recovery(&card, "dials");
            assert!(
                recovery.is_none() || recovery == Some(key(&R_SEED)),
                "{allowed}"
            );
        } else {
            // Not installed: no pin, no code named.
            assert_eq!((author, code), (None, None), "{allowed}");
        }
        assert!(
            !card.exists(JOURNAL) && !card.exists("plugins/.old-dials"),
            "{allowed}"
        );
    }
}

#[test]
fn a_card_error_before_the_stage_is_in_place_rolls_the_swap_back() {
    let entry = update_entry();
    // The journal's 4 actions (dir, write, sync, rename, sync), then the old
    // folder's move and the stage's: fail each in turn.
    for nth in 5..7 {
        let card = updated_card();
        let cut = Cut::failing(&card, nth);
        let ended = install(&cut, &entry).unwrap();
        assert_eq!(ended, SwapStep::Failed(reason::CARD), "action {nth}");
        assert_eq!(state(&card), old_state(), "action {nth}");
        assert!(
            !card.exists(JOURNAL) && !card.exists("plugins/.dials.part"),
            "action {nth}"
        );
        assert!(!card.exists("plugins/.old-dials"), "action {nth}");
    }
}

#[test]
fn a_card_error_after_the_stage_is_in_place_keeps_the_journal_for_the_next_start() {
    let entry = update_entry();
    // Fail the first of the pins' writes.
    let card = updated_card();
    let total = actions(&updated_card(), &entry);
    let mut seen = false;
    for nth in 7..total {
        let card = updated_card();
        let ended = install(&Cut::failing(&card, nth), &entry).unwrap();
        if let SwapStep::Failed(why) = ended {
            seen = true;
            assert_eq!(why, reason::CARD);
            // The new folder is in place; the next start finishes it.
            assert_eq!(
                text(&card, "plugins/dials/plugin.ini").as_deref(),
                Some("new")
            );
            assert!(card.exists(JOURNAL), "action {nth}");
            assert!(matches!(
                replay(&card),
                Replayed::Finished {
                    rolled_back: false,
                    ..
                }
            ));
            assert_eq!(state(&card), new_state(), "action {nth}");
        }
    }
    assert!(seen);
    drop(card);
}

#[test]
fn a_folder_that_appeared_since_the_check_is_not_replaced_unasked() {
    let card = MemoryStorage::new();
    card.write("plugins/.dials.part/plugin.ini", b"new")
        .unwrap();
    card.write("plugins/dials/plugin.ini", b"someone's")
        .unwrap();
    let entry = Entry {
        id: Text::new("dials"),
        had_old: false,
        pins: Pins::NONE,
    };
    assert_eq!(
        install(&card, &entry),
        Ok(SwapStep::Failed(reason::CHANGED))
    );
    assert_eq!(
        text(&card, "plugins/dials/plugin.ini").as_deref(),
        Some("someone's")
    );
    assert!(!card.exists("plugins/.dials.part") && !card.exists(JOURNAL));
}

#[test]
fn a_journal_that_does_not_hold_is_set_aside_and_never_trusted() {
    let card = updated_card();
    let entry = update_entry();
    begin(&card, &entry).unwrap();
    let good = {
        let mut buf = std::vec![0; card.size(JOURNAL).unwrap() as usize];
        card.read_at(JOURNAL, 0, &mut buf).unwrap();
        buf
    };
    assert_eq!(read(&card), Ok(Found::Entry(entry)));
    // A flipped byte anywhere, a cut record, one with too much.
    let mut bad: Vec<Vec<u8>> = (0..good.len())
        .map(|at| {
            let mut b = good.clone();
            b[at] ^= 0x10;
            b
        })
        .collect();
    bad.push(good[..good.len() - 1].to_vec());
    bad.push([good.clone(), std::vec![0]].concat());
    bad.push(Vec::new());
    for record in bad {
        let card = updated_card();
        card.write(JOURNAL, &record).unwrap();
        assert_eq!(replay(&card), Replayed::SetAside);
        assert!(card.exists(BAD) && !card.exists(JOURNAL));
        // Nothing was touched: the old plugin, its stage.
        assert_eq!(state(&card), old_state());
        assert!(card.exists("plugins/.dials.part/plugin.ini"));
    }
}

#[test]
fn a_journal_naming_an_id_no_plugin_has_is_not_followed() {
    for id in ["../x", "core", "A", "", "a/b"] {
        let mut entry = update_entry();
        entry.id = Text::new(id);
        let mut buf = [0_u8; RECORD_MAX];
        let len = encode(&entry, &mut buf);
        assert_eq!(decode(&buf[..len]), None, "{id:?}");
    }
    assert_eq!(target_path("../x"), None);
    assert_eq!(stage_path("core"), None);
    assert_eq!(old_path("dials").as_deref(), Some("plugins/.old-dials"));
    assert_eq!(stage_path("dials").as_deref(), Some("plugins/.dials.part"));
}

#[test]
fn a_card_that_cannot_be_read_is_an_error_not_a_bad_journal() {
    struct Dead(MemoryStorage);
    impl Storage for Dead {
        fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
            Err(StorageError::Io)
        }
        fn size(&self, p: &str) -> Result<u64, StorageError> {
            self.0.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            self.0.append(p, d)
        }
        fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
            self.0.list(d, v)
        }
        fn remove(&self, p: &str) -> Result<(), StorageError> {
            self.0.remove(p)
        }
    }
    let dead = Dead(updated_card());
    begin(&dead.0, &update_entry()).unwrap();
    assert_eq!(replay(&dead), Replayed::Failed(reason::CARD));
    assert!(dead.0.exists(JOURNAL) && !dead.0.exists(BAD));
}

#[test]
fn the_pins_are_taken_idempotently() {
    let card = updated_card();
    let entry = update_entry();
    assert!(take_pins(&card, &entry));
    assert!(take_pins(&card, &entry));
    assert_eq!(native::known_author(&card, "dials"), Some(key(&B_SEED)));
    assert_eq!(native::installed_code(&card, "dials"), Some(NEW_CODE));
    // The recovery key stays.
    assert_eq!(keys::known_recovery(&card, "dials"), Some(key(&R_SEED)));
    let _ = format!("{entry:?}");
}

/// An update through the chain A to B to C (two rotations).
fn chain_pins() -> Pins {
    let step = |from: &[u8; 32], to: &[u8; 32]| Rotation {
        to: key(to),
        signature: sign_message(
            rotation_message("dials", &key(from), &key(to)).as_bytes(),
            &R_SEED,
        ),
    };
    let c_seed = [5_u8; 32];
    let evidence = Evidence::new(None, &[step(&A_SEED, &B_SEED), step(&B_SEED, &c_seed)]).unwrap();
    let staged = Staged {
        id: "dials",
        code: Some((key(&c_seed), NEW_CODE)),
        standing: Standing {
            pinned: Some(key(&A_SEED)),
            recovery: Some(key(&R_SEED)),
            revoked: false,
        },
    };
    match decide(&staged, &evidence) {
        Decision::Install { pins, .. } => {
            assert_eq!(pins.rotations().len(), 2);
            pins
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_replay_after_the_pins_were_taken_follows_only_the_steps_after_the_pinned_key() {
    let entry = Entry {
        id: Text::new("dials"),
        had_old: true,
        pins: chain_pins(),
    };
    let c = key(&[5; 32]);
    // Taken, then taken again (a cut after the pins, before the journal went).
    let card = updated_card();
    assert!(take_pins(&card, &entry));
    assert_eq!(native::known_author(&card, "dials"), Some(c));
    assert!(take_pins(&card, &entry));
    assert_eq!(native::known_author(&card, "dials"), Some(c));
    // Cut between the two rotations: the next start goes on from B.
    let card = updated_card();
    let first = Entry {
        pins: Pins::with(None, &entry.pins.rotations()[..1], None).unwrap(),
        ..entry
    };
    assert!(take_pins(&card, &first));
    assert_eq!(native::known_author(&card, "dials"), Some(key(&B_SEED)));
    assert!(take_pins(&card, &entry));
    assert_eq!(native::known_author(&card, "dials"), Some(c));
    assert_eq!(native::installed_code(&card, "dials"), Some(NEW_CODE));
}

#[test]
fn a_cut_during_the_roll_back_never_leaves_the_old_plugin_with_the_new_pins() {
    let entry = update_entry();
    let total = actions(&updated_card(), &entry);
    let mut rolled = 0;
    // The stage's move into place fails (action k); the power goes a few
    // actions later, wherever the roll back is.
    for failing in 0..total {
        for extra in 0..5 {
            let card = updated_card();
            let cut = Cut::failing_then_cut(&card, failing, failing + 1 + extra);
            let Ok(SwapStep::Failed(_)) = install(&cut, &entry) else {
                continue;
            };
            rolled += 1;
            let _ = replay(&card);
            let now = state(&card);
            assert!(
                now == old_state() || now == new_state(),
                "failing {failing}, cut after {extra}: {now:?}"
            );
        }
    }
    assert!(rolled > 0);
}

#[test]
fn another_installs_unfinished_journal_is_not_replaced() {
    let card = updated_card();
    let other = Entry {
        id: Text::new("other"),
        had_old: false,
        pins: Pins::NONE,
    };
    begin(&card, &other).unwrap();
    let before = card.size(JOURNAL).unwrap();
    assert_eq!(begin(&card, &update_entry()), Err(reason::OTHER));
    assert_eq!(card.size(JOURNAL).unwrap(), before);
    assert_eq!(read(&card), Ok(Found::Entry(other)));
}

#[test]
fn the_two_moves_of_the_swap_are_one_step() {
    // A stop between steps never finds the plugin without its folder.
    let card = updated_card();
    let entry = update_entry();
    begin(&card, &entry).unwrap();
    let mut swap = Swap::new(entry, true).unwrap();
    loop {
        assert!(card.exists("plugins/dials/plugin.ini"), "between two steps");
        match swap.step(&card) {
            SwapStep::More => {}
            end => {
                assert_eq!(end, SwapStep::Done { rolled_back: false });
                break;
            }
        }
    }
}
