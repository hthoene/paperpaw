//! What the store asks and how it reads the answers (slice 7, host-free): the
//! dialog of plugins another plugin asks for, the first-run offer, with the interface-6 words and decisions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_i18n::{Arg as Word, Catalog, Translator};
use paperpaw_plugin_store::native_ask::{
    self as ask, Badge, Decision, Have, Plan, Reply, Say, Way,
};
use paperpaw_plugin_store::native_index::{Index, take};
use paperpaw_plugin_store::native_item::Device;
use paperpaw_plugin_store::native_list::Hooks;
use paperpaw_plugin_store::native_queue::{Mark, Queue, Report, note, report};
use paperpaw_plugin_store::native_request::{
    DECLINED_MS, Finished, Last, REQUEST, Request, outcome,
};
use paperpaw_plugin_store::native_screens::Next;
use paperpaw_sdk::native::commands::PluginOutcome;
use paperpaw_sdk::native::events::AnswerOutcome;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::{Error, Fingerprint, INTERFACE};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

const EN: &str = include_str!("../i18n/en.ini");
const DE: &str = include_str!("../i18n/de.ini");

/// The store's own catalog in one language, as the host says it.
struct Words(Translator);

fn words(language: &str) -> Words {
    let mut translator = Translator::new();
    translator.add(Catalog::parse(if language == "de" { DE } else { EN }).unwrap());
    translator.set_language(language);
    Words(translator)
}

impl Say for Words {
    fn lookup(&mut self, key: &str) -> Result<Option<String>, Error> {
        // The host's own decimal mark, which the store's catalog does not hold.
        if key == "core.decimal" {
            return Ok((self.0.language() == "de").then(|| ",".to_string()));
        }
        Ok(self.0.get(key).map(ToString::to_string))
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        let count = args.iter().find_map(|arg| match arg {
            Arg::Count(count) => Some(*count),
            _ => None,
        });
        let named: Vec<(&str, Word<'_>)> = args
            .iter()
            .filter_map(|arg| match arg {
                Arg::Text { name, value } => Some((*name, Word::Str(value))),
                _ => None,
            })
            .collect();
        Ok(match count {
            Some(count) => self.0.plural(key, count, &named),
            None => self.0.format(key, &named),
        })
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        Ok(self.0.localize_digits(text).into_owned())
    }
}

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

/// An index of the plugins `(id, name, size)` on the card.
fn indexed(storage: &MemoryStorage, plugins: &[(&str, &str, u64)]) {
    fn nobody(_: &str) -> Option<[u8; 32]> {
        None
    }
    fn none() -> Vec<String> {
        Vec::new()
    }
    let items: Vec<String> = plugins
        .iter()
        .map(|(id, name, size)| {
            format!(
                r#"{{"id": "{id}", "name": "{name}", "summary": "s", "group": "look",
 "kind": "data", "author": "A", "licence": "MIT",
 "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": {size}}}}}"#
            )
        })
        .collect();
    let list = format!(r#"{{"format": 2, "plugins": [{}]}}"#, items.join(","));
    storage.write("list.json", list.as_bytes()).unwrap();
    let device = Device {
        name: "x4",
        fingerprint: Some(Fingerprint::of(INTERFACE, 1)),
        code_file: "code.ppn",
        have: None,
        sdk: paperpaw_sdk::SDK_VERSION,
        official: true,
        provides: None,
    };
    let hooks = Hooks {
        author_of: &nobody,
        stopped: &none,
        host_checks: false,
    };
    take(storage, "list.json", device, hooks).unwrap().unwrap();
}

/// A card that refuses to write.
struct Refusing<'a>(&'a MemoryStorage);

impl Storage for Refusing<'_> {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.0.size(p)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::NotFound)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.0.remove(p)
    }
    fn rename(&self, a: &str, b: &str) -> Result<(), StorageError> {
        self.0.rename(a, b)
    }
}

// --- sizes -----------------------------------------------------------------

#[test]
fn sizes_read_as_interface_6_wrote_them() {
    let mut bytes = vec![0, 1, 9, 10, 819, 820, 999, 1000, 1023, 1024, 1025, 14_336];
    bytes.extend([9_999, 10_240, 999_999, 1_000_000, 2_900_000, 2_970_000]);
    bytes.extend([1_048_575, 1_048_576, 1 << 30, 5 << 30, u64::MAX]);
    for n in bytes {
        for mark in [".", ",", "٫"] {
            assert_eq!(
                ask::size_text(n, mark).unwrap(),
                paperpaw_sdk::files::size(n, mark),
                "{n} {mark}"
            );
        }
    }
    assert_eq!(
        ask::shown_size(&mut words("en"), 2_900_000).unwrap(),
        "2.8 MB"
    );
}

// --- the requests of other plugins ----------------------------------------

#[test]
fn an_ask_is_taken_as_the_kernel_takes_it_and_more() {
    let on_device = |id: &str| id == "there";
    let got = ask::request_ids(
        [
            "pack-a", "there", "Bad Name", "pack-a", "../x", "", "core", "pack-b", "-no",
        ],
        &on_device,
    )
    .unwrap();
    assert_eq!(got, ["pack-a", "pack-b"]);
    let many: Vec<String> = (0..50).map(|n| format!("p{n}")).collect();
    let got = ask::request_ids(many.iter().map(String::as_str), &|_| false).unwrap();
    assert_eq!(got.len(), 32);
}

#[test]
fn an_ask_is_decided_as_interface_6_decided_it() {
    let w = strings(&["pack-a"]);
    let last = |declined| Last {
        by: "reader".into(),
        ids: w.clone(),
        at_ms: 1_000,
        declined,
    };
    let declined = Plan::Answer(PluginOutcome::Declined);
    assert_eq!(
        ask::plan("reader", &[], None, 0, false, false),
        Plan::Nothing
    );
    assert_eq!(
        ask::plan("reader", &w, None, 5, false, false),
        Plan::Ask { close_stale: false }
    );
    // Declined a moment ago: not asked again; after the minute it is.
    let before = 1_000 + DECLINED_MS - 1;
    assert_eq!(
        ask::plan("reader", &w, Some(&last(true)), before, false, false),
        declined
    );
    assert_eq!(
        ask::plan("reader", &w, Some(&last(true)), before + 1, false, false),
        Plan::Ask { close_stale: false }
    );
    // The dialog still open or the download going on is the one request.
    assert_eq!(
        ask::plan("reader", &w, Some(&last(false)), before, true, true),
        Plan::SameOpen
    );
    // Another's request is under way; a stale one is closed.
    assert_eq!(ask::plan("other", &w, None, 0, true, true), declined);
    assert_eq!(
        ask::plan("other", &w, None, 0, true, false),
        Plan::Ask { close_stale: true }
    );
    // A name that is no plugin's is not trusted with a request file.
    assert_eq!(ask::plan("reader\nx", &w, None, 0, false, false), declined);
    assert_eq!(ask::plan("Reader", &w, None, 0, false, false), declined);
}

#[test]
fn the_dialog_names_the_plugins_with_their_sizes_in_both_languages() {
    let storage = MemoryStorage::new();
    indexed(
        &storage,
        &[
            ("script-greek", "Greek letters", 2_900_000),
            ("airy", "Airy", 300),
        ],
    );
    let index = Index::kept(&storage).unwrap();
    let wanted = strings(&["script-greek", "unlisted", "airy"]);
    let Reply::Dialog(dialog) = ask::offer(
        &storage,
        &mut words("en"),
        index.as_ref(),
        "reader",
        &wanted,
        true,
    )
    .unwrap() else {
        panic!("a dialog")
    };
    // The library's own listing is what the dialog names; unlisted is left out.
    assert_eq!(dialog.ids, ["script-greek", "airy"]);
    // The one the library lacks is answered at once; nothing is left unanswered.
    assert_eq!(dialog.not_found, ["unlisted"]);
    assert!(dialog.declined.is_empty());
    assert_eq!(
        dialog.title,
        "This book needs more letters to show its text. Download them?"
    );
    assert_eq!(dialog.text, "Greek letters (2.8 MB)\nAiry (300 B)");
    assert_eq!(dialog.labels, "Download\nNot now");
    dialog.question().unwrap();
    // Kept on the card for the answer.
    let open = Request::open(&storage).unwrap().unwrap();
    assert_eq!((open.by.as_str(), open.ids.len()), ("reader", 2));
    // German.
    let Reply::Dialog(de) = ask::offer(
        &storage,
        &mut words("de"),
        index.as_ref(),
        "store",
        &wanted,
        true,
    )
    .unwrap() else {
        panic!("a dialog")
    };
    assert!(
        de.title.starts_with("Das eben installierte Plugin"),
        "{}",
        de.title
    );
    assert_eq!(de.labels, "Herunterladen\nNicht jetzt");
    assert_eq!(de.text, "Greek letters (2,8 MB)\nAiry (300 B)");
}

#[test]
fn what_asks_is_named_by_a_fixed_key_never_by_what_the_ask_says() {
    assert_eq!(ask::by_key("reader"), "store.request.by.reader");
    assert_eq!(ask::by_key("store"), "store.request.by.store");
    for hostile in [
        "other",
        "calibre",
        "",
        "reader\n",
        "x.y",
        "../reader",
        "Reader",
    ] {
        assert_eq!(
            ask::by_key(hostile),
            "store.request.by.other",
            "{hostile:?}"
        );
    }
    // Every key the dialog can use exists in both languages.
    for catalog in [EN, DE] {
        for key in ["reader", "store", "other"] {
            assert!(catalog.contains(&format!("store.request.by.{key} = ")));
        }
    }
}

#[test]
fn without_the_listing_the_ids_stand_for_themselves_and_without_the_library_nothing() {
    let storage = MemoryStorage::new();
    let wanted = strings(&["pack-a", "pack-b"]);
    // No index: every id is offered by its id.
    let Reply::Dialog(dialog) =
        ask::offer(&storage, &mut words("en"), None, "reader", &wanted, true).unwrap()
    else {
        panic!("a dialog")
    };
    assert_eq!(dialog.text, "pack-a\npack-b");
    Request::close(&storage);
    // An index that has none of them: not offered, not found.
    indexed(&storage, &[("other", "Other", 1)]);
    let index = Index::kept(&storage).unwrap();
    let reply = ask::offer(
        &storage,
        &mut words("en"),
        index.as_ref(),
        "reader",
        &wanted,
        true,
    )
    .unwrap();
    assert_eq!(
        reply,
        Reply::Answer {
            ids: wanted.clone(),
            outcome: PluginOutcome::NotFound
        }
    );
    assert!(storage.size(REQUEST).is_err());
}

#[test]
fn no_network_is_failed_and_a_card_that_refuses_is_declined() {
    let storage = MemoryStorage::new();
    indexed(&storage, &[("pack-a", "Pack A", 10)]);
    let index = Index::kept(&storage).unwrap();
    let wanted = strings(&["pack-a"]);
    let reply = ask::offer(
        &storage,
        &mut words("en"),
        index.as_ref(),
        "reader",
        &wanted,
        false,
    )
    .unwrap();
    assert_eq!(
        reply,
        Reply::Answer {
            ids: wanted.clone(),
            outcome: PluginOutcome::Failed
        }
    );
    assert!(storage.size(REQUEST).is_err(), "no dialog, no request");
    let card = Refusing(&storage);
    let reply = ask::offer(
        &card,
        &mut words("en"),
        index.as_ref(),
        "reader",
        &wanted,
        true,
    )
    .unwrap();
    assert_eq!(
        reply,
        Reply::Answer {
            ids: wanted,
            outcome: PluginOutcome::Declined
        }
    );
}

#[test]
fn a_long_list_names_what_it_asks_for_and_declines_the_rest() {
    let storage = MemoryStorage::new();
    let long = "N".repeat(78);
    let plugins: Vec<(String, String, u64)> = (0..16)
        .map(|n| (format!("pack-{n}"), format!("{long}{n:02}"), 1_234_567))
        .collect();
    let refs: Vec<(&str, &str, u64)> = plugins
        .iter()
        .map(|(id, name, size)| (id.as_str(), name.as_str(), *size))
        .collect();
    indexed(&storage, &refs);
    let index = Index::kept(&storage).unwrap();
    let wanted: Vec<String> = plugins.iter().map(|(id, ..)| id.clone()).collect();
    let Reply::Dialog(dialog) = ask::offer(
        &storage,
        &mut words("en"),
        index.as_ref(),
        "reader",
        &wanted,
        true,
    )
    .unwrap() else {
        panic!("a dialog")
    };
    assert!(dialog.text.len() <= 1024, "{}", dialog.text.len());
    dialog.question().unwrap();
    // Every plugin fetched was named in the dialog: what does not fit is not
    // asked for and is answered *declined*; the request keeps the named ones.
    assert!(!dialog.text.ends_with('…'));
    assert_eq!(dialog.text.lines().count(), dialog.ids.len());
    assert!(dialog.ids.len() < 16, "the text cannot name all of them");
    assert_eq!(dialog.ids.len() + dialog.declined.len(), 16);
    assert!(dialog.not_found.is_empty());
    for id in &dialog.ids {
        assert!(!dialog.declined.contains(id));
    }
    let open = Request::open(&storage).unwrap().unwrap();
    assert_eq!(open.ids, dialog.ids);
}

#[test]
fn the_whole_ask_keeps_what_was_last_shown() {
    let storage = MemoryStorage::new();
    indexed(&storage, &[("pack-a", "Pack A", 10)]);
    let index = Index::kept(&storage).unwrap();
    let wanted = strings(&["pack-a"]);
    let mut last = None;
    let mut say = words("en");
    let run = |last: &mut Option<Last>, say: &mut Words, now, flags| {
        ask::asked(
            &storage,
            say,
            index.as_ref(),
            last,
            ("reader", &wanted),
            now,
            flags,
        )
        .unwrap()
        .reply
    };
    let first = run(&mut last, &mut say, 10, (true, false, false));
    assert!(matches!(first, Reply::Dialog(_)));
    assert_eq!(
        last.as_ref().map(|l| (l.at_ms, l.declined)),
        Some((10, false))
    );
    // The same ask while its dialog is open is the one.
    assert_eq!(
        run(&mut last, &mut say, 20, (true, true, false)),
        Reply::Join
    );
    // Nothing missing is nothing to say.
    let none = ask::asked(
        &storage,
        &mut say,
        index.as_ref(),
        &mut last,
        ("reader", &[]),
        30,
        (true, false, false),
    )
    .unwrap()
    .reply;
    assert_eq!(none, Reply::Nothing);
}

#[test]
fn the_readers_word_is_a_decision() {
    use AnswerOutcome::{Answered, Cancelled, Typing};
    assert_eq!(ask::decision(Answered, 1, false), Decision::Download);
    assert_eq!(ask::decision(Answered, 0, false), Decision::NotNow);
    assert_eq!(ask::decision(Cancelled, 1, false), Decision::Ignore);
    assert_eq!(ask::decision(Typing, 1, false), Decision::Ignore);
    assert_eq!(ask::decision(Cancelled, 0, true), Decision::NotNow);
}

#[test]
fn not_now_is_told_as_declined_and_remembered() {
    let storage = MemoryStorage::new();
    let ids = strings(&["pack-a"]);
    Request::start(&storage, "reader", &ids).unwrap();
    let mut last = None;
    let reply = ask::answered(&storage, &mut last, 50, true, Decision::NotNow).unwrap();
    assert_eq!(
        reply,
        Reply::Answer {
            ids: ids.clone(),
            outcome: PluginOutcome::Declined
        }
    );
    assert!(storage.size(REQUEST).is_err(), "the request is closed");
    let last = last.unwrap();
    assert!(last.declined);
    assert_eq!(
        (last.by.as_str(), last.at_ms, last.ids),
        ("reader", 50, ids)
    );
    // No request open: nothing.
    let mut none = None;
    let reply = ask::answered(&storage, &mut none, 60, true, Decision::NotNow).unwrap();
    assert_eq!(reply, Reply::Nothing);
    assert!(none.is_none());
}

#[test]
fn download_queues_the_plugins_as_asked_for_and_shows_the_download() {
    let storage = MemoryStorage::new();
    let ids = strings(&["pack-a", "pack-b"]);
    Request::start(&storage, "reader", &ids).unwrap();
    let mut queue = Queue::default();
    queue.add(["airy"], Mark::Said).unwrap();
    queue.keep(&storage).unwrap();
    let mut last = None;
    let reply = ask::answered(&storage, &mut last, 50, true, Decision::Download).unwrap();
    let Reply::Fetch(Next::Open { screen, params }) = reply else {
        panic!("the download is shown")
    };
    assert_eq!(screen, "store.fetch");
    assert_eq!(
        params,
        [
            ("id", "reader".to_string()),
            ("text", "pack-a\npack-b".to_string())
        ]
    );
    let queue = Queue::read(&storage).unwrap();
    let marks: Vec<_> = queue
        .lines()
        .iter()
        .map(|l| (l.id.as_str(), l.mark))
        .collect();
    assert_eq!(
        marks,
        [
            ("airy", Mark::Said),
            ("pack-a", Mark::Requested),
            ("pack-b", Mark::Requested)
        ]
    );
    // The request stays open until the queue ends.
    assert!(storage.size(REQUEST).is_ok());
    assert!(last.is_none());
}

#[test]
fn the_connection_gone_meanwhile_is_failed_and_a_closed_dialog_is_declined() {
    let storage = MemoryStorage::new();
    let ids = strings(&["pack-a"]);
    Request::start(&storage, "reader", &ids).unwrap();
    let mut last = None;
    let ignored = ask::answered(&storage, &mut last, 1, true, Decision::Ignore).unwrap();
    // A closed dialog is not left open: the host would keep the request open
    // for good, and it allows only a few.
    assert_eq!(
        ignored,
        Reply::Answer {
            ids: ids.clone(),
            outcome: PluginOutcome::Declined
        }
    );
    assert!(storage.size(REQUEST).is_err());
    assert!(last.is_none(), "nothing remembered: the ask may come again");
    Request::start(&storage, "reader", &ids).unwrap();
    let reply = ask::answered(&storage, &mut last, 1, false, Decision::Download).unwrap();
    assert_eq!(
        reply,
        Reply::Answer {
            ids,
            outcome: PluginOutcome::Failed
        }
    );
    assert!(storage.size(REQUEST).is_err());
}

#[test]
fn the_end_tells_each_plugin_installed_or_failed() {
    let storage = MemoryStorage::new();
    let ids = strings(&["pack-a", "pack-b", "pack-c"]);
    Request::start(&storage, "reader", &ids).unwrap();
    outcome(&storage, "pack-b", true).unwrap();
    outcome(&storage, "pack-a", false).unwrap();
    let done: Finished = paperpaw_plugin_store::native_request::finished(&storage)
        .unwrap()
        .unwrap();
    let answers = ask::finished_answers(&done).unwrap();
    assert_eq!(
        answers,
        [
            ("pack-b", PluginOutcome::Installed),
            ("pack-a", PluginOutcome::Failed),
            ("pack-c", PluginOutcome::Failed)
        ]
    );
}

// --- the first-run offer ---------------------------------------------------

fn have_of(table: &'static [(&'static str, Have)]) -> impl Fn(&str) -> Have {
    move |id| {
        table
            .iter()
            .find(|(name, _)| *name == id)
            .map_or(Have::Absent, |(_, have)| *have)
    }
}

#[test]
fn the_offer_has_the_four_plugins_in_order_and_adds_what_can_be() {
    assert_eq!(ask::OFFERED, ["stats", "widgets", "calibre", "kosync"]);
    // A device that loads code plugins, with a network: all four.
    let none = have_of(&[]);
    assert_eq!(ask::rows(&none, true).unwrap(), ask::OFFERED);
    // No code plugins (or no network): nothing to offer, the step is left out.
    assert!(ask::rows(&none, false).unwrap().is_empty());
    // Android: built in and off are switched on; those on or off by the plugin
    // manager are not offered.
    let table = have_of(&[
        ("stats", Have::OffBuiltIn),
        ("widgets", Have::On),
        ("calibre", Have::OffOther),
    ]);
    assert_eq!(ask::rows(&table, false).unwrap(), ["stats"]);
    assert_eq!(ask::rows(&table, true).unwrap(), ["stats", "kosync"]);
    assert_eq!(ask::way(Have::On, true), Way::There);
    assert_eq!(ask::way(Have::Absent, false), Way::No);
}

#[test]
fn done_switches_on_fetches_and_says_which() {
    let table = have_of(&[("stats", Have::OffBuiltIn)]);
    let chosen = ask::choose(&["stats", "calibre"], &table, true).unwrap();
    assert_eq!(chosen.switch_on, ["stats"]);
    assert_eq!(chosen.fetch, ["calibre"]);
    assert_eq!(chosen.kept, "stats\ncalibre");
    assert_eq!(chosen.notify, "store.offer.adding");
    let only = ask::choose(&["stats"], &table, true).unwrap();
    assert!(only.fetch.is_empty());
    assert_eq!(only.notify, "store.offer.switched-on");
    let nothing = ask::choose(&[], &table, true).unwrap();
    assert_eq!(nothing.kept, "");
}

#[test]
fn what_is_chosen_is_queued_quietly_after_the_ones_waiting_and_kept_for_the_card() {
    let storage = MemoryStorage::new();
    let mut queue = Queue::default();
    queue.add(["airy"], Mark::Said).unwrap();
    queue.add(["stats"], Mark::Requested).unwrap();
    queue.keep(&storage).unwrap();
    let table = have_of(&[]);
    let chosen = ask::choose(&["stats", "widgets"], &table, true).unwrap();
    ask::keep_chosen(&storage, &chosen).unwrap();
    let text = String::from_utf8(storage.get("queue.txt").unwrap()).unwrap();
    // Each once: stats was waiting already.
    assert_eq!(text, "airy\n&stats\n~widgets\n");
    assert_eq!(storage.get(ask::CHOSEN).unwrap(), b"stats\nwidgets");
    assert!(storage.get(ask::CODE_TOLD).is_some());
    // Only switched on: no queue and no notice of code plugins.
    let other = MemoryStorage::new();
    let table = have_of(&[("stats", Have::OffBuiltIn)]);
    let chosen = ask::choose(&["stats"], &table, true).unwrap();
    ask::keep_chosen(&other, &chosen).unwrap();
    assert!(other.get("queue.txt").is_none() && other.get(ask::CODE_TOLD).is_none());
    assert!(other.get(ask::CHOSEN).is_some());
}

#[test]
fn the_setup_card_says_how_many_are_added_and_then_that_they_were() {
    let mut queue = Queue::default();
    queue.add(["stats", "airy", "kosync"], Mark::Quiet).unwrap();
    let waiting = ask::waiting_offered(&queue);
    assert_eq!(waiting, 2);
    assert_eq!(ask::badge(waiting, false), Badge::BeingAdded(2));
    assert_eq!(ask::badge(0, true), Badge::Added);
    assert_eq!(ask::badge(0, false), Badge::None);
    let mut en = words("en");
    assert_eq!(
        ask::badge_text(&mut en, Badge::BeingAdded(2))
            .unwrap()
            .as_deref(),
        Some("2 being added")
    );
    assert_eq!(
        ask::badge_text(&mut en, Badge::BeingAdded(1))
            .unwrap()
            .as_deref(),
        Some("1 being added")
    );
    assert_eq!(
        ask::badge_text(&mut en, Badge::Added).unwrap().as_deref(),
        Some("Added")
    );
    assert_eq!(ask::badge_text(&mut en, Badge::None).unwrap(), None);
    assert_eq!(
        ask::badge_text(&mut words("de"), Badge::BeingAdded(3))
            .unwrap()
            .as_deref(),
        Some("3 werden hinzugefügt")
    );
}

#[test]
fn every_offered_plugin_has_its_words_and_the_hint_is_chosen_by_state() {
    for id in ask::OFFERED {
        let (name, help) = ask::row_keys(id).unwrap();
        for catalog in [EN, DE] {
            assert!(catalog.contains(&format!("{name} = ")), "{name}");
            assert!(catalog.contains(&format!("{help} = ")), "{help}");
        }
    }
    assert_eq!(ask::row_keys("evil"), None);
    assert_eq!(ask::hint_key(true, true), "store.offer.hint.chosen");
    assert_eq!(ask::hint_key(false, true), "store.offer.hint.buttons");
    assert_eq!(ask::hint_key(false, false), "store.offer.hint");
}

#[test]
fn the_end_notices_say_what_was_added_and_what_was_not() {
    let mut en = words("en");
    let said = |say: &mut Words, done: &str, not: &str| {
        ask::quiet_notices(
            say,
            &Report {
                done: done.into(),
                not: not.into(),
            },
        )
        .unwrap()
    };
    assert_eq!(
        said(
            &mut en,
            "Reading statistics, Wireless library",
            "Progress sync"
        ),
        [
            "Added: Reading statistics, Wireless library. They start the next time PaperPaw starts.",
            "Could not be added: Progress sync. They are in the Plugin store."
        ]
    );
    // The faces of Literata have no hint of their own any more: the lean
    // firmware has none to offer (Decision 186).
    assert_eq!(
        said(&mut en, "", "Bold and italic, Progress sync"),
        ["Could not be added: Bold and italic, Progress sync. They are in the Plugin store."]
    );
    assert_eq!(
        said(&mut words("de"), "Lesestatistik", ""),
        ["Hinzugefügt: Lesestatistik. Sie starten beim nächsten Start von PaperPaw."]
    );
}

#[test]
fn the_quiet_notes_become_a_report_the_notices_read() {
    let storage = MemoryStorage::new();
    note(
        &storage,
        &ask::name_of(&mut words("en"), "stats").unwrap(),
        true,
    )
    .unwrap();
    note(&storage, "Bold and italic", false).unwrap();
    note(
        &storage,
        &ask::name_of(&mut words("en"), "unknown-one").unwrap(),
        false,
    )
    .unwrap();
    let report = report(&storage, &Queue::default()).unwrap().unwrap();
    assert_eq!(report.done, "Reading statistics");
    assert_eq!(report.not, "Bold and italic, unknown-one");
    let notices = ask::quiet_notices(&mut words("en"), &report).unwrap();
    assert_eq!(notices.len(), 2);
}

#[test]
fn a_name_is_looked_up_only_by_an_id_that_is_a_plugins_name() {
    let mut en = words("en");
    assert_eq!(ask::name_of(&mut en, "kosync").unwrap(), "Progress sync");
    // Not a plugin's name: no key is built from it.
    for hostile in ["stats\n", "Stats", "a b", "", "../stats", "stats.help"] {
        assert_eq!(ask::name_of(&mut en, hostile).unwrap(), hostile);
    }
}

#[test]
fn a_list_without_a_name_is_kept_as_it_is() {
    assert_eq!(
        ask::without("a, b, c", "b").unwrap().as_deref(),
        Some("a, c")
    );
    assert_eq!(ask::without("a, b", "z").unwrap(), None);
    assert_eq!(ask::without("b", "b").unwrap().as_deref(), Some(""));
    assert_eq!(ask::without("", "b").unwrap(), None);
}

#[test]
fn every_text_the_module_says_exists_in_both_languages() {
    let source = include_str!("../src/native_ask.rs");
    let screen = include_str!("../src/native_screen_fetch.rs");
    let mut found = 0;
    for part in source.split('"').chain(screen.split('"')) {
        if let Some(rest) = part.strip_prefix("store.")
            && !part.ends_with('.')
            && !part.contains(' ')
            && !rest.is_empty()
        {
            found += 1;
            for (name, catalog) in [("en", EN), ("de", DE)] {
                let plain = catalog.contains(&format!("\n{part} = "));
                let plural = catalog.contains(&format!("\n{part}.other = "));
                assert!(plain || plural, "{part} missing in {name}");
            }
        }
    }
    assert!(found > 20, "{found}");
}

#[test]
fn the_labels_fit_a_confirmation() {
    for catalog in [EN, DE] {
        for key in ["store.request.download", "store.request.not-now"] {
            let line = catalog
                .lines()
                .find_map(|l| l.strip_prefix(&format!("{key} = ")))
                .unwrap();
            assert!(line.len() <= 128 && !line.contains('\n'));
        }
    }
}

// --- every id gets an outcome on every path ---------------------------------

/// A catalog that says nothing: the host refuses.
struct Mute;

impl Say for Mute {
    fn lookup(&mut self, _: &str) -> Result<Option<String>, Error> {
        Err(Error::Unsupported)
    }
    fn format(&mut self, _: &str, _: &[Arg<'_>]) -> Result<String, Error> {
        Err(Error::Unsupported)
    }
    fn digits(&mut self, _: &str) -> Result<String, Error> {
        Err(Error::Unsupported)
    }
}

#[test]
fn the_request_a_closed_dialog_left_is_answered_declined_by_the_next_ask() {
    let storage = MemoryStorage::new();
    indexed(
        &storage,
        &[("pack-a", "Pack A", 10), ("pack-b", "Pack B", 10)],
    );
    let index = Index::kept(&storage).unwrap();
    let left = strings(&["pack-a"]);
    Request::start(&storage, "reader", &left).unwrap();
    let wanted = strings(&["pack-b"]);
    let mut last = None;
    // Another plugin asks; the card holds a request nobody answered.
    let told = ask::asked(
        &storage,
        &mut words("en"),
        index.as_ref(),
        &mut last,
        ("store", &wanted),
        5,
        (true, true, false),
    )
    .unwrap();
    assert_eq!(told.stale, left);
    let Reply::Dialog(dialog) = told.reply else {
        panic!("a dialog")
    };
    assert_eq!(dialog.ids, wanted);
    let open = Request::open(&storage).unwrap().unwrap();
    assert_eq!((open.by.as_str(), open.ids), ("store", wanted));
}

#[test]
fn an_ask_the_store_cannot_go_on_with_is_answered_failed() {
    let storage = MemoryStorage::new();
    indexed(&storage, &[("pack-a", "Pack A", 10)]);
    let index = Index::kept(&storage).unwrap();
    let wanted = strings(&["pack-a"]);
    let mut last = None;
    // The host refuses the words: no dialog, so nothing may stay open.
    let told = ask::asked(
        &storage,
        &mut Mute,
        index.as_ref(),
        &mut last,
        ("reader", &wanted),
        5,
        (true, false, false),
    )
    .unwrap();
    assert_eq!(told.reply, Reply::AnswerAll(PluginOutcome::Failed));
    assert!(storage.size(REQUEST).is_err(), "no request left open");
    assert!(last.is_none());
}

#[test]
fn a_queue_the_card_refuses_is_answered_failed_not_dropped() {
    let storage = MemoryStorage::new();
    let ids = strings(&["pack-a", "pack-b"]);
    Request::start(&storage, "reader", &ids).unwrap();
    let card = Refusing(&storage);
    let mut last = None;
    let reply = ask::answered(&card, &mut last, 1, true, Decision::Download).unwrap();
    assert_eq!(
        reply,
        Reply::Answer {
            ids,
            outcome: PluginOutcome::Failed
        }
    );
    assert!(storage.size(REQUEST).is_err(), "the request is closed");
}

#[test]
fn the_end_of_a_request_is_told_without_memory() {
    let done = Finished {
        by: "reader".into(),
        installed: strings(&["pack-b"]),
        missing: strings(&["pack-a"]),
    };
    let told: Vec<_> = ask::finished_outcomes(&done).collect();
    assert_eq!(
        told,
        [
            ("pack-b", PluginOutcome::Installed),
            ("pack-a", PluginOutcome::Failed)
        ]
    );
}
