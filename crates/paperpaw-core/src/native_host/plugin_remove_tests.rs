//! The removal of a plugin: its refusals, its question, what goes and what
//! stays (an exact id, the data only when chosen), and the cut after every card
//! action, finished by the start's replay.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::firmware_install::BuiltIns;
use crate::native_host::plugin_job::PluginHost;
use crate::native_host::plugin_swap::{self, BAD, Replayed, replay, tests::Cut};
use crate::native_host::plugin_unforgotten;
use paperpaw_native::{
    header5::{Host as Loader, X4_PROFILE},
    metadata::Host as Offered,
};
use paperpaw_sdk::MemoryStorage;

struct Names;
impl BuiltIns for Names {
    fn count(&self) -> usize {
        1
    }
    fn index_of(&self, id: &str) -> Option<usize> {
        (id == "home").then_some(0)
    }
}

fn host() -> PluginHost {
    PluginHost {
        loader: Loader::x4(1),
        offered: Offered {
            abi_major: 7,
            minor: 1,
            profile: X4_PROFILE,
            services: &[],
            features: &[],
        },
        signers: &[],
    }
}

/// A card with `dials` installed (code and data), its neighbours `dials-plus`
/// and `dia` (whose names `dials` is, or has, a prefix of) with data of their
/// own.
fn card() -> MemoryStorage {
    let card = MemoryStorage::new();
    card.write(
        "plugins/dials/plugin.ini",
        b"[plugin]\nid = dials\nname = Dials\nversion = 1.0.0\n",
    )
    .unwrap();
    card.write("plugins/dials/sub/file.txt", b"file").unwrap();
    card.write(
        "plugins/dials-plus/plugin.ini",
        b"[plugin]\nid = dials-plus\n",
    )
    .unwrap();
    card.write(".paperpaw/plugins/dials/notes.txt", b"notes")
        .unwrap();
    card.write(".paperpaw/plugins/dials/deep/more.txt", b"more")
        .unwrap();
    card.write(".paperpaw/plugins/dials-plus/keep.txt", b"keep")
        .unwrap();
    card.write(".paperpaw/plugins/dia/keep.txt", b"keep")
        .unwrap();
    card
}

fn job() -> PluginRemove {
    PluginRemove::new(1, 7, "dials").unwrap()
}

/// What a job ended with, and what it asked.
struct Run {
    done: Finished,
    asked: Option<PluginQuestion>,
    unloads: u32,
    /// The plugins the host was asked to forget, and whether the journal was
    /// still there and the data already moved when it was.
    forgets: std::vec::Vec<(std::string::String, bool)>,
}

/// Steps `job` on `storage`: the question gets `answer`, an unload is answered
/// at once.
fn drive(job: &mut PluginRemove, storage: &dyn Storage, answer: Answer) -> Run {
    drive_with(job, storage, &Names, answer)
}

/// As [`drive`], with the kernel's view of `built_ins`.
fn drive_with(
    job: &mut PluginRemove,
    storage: &dyn Storage,
    built_ins: &dyn BuiltIns,
    answer: Answer,
) -> Run {
    let host = host();
    let mut run = Run {
        done: Finished::refused(0),
        asked: None,
        unloads: 0,
        forgets: std::vec::Vec::new(),
    };
    for _ in 0..200 {
        let env = JobEnv {
            storage,
            host: &host,
            built_ins,
            granted: true,
        };
        match job.step(&env) {
            JobStep::More => {}
            JobStep::Wait => panic!("waits for what nobody gives"),
            JobStep::Ask(question) => {
                run.asked = Some(question);
                job.answer(answer);
            }
            JobStep::Unload(id) => {
                assert_eq!(id.as_str(), "dials");
                run.unloads += 1;
                job.unloaded();
            }
            JobStep::Forget(id) => {
                let ready = card_is_ready_to_forget(storage);
                run.forgets.push((id.as_str().into(), ready));
                job.forgotten(true);
            }
            JobStep::Done(done) => {
                run.done = done;
                return run;
            }
        }
    }
    panic!("never ended");
}

/// The data was moved aside and the journal is there: what the forget step
/// stands between.
fn card_is_ready_to_forget(card: &dyn Storage) -> bool {
    is_there(card, plugin_swap::JOURNAL)
        && !is_there(card, ".paperpaw/plugins/dials")
        && is_there(card, ".paperpaw/plugins/.removing-dials")
}

fn is_there(card: &dyn Storage, path: &str) -> bool {
    plugin_swap::present(card, path).unwrap()
}

/// Nothing of the neighbours was touched.
fn neighbours_intact(card: &MemoryStorage) {
    assert!(card.exists("plugins/dials-plus/plugin.ini"));
    assert!(card.exists(".paperpaw/plugins/dials-plus/keep.txt"));
    assert!(card.exists(".paperpaw/plugins/dia/keep.txt"));
}

fn nothing_left_over(card: &dyn Storage) {
    for path in [
        "plugins/.removing-dials",
        ".paperpaw/plugins/.removing-dials",
        "plugins/.journal",
        "plugins/.journal.tmp",
        plugin_swap::TRIES,
        plugin_unforgotten::FILE,
    ] {
        assert!(
            !is_there(card, path) && card.size(path).is_err(),
            "{path} is left"
        );
    }
}

#[test]
fn core_a_built_in_and_a_missing_plugin_are_refused_before_any_question() {
    let card = card();
    // A leftover folder of a built-in's id stays where it is.
    card.write("plugins/home/plugin.ini", b"x").unwrap();
    for (id, why) in [("core", 15), ("home", 15), ("ghost", 1)] {
        let mut remove = PluginRemove::new(1, 7, id).unwrap();
        let run = drive(&mut remove, &card, Answer::Yes);
        assert_eq!(run.done.reason, why, "{id}");
        assert!(run.asked.is_none(), "{id} is not asked about");
    }
    assert!(card.exists("plugins/home/plugin.ini"));
}

#[test]
fn an_id_that_is_no_plugin_name_is_never_a_path() {
    let card = card();
    for id in [
        "",
        ".",
        "..",
        "a/b",
        "../dials",
        ".removing-dials",
        "Dials Plus",
    ] {
        let mut remove = PluginRemove::new(1, 7, id).unwrap();
        let run = drive(&mut remove, &card, Answer::Yes);
        assert_eq!(run.done.reason, 15, "{id:?}");
        assert!(run.asked.is_none());
    }
    assert!(card.exists("plugins/dials/plugin.ini"));
    assert!(card.exists(".paperpaw/plugins/dials/notes.txt"));
    assert!(!is_there(&card, "plugins/.removing-"));
}

#[test]
fn the_question_names_the_plugin_and_a_damaged_one_by_its_id() {
    let card = card();
    let run = drive(&mut job(), &card, Answer::No);
    let question = run.asked.unwrap();
    assert_eq!((question.owner, question.request), (1, 7));
    assert_eq!(question.kind, KIND_REMOVAL);
    assert_eq!(question.id.as_str(), "dials");
    assert_eq!(question.name.as_str(), "Dials");
    assert_eq!(question.version.as_str(), "1.0.0");
    // The grant never skips it: `granted` is true in `drive`.
    card.write("plugins/dials/plugin.ini", &[0xff, 0xfe])
        .unwrap();
    let run = drive(&mut job(), &card, Answer::No);
    assert_eq!(run.asked.unwrap().name.as_str(), "dials");
}

#[test]
fn no_and_dismissed_change_nothing() {
    for (answer, why) in [(Answer::No, 2), (Answer::Dismissed, 3)] {
        let card = card();
        let run = drive(&mut job(), &card, answer);
        assert_eq!(run.done.reason, why);
        assert_eq!(run.unloads, 0, "its instances stay");
        assert!(card.exists("plugins/dials/plugin.ini"));
        assert!(card.exists(".paperpaw/plugins/dials/notes.txt"));
        neighbours_intact(&card);
        nothing_left_over(&card);
    }
}

#[test]
fn a_yes_removes_the_plugin_and_keeps_its_data() {
    let card = card();
    let run = drive(&mut job(), &card, Answer::Yes);
    assert_eq!(run.done.reason, 0);
    assert_eq!(run.unloads, 1, "its instances were closed first");
    assert_eq!(run.done.changed.map(|id| id.as_str().len()), Some(5));
    // Data kept (bit 0); a data plugin has no code to leave at the next start.
    assert_eq!(run.done.flags, DONE_DATA_KEPT);
    assert!(!is_there(&card, "plugins/dials"));
    assert!(card.exists(".paperpaw/plugins/dials/notes.txt"));
    assert!(card.exists(".paperpaw/plugins/dials/deep/more.txt"));
    neighbours_intact(&card);
    nothing_left_over(&card);
}

#[test]
fn the_second_yes_removes_the_data_too_and_only_of_that_exact_id() {
    let card = card();
    let run = drive(&mut job(), &card, Answer::YesAndData);
    assert_eq!(run.done.reason, 0);
    assert_eq!(run.done.flags & DONE_DATA_KEPT, 0);
    assert!(!is_there(&card, "plugins/dials"));
    assert!(!is_there(&card, ".paperpaw/plugins/dials"));
    neighbours_intact(&card);
    nothing_left_over(&card);
}

#[test]
fn code_that_is_loaded_says_its_flash_room_returns_at_the_next_start() {
    let card = card();
    card.write("plugins/dials/code.ppn", b"code").unwrap();
    let run = drive(&mut job(), &card, Answer::Yes);
    assert_eq!(run.done.flags, DONE_DATA_KEPT | DONE_AT_NEXT_START);
}

#[test]
fn the_job_waits_for_the_instances_and_a_stop_after_the_yes_still_ends_it() {
    let card = card();
    let host = host();
    let env = JobEnv {
        storage: &card,
        host: &host,
        built_ins: &Names,
        granted: false,
    };
    let mut remove = job();
    while !matches!(remove.step(&env), JobStep::Ask(_)) {}
    assert!(!remove.must_finish() && !remove.keep_without_sender());
    remove.answer(Answer::Yes);
    assert!(remove.keep_without_sender(), "the user's yes stays");
    while !matches!(remove.step(&env), JobStep::Unload(_)) {}
    assert!(remove.must_finish());
    for _ in 0..5 {
        assert_eq!(remove.step(&env), JobStep::Wait, "waits for the host");
    }
    // The host stops (sleep): not waited for, the removal still ends, with
    // its own reason.
    remove.abort(20);
    let mut ended = None;
    for _ in 0..50 {
        if let JobStep::Done(done) = remove.step(&env) {
            ended = Some(done);
            break;
        }
    }
    assert_eq!(ended.unwrap().reason, 0);
    assert!(!is_there(&card, "plugins/dials"));
}

#[test]
fn a_stop_before_the_yes_ends_it_with_the_reason() {
    let card = card();
    let host = host();
    let env = JobEnv {
        storage: &card,
        host: &host,
        built_ins: &Names,
        granted: false,
    };
    let mut remove = job();
    while !matches!(remove.step(&env), JobStep::Ask(_)) {}
    remove.abort(20);
    assert_eq!(remove.step(&env), JobStep::Done(Finished::refused(20)));
    assert!(card.exists("plugins/dials/plugin.ini"));
    nothing_left_over(&card);
}

/// What the next start sees of a removal of `dials` cut after `allowed` card
/// actions: either the plugin whole (the journal never came) or gone for good,
/// its data with it only if chosen; never a half.
fn after_a_cut(allowed: usize, with_data: bool) -> bool {
    let card = card();
    let cut = Cut::after(&card, allowed);
    let answer = if with_data {
        Answer::YesAndData
    } else {
        Answer::Yes
    };
    let host = host();
    let mut remove = job();
    let mut forgets = 0;
    for _ in 0..200 {
        let env = JobEnv {
            storage: &cut,
            host: &host,
            built_ins: &Names,
            granted: false,
        };
        match remove.step(&env) {
            JobStep::More | JobStep::Wait => {}
            JobStep::Ask(_) => remove.answer(answer),
            JobStep::Unload(_) => remove.unloaded(),
            JobStep::Forget(_) => {
                forgets += 1;
                remove.forgotten(true);
            }
            JobStep::Done(_) => break,
        }
    }
    // The next start: it finishes the journal and says nothing; a removal
    // stopped at the forget step waits for the host, the first pass has it.
    let mut said = std::vec::Vec::new();
    let waiting =
        crate::native_host::plugin_start::replay_at_start(&card, &mut |line| said.push(line));
    assert!(said.is_empty(), "{allowed}: {said:?}");
    if waiting {
        assert!(
            card.exists(plugin_swap::JOURNAL),
            "{allowed}: the journal stays"
        );
        let mut asked = std::vec::Vec::new();
        let done = plugin_swap::resume(&card, &mut |id| {
            asked.push(std::string::String::from(id));
            true
        });
        assert!(
            matches!(done, Replayed::Removed { .. }),
            "{allowed}: {done:?}"
        );
        forgets += asked.len();
        assert_eq!(asked, ["dials"], "{allowed}");
        assert!(!card.exists(plugin_swap::JOURNAL), "{allowed}");
    }
    // What the journal left to clean goes in bounded pieces, by the sweep.
    crate::native_host::plugin_start::sweep(&card);
    nothing_left_over(&card);
    neighbours_intact(&card);
    let there = is_there(&card, "plugins/dials");
    if there {
        assert!(card.exists("plugins/dials/sub/file.txt"), "{allowed}");
        assert!(
            card.exists(".paperpaw/plugins/dials/notes.txt"),
            "{allowed}"
        );
    } else {
        assert_eq!(
            is_there(&card, ".paperpaw/plugins/dials"),
            !with_data,
            "{allowed}: data"
        );
        if !with_data {
            assert!(card.exists(".paperpaw/plugins/dials/deep/more.txt"));
        }
    }
    // Once the data is gone for good the host was asked to forget the plugin,
    // whatever cut came (the journal stays until it was), and never without it.
    let data_gone = !is_there(&card, ".paperpaw/plugins/dials");
    assert_eq!(
        data_gone && with_data,
        forgets >= 1,
        "{allowed}: forgets {forgets}"
    );
    there
}

#[test]
fn a_cut_after_every_card_action_is_the_old_plugin_or_the_removed_one() {
    for with_data in [false, true] {
        let (mut kept, mut removed) = (0, 0);
        // Beyond the last action of the job the cut never comes.
        for allowed in 0..40 {
            if after_a_cut(allowed, with_data) {
                kept += 1;
            } else {
                removed += 1;
            }
        }
        assert!(kept > 0 && removed > 0, "both ends are reached");
    }
}

#[test]
fn a_card_error_before_the_first_move_leaves_the_plugin_and_no_journal() {
    // Fail each action in turn, the rest let through.
    let mut kept_whole = 0;
    for nth in 0..12 {
        let card = card();
        let failing = Cut::failing(&card, nth);
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            drive(&mut job(), &failing, Answer::Yes)
        }));
        let run = run.unwrap();
        // Either it went through or it ended with a card reason; the start
        // finishes whatever the journal still holds.
        let mut said = std::vec::Vec::new();
        crate::native_host::plugin_start::replay_at_start(&card, &mut |line| said.push(line));
        assert!(said.is_empty(), "{nth}: {said:?}");
        crate::native_host::plugin_start::sweep(&card);
        nothing_left_over(&card);
        neighbours_intact(&card);
        if run.done.reason != 0 && is_there(&card, "plugins/dials") {
            kept_whole += 1;
            assert!(card.exists(".paperpaw/plugins/dials/notes.txt"));
        }
    }
    assert!(kept_whole > 0);
}

#[test]
fn a_journal_whose_state_no_step_makes_is_set_aside_not_left_to_block() {
    let card = card();
    // The plugin folder and its moved copy at once.
    card.write("plugins/.removing-dials/plugin.ini", b"x")
        .unwrap();
    let removal = Removal {
        id: Text::new("dials"),
        data: false,
    };
    plugin_swap::begin_removal(&card, &removal).unwrap();
    assert_eq!(replay(&card), Replayed::SetAside);
    assert!(
        card.size(JOURNAL).is_err(),
        "no journal blocks the next one"
    );
    assert!(card.size(BAD).is_ok());
    // Both folders are as they were: nothing was guessed at.
    assert!(card.exists("plugins/dials/plugin.ini"));
    assert!(card.exists("plugins/.removing-dials/plugin.ini"));
    // The next change of the plugin folder is not blocked.
    assert_eq!(plugin_swap::begin_removal(&card, &removal), Ok(()));
}

#[test]
fn a_swap_journal_with_the_stage_the_old_folder_and_the_target_is_set_aside_too() {
    use crate::native_host::plugin_rules::Pins;
    let card = card();
    card.write("plugins/.dials.part/plugin.ini", b"new")
        .unwrap();
    card.write("plugins/.old-dials/plugin.ini", b"old").unwrap();
    let entry = plugin_swap::Entry {
        id: Text::new("dials"),
        had_old: true,
        pins: Pins::with(None, &[], None).unwrap(),
    };
    plugin_swap::begin(&card, &entry).unwrap();
    assert_eq!(replay(&card), Replayed::SetAside);
    assert!(card.size(JOURNAL).is_err());
    assert!(card.exists("plugins/.old-dials/plugin.ini"), "no copy lost");
    assert!(card.exists("plugins/.dials.part/plugin.ini"));
    assert!(card.exists("plugins/dials/plugin.ini"));
}

#[test]
fn a_removal_record_reads_back_and_a_damaged_one_does_not() {
    let card = MemoryStorage::new();
    for data in [false, true] {
        let removal = Removal {
            id: Text::new("dials-plus"),
            data,
        };
        plugin_swap::begin_removal(&card, &removal).unwrap();
        assert_eq!(
            plugin_swap::read(&card),
            Ok(plugin_swap::Found::Removal(removal))
        );
        card.remove(JOURNAL).unwrap();
    }
    // Every flipped byte of a record is a bad record, set aside, never a
    // removal of something else.
    let removal = Removal {
        id: Text::new("dials"),
        data: true,
    };
    plugin_swap::begin_removal(&card, &removal).unwrap();
    let mut bytes = std::vec![0_u8; card.size(JOURNAL).unwrap() as usize];
    card.read_at(JOURNAL, 0, &mut bytes).unwrap();
    for at in 0..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[at] ^= 0x01;
        card.write(JOURNAL, &flipped).unwrap();
        assert_eq!(
            plugin_swap::read(&card),
            Ok(plugin_swap::Found::SetAside),
            "byte {at}"
        );
        assert!(card.size(BAD).is_ok());
        card.remove(BAD).unwrap();
    }
}

#[test]
fn removed_with_its_data_the_host_forgets_the_plugin_after_the_moves_and_before_the_journal_goes() {
    let card = card();
    let run = drive(&mut job(), &card, Answer::YesAndData);
    assert_eq!(run.done.reason, 0);
    assert_eq!(run.forgets, [("dials".into(), true)]);
    nothing_left_over(&card);
}

#[test]
fn removed_keeping_its_data_the_host_forgets_nothing() {
    let card = card();
    let run = drive(&mut job(), &card, Answer::Yes);
    assert_eq!(run.done.reason, 0);
    assert!(run.forgets.is_empty());
}

#[test]
fn a_forget_that_fails_ends_the_job_and_the_first_pass_after_start_finishes_it() {
    let card = card();
    let host = host();
    let mut remove = job();
    let mut asked = 0;
    let done = loop {
        let env = JobEnv {
            storage: &card,
            host: &host,
            built_ins: &Names,
            granted: false,
        };
        match remove.step(&env) {
            JobStep::More | JobStep::Wait => {}
            JobStep::Ask(_) => remove.answer(Answer::YesAndData),
            JobStep::Unload(_) => remove.unloaded(),
            JobStep::Forget(_) => {
                asked += 1;
                remove.forgotten(false);
            }
            JobStep::Done(done) => break done,
        }
    };
    assert_eq!(asked, 1);
    assert_eq!(done.reason, reason::CARD);
    assert!(card.exists(plugin_swap::JOURNAL), "the journal stays");
    // The next start stops at the forget step: nothing is removed beyond the
    // moves, and the journal waits for the host.
    let mut said = std::vec::Vec::new();
    assert!(crate::native_host::plugin_start::replay_at_start(
        &card,
        &mut |line| said.push(line)
    ));
    assert!(said.is_empty(), "{said:?}");
    assert!(card.exists(plugin_swap::JOURNAL));
    assert!(is_there(&card, ".paperpaw/plugins/.removing-dials"));
    // The first pass has the host: a host that cannot forget keeps it waiting,
    // one that can finishes the removal.
    let done = plugin_swap::resume(&card, &mut |_| false);
    assert_eq!(done, Replayed::Failed(reason::CARD));
    assert!(card.exists(plugin_swap::JOURNAL));
    let done = plugin_swap::resume(&card, &mut |id| id == "dials");
    assert!(
        matches!(done, Replayed::Removed { data: true, .. }),
        "{done:?}"
    );
    crate::native_host::plugin_start::sweep(&card);
    assert!(!card.exists(plugin_swap::JOURNAL));
    assert!(!is_there(&card, "plugins/dials"));
    nothing_left_over(&card);
    neighbours_intact(&card);
}

/// A removal with its data whose forget failed in the live job: the data is
/// moved aside and the journal waits.
fn stopped_at_a_failed_forget(card: &MemoryStorage) {
    let host = host();
    let mut remove = job();
    loop {
        let env = JobEnv {
            storage: card,
            host: &host,
            built_ins: &Names,
            granted: false,
        };
        match remove.step(&env) {
            JobStep::More | JobStep::Wait => {}
            JobStep::Ask(_) => remove.answer(Answer::YesAndData),
            JobStep::Unload(_) => remove.unloaded(),
            JobStep::Forget(_) => remove.forgotten(false),
            JobStep::Done(_) => break,
        }
    }
    assert!(card_is_ready_to_forget(card));
}

#[test]
fn a_host_that_cannot_forget_does_not_hold_the_journal_for_ever() {
    let card = card();
    stopped_at_a_failed_forget(&card);
    // Two starts in a row at which the host cannot: the journal stays.
    for tried in 1..plugin_swap::FORGET_TRIES {
        let done = plugin_swap::resume(&card, &mut |_| false);
        assert_eq!(done, Replayed::Failed(reason::CARD), "{tried}");
        assert!(card.exists(plugin_swap::JOURNAL));
    }
    // The next one ends the waiting: the plugin is out, its secrets stay
    // (that is told), the journal and its count are gone.
    let done = plugin_swap::resume(&card, &mut |_| false);
    assert!(matches!(done, Replayed::Unforgotten { .. }), "{done:?}");
    assert!(!card.exists(plugin_swap::JOURNAL));
    assert!(!card.exists(plugin_swap::TRIES));
    // The secrets that stay are on the list: tried again at every start, the
    // id is not free for an install until they are gone.
    assert!(plugin_unforgotten::has(&card, "dials"));
    assert!(crate::native_host::plugin_start::replay_at_start(
        &card,
        &mut |_| ()
    ));
    assert_eq!(plugin_unforgotten::retry(&card, &mut |_| false), 1);
    assert!(plugin_unforgotten::has(&card, "dials"));
    assert_eq!(plugin_unforgotten::retry(&card, &mut |id| id == "dials"), 0);
    assert!(!plugin_unforgotten::has(&card, "dials"));
    assert!(!card.exists(plugin_unforgotten::FILE));
    crate::native_host::plugin_start::sweep(&card);
    nothing_left_over(&card);
    neighbours_intact(&card);
}

#[test]
fn a_host_that_forgets_at_a_later_start_resets_the_count() {
    let card = card();
    stopped_at_a_failed_forget(&card);
    assert_eq!(
        plugin_swap::resume(&card, &mut |_| false),
        Replayed::Failed(reason::CARD)
    );
    assert!(card.exists(plugin_swap::TRIES));
    let done = plugin_swap::resume(&card, &mut |id| id == "dials");
    assert!(
        matches!(done, Replayed::Removed { data: true, .. }),
        "{done:?}"
    );
    assert!(!card.exists(plugin_swap::TRIES));
}

#[test]
fn a_large_data_folder_is_cleaned_by_the_bounded_walk_and_the_sweeps_finish_what_the_job_left() {
    let card = card();
    for n in 0..300 {
        card.write(&std::format!(".paperpaw/plugins/dials/bulk/f{n}.txt"), b"x")
            .unwrap();
    }
    // A card that counts: bounded tree steps, and whole-tree removals. How many
    // entries one step removes is the card's own contract (`remove_tree_step`:
    // at most 64 removals and 256 reads); the job only goes by it.
    let counting = Cut::after(&card, usize::MAX);
    let run = drive(&mut job(), &counting, Answer::YesAndData);
    assert_eq!(run.done.reason, 0);
    assert_eq!(counting.whole_trees.get(), 0, "no remove_dir_all");
    assert!(
        counting.tree_steps.get() >= 2,
        "{}",
        counting.tree_steps.get()
    );
    // What the job left (nothing, or a rest) is finished by the starts' sweeps,
    // a bounded number of pieces each, in a bounded number of starts.
    let mut starts = 0;
    while is_there(&card, ".paperpaw/plugins/.removing-dials")
        || is_there(&card, "plugins/.removing-dials")
    {
        starts += 1;
        assert!(
            starts <= 50,
            "never finished: {:?} {:?}",
            card.remove_tree_step(".paperpaw/plugins/.removing-dials", false, 0),
            counting.tree_steps.get()
        );
        crate::native_host::plugin_start::sweep(&card);
    }
    assert!(!is_there(&card, ".paperpaw/plugins/dials"));
    nothing_left_over(&card);
    neighbours_intact(&card);
}

#[test]
fn a_leftover_removal_folder_refuses_before_any_journal_is_written() {
    for left in [
        "plugins/.removing-dials",
        ".paperpaw/plugins/.removing-dials",
    ] {
        let card = card();
        card.write(&std::format!("{left}/x.txt"), b"x").unwrap();
        let run = drive(&mut job(), &card, Answer::YesAndData);
        assert_eq!(run.done.reason, reason::OTHER, "{left}");
        assert_eq!(run.unloads, 0, "{left}: nothing was closed");
        assert!(!card.exists(plugin_swap::JOURNAL), "{left}: no journal");
        assert!(is_there(&card, "plugins/dials"), "{left}");
        // The sweep finishes the leftover; the removal can then be asked again.
        crate::native_host::plugin_start::sweep(&card);
        let again = drive(&mut job(), &card, Answer::Yes);
        assert_eq!(again.done.reason, 0, "{left}");
    }
}

#[test]
fn a_device_without_a_record_takes_its_plugins_for_the_stores_before_a_removal_commits() {
    use super::super::store_record::tests::{Found, record};
    let card = card();
    // Plugins installed before PaperPaw kept a record: the store's, taken
    // over before dials goes (as interface 6's store did).
    let found = Found::data(&[("dials", "1.0.0"), ("dials-plus", "0.2")]);
    let run = drive_with(&mut job(), &card, &found, Answer::Yes);
    assert_eq!(run.done.reason, reason::OK);
    assert!(!is_there(&card, "plugins/dials"));
    assert_eq!(
        record(&card).as_deref(),
        Some("dials\t1.0.0\ndials-plus\t0.2.0\n")
    );
}
