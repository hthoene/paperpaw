//! Catalogs on the card, read as needed (ADR 0033): compiled and opened
//! they give every message as the text catalog does; one read a message,
//! kept until forgotten, and no more than the limit; a card pulled or a
//! damaged file gives no message and no panic, and the language falls
//! back; a catalog of an Arabic interface's size takes about a kilobyte.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use paperpaw_i18n::{Catalog, CatalogError, CatalogFile, Direction, MAX_READ, Translator};
use proptest::prelude::*;

/// A file in memory that counts its reads and can be pulled.
#[derive(Default)]
struct File {
    bytes: RefCell<Vec<u8>>,
    reads: Cell<u32>,
    pulled: Cell<bool>,
}

impl File {
    fn new(bytes: Vec<u8>) -> Rc<File> {
        Rc::new(File {
            bytes: RefCell::new(bytes),
            ..File::default()
        })
    }
}

impl CatalogFile for File {
    fn read_exact_at(&self, offset: u32, buf: &mut [u8]) -> bool {
        self.reads.set(self.reads.get() + 1);
        let bytes = self.bytes.borrow();
        let start = offset as usize;
        match bytes.get(start..start + buf.len()) {
            Some(part) if !self.pulled.get() => {
                buf.copy_from_slice(part);
                true
            }
            _ => false,
        }
    }

    fn size(&self) -> u32 {
        self.bytes.borrow().len() as u32
    }
}

const EN: &str = "[meta]\nlanguage = en\nname = English\n[messages]\n\
    hello = Hello\nonly-en = English only\nbooks.one = {count} book\nbooks.other = {count} books\n";

const AR: &str = "[meta]\nlanguage = ar\nname = العربية\nplural = arabic\ndirection = rtl\n\
    [messages]\nhello = مرحبا\nbooks.zero = لا كتب\nbooks.other = {count} كتاب\n\
    quoted = \"  حول  \"\nescaped = \"سطر\\nآخر\"\nhello = أهلا\n";

fn opened(source: &str) -> (Catalog, Rc<File>) {
    let file = File::new(Catalog::compile(source).unwrap());
    (Catalog::open(file.clone()).unwrap(), file)
}

#[test]
fn a_compiled_catalog_gives_every_message_of_the_text() {
    let text = Catalog::parse(AR).unwrap();
    let (card, _) = opened(AR);
    assert!(card.is_on_card() && !text.is_on_card());
    assert_eq!(card.language(), "ar");
    assert_eq!(card.name(), "العربية");
    assert_eq!(card.direction(), Direction::Rtl);
    assert_eq!(card.plural_rule(), text.plural_rule());
    assert_eq!(card.len(), text.len());
    for key in text.keys() {
        assert_eq!(card.get(key), text.get(key), "{key}");
    }
    // The later of two wins, as in the text.
    assert_eq!(card.get("hello"), Some("أهلا"));
    assert_eq!(card.get("escaped"), Some("سطر\nآخر"));
    assert_eq!(card.get("quoted"), Some("  حول  "));
    assert_eq!(card.get("missing"), None);
    assert_eq!(card.get(""), None);
    // Listing keys would read it whole.
    assert_eq!(card.keys().count(), 0);
}

#[test]
fn a_message_is_read_once_until_forgotten() {
    let (mut card, file) = opened(AR);
    let opening = file.reads.get();
    assert_eq!(card.get("hello"), Some("أهلا"));
    assert_eq!(card.get("hello"), Some("أهلا"));
    assert_eq!(card.get("missing"), None);
    assert_eq!(card.get("missing"), None);
    // One read each, that there is none kept too.
    assert_eq!(file.reads.get(), opening + 2);
    assert!(card.read_bytes() > 0);
    card.forget_read();
    assert_eq!(card.read_bytes(), 0);
    assert_eq!(card.get("hello"), Some("أهلا"));
    assert_eq!(file.reads.get(), opening + 3);
}

#[test]
fn messages_borrowed_stay_while_more_are_read() {
    let source = many(150, "x");
    let (card, _) = opened(&source);
    let first = card.get("key.0000").unwrap();
    let all: Vec<&str> = (0..150)
        .map(|n| card.get(&format!("key.{n:04}")).unwrap())
        .collect();
    assert_eq!(first, "message 0 x");
    assert_eq!(all[149], "message 149 x");
    assert!(
        all.iter()
            .enumerate()
            .all(|(n, m)| *m == format!("message {n} x"))
    );
}

#[test]
fn no_more_is_read_beyond_the_limit_and_the_language_falls_back() {
    let source = many(2000, &"ب".repeat(20));
    let mut t = Translator::new();
    t.add(Catalog::parse(english(2000)).unwrap());
    let file = File::new(Catalog::compile(&source).unwrap());
    t.add(Catalog::open(file.clone()).unwrap());
    t.set_language("ar");
    let mut read_all = 0;
    for n in 0..2000 {
        let key = format!("key.{n:04}");
        if t.tr(&key).starts_with("message") && t.tr(&key).contains('ب') {
            read_all += 1;
        } else {
            // English stood in.
            assert_eq!(t.tr(&key), format!("english {n}"));
        }
    }
    assert!(read_all > 100 && read_all < 2000, "{read_all}");
    assert!(t.read_bytes() <= MAX_READ + 200, "{}", t.read_bytes());
    let reads = file.reads.get();
    let _ = t.tr("key.1999");
    assert_eq!(file.reads.get(), reads, "nothing read past the limit");
    t.forget_read();
    assert!(t.tr("key.1999").contains('ب'));
}

#[test]
fn a_pulled_card_gives_english_or_the_key() {
    let mut t = Translator::new();
    t.add(Catalog::parse(EN).unwrap());
    let file = File::new(Catalog::compile(AR).unwrap());
    t.add(Catalog::open(file.clone()).unwrap());
    t.set_language("ar");
    assert_eq!(t.direction(), Direction::Rtl);
    assert_eq!(t.tr("hello"), "أهلا");
    file.pulled.set(true);
    // Read before: still there until forgotten.
    assert_eq!(t.tr("hello"), "أهلا");
    t.forget_read();
    assert_eq!(t.tr("hello"), "Hello");
    assert_eq!(t.tr("only-en"), "English only");
    assert_eq!(t.tr("nowhere"), "nowhere");
    assert_eq!(t.plural("books", 0, &[]), "0 books");
    // Back in: read again once forgotten.
    file.pulled.set(false);
    assert_eq!(t.tr("hello"), "Hello", "the miss is kept until forgotten");
    t.forget_read();
    assert_eq!(t.tr("hello"), "أهلا");
    assert_eq!(t.plural("books", 0, &[]), "لا كتب");
    assert_eq!(t.plural("books", 5, &[]), "5 كتاب");
}

#[test]
fn a_language_switched_away_keeps_only_its_meta_and_comes_back() {
    let mut t = Translator::new();
    t.add(Catalog::parse(EN).unwrap());
    let file = File::new(Catalog::compile(&many(3000, "ج")).unwrap());
    t.add(Catalog::open(file.clone()).unwrap());
    t.set_language("ar");
    assert!(t.indexes_on_heap() > 0);
    assert_eq!(t.tr("key.0001"), "message 1 ج");
    t.set_language("en");
    // English is built in: no index on the heap but its own.
    let english_only = {
        let mut e = Translator::new();
        e.add(Catalog::parse(EN).unwrap());
        e.indexes_on_heap()
    };
    assert_eq!(t.indexes_on_heap(), english_only);
    assert_eq!(t.read_bytes(), 0);
    assert!(t.languages().iter().any(|(tag, _)| *tag == "ar"));
    // The card pulled meanwhile: the language falls back whole.
    file.pulled.set(true);
    t.set_language("ar");
    assert_eq!(t.tr("key.0001"), "key.0001");
    assert_eq!(t.tr("hello"), "Hello");
    file.pulled.set(false);
    t.set_language("en");
    t.set_language("ar");
    assert_eq!(t.tr("key.0001"), "message 1 ج");
}

#[test]
fn english_stays_beside_a_catalog_on_the_card() {
    // A language on the card that has every English message does not
    // make English drop out: it would be asked for all of them.
    let mut t = Translator::new();
    t.add(Catalog::parse(EN).unwrap());
    let all = "[meta]\nlanguage = de\n[messages]\nhello = Hallo\nonly-en = Nur\n\
        books.one = {count} Buch\nbooks.other = {count} Bücher\n";
    let file = File::new(Catalog::compile(all).unwrap());
    t.add(Catalog::open(file.clone()).unwrap());
    t.set_language("de");
    t.shrink_to_fit();
    let reads = file.reads.get();
    assert_eq!(t.tr("hello"), "Hallo");
    assert_eq!(
        file.reads.get(),
        reads + 1,
        "nothing read to check coverage"
    );
    file.pulled.set(true);
    t.forget_read();
    assert_eq!(t.tr("hello"), "Hello");
}

#[test]
fn a_catalog_of_an_arabic_interfaces_size_takes_a_kilobyte() {
    // Some 3,400 messages, 210 KB of text, as the Arabic catalog of all
    // plugins.
    let message = "نص الرسالة بالعربية لواجهة المستخدم";
    let source = many(3400, message);
    assert!(source.len() > 200_000, "{}", source.len());
    let compiled = Catalog::compile(&source).unwrap();
    let file = File::new(compiled);
    let mut card = Catalog::open(file.clone()).unwrap();
    assert_eq!(card.len(), 3400);
    // A block of 2 KiB holds some 20 of these messages.
    assert!(card.index_on_heap() <= 1400, "{}", card.index_on_heap());
    // A screen's worth: 30 messages, a read each, a few KiB kept.
    let opening = file.reads.get();
    for n in (0..3400).step_by(113) {
        assert_eq!(
            card.get(&format!("key.{n:04}")),
            Some(&*format!("message {n} {message}"))
        );
    }
    assert_eq!(file.reads.get(), opening + 31);
    // On a computer: on the X4 the list takes half the words.
    assert!(card.read_bytes() < 5 * 1024, "{}", card.read_bytes());
    // Every message is found, a screen at a time.
    for n in 0..3400 {
        if n % 50 == 0 {
            card.forget_read();
        }
        assert_eq!(
            card.get(&format!("key.{n:04}")),
            Some(&*format!("message {n} {message}"))
        );
    }
    // The text catalog of the same takes the whole text and more.
    let text = Catalog::parse(source).unwrap();
    assert!(text.index_on_heap() > 6 * 1024);
}

#[test]
fn compiling_refuses_what_does_not_fit() {
    let long_key = format!(
        "[meta]\nlanguage = xx\n[messages]\n{} = a\n",
        "k".repeat(256)
    );
    assert_eq!(Catalog::compile(&long_key), Err(CatalogError::TooLarge));
    let long = format!(
        "[meta]\nlanguage = xx\n[messages]\nk = {}\n",
        "m".repeat(4097)
    );
    assert_eq!(Catalog::compile(&long), Err(CatalogError::TooLarge));
    // The longest message has a block of its own.
    let longest = format!(
        "[meta]\nlanguage = xx\n[messages]\na = 1\nk = {}\nz = 2\n",
        "m".repeat(4096)
    );
    let (card, _) = opened(&longest);
    assert_eq!(card.get("k").map(str::len), Some(4096));
    assert_eq!(card.get("z"), Some("2"));
    // An empty one opens and has nothing.
    let (empty, _) = opened("[meta]\nlanguage = xx\n");
    assert!(empty.is_empty());
    assert_eq!(empty.get("a"), None);
    // Text that is no catalog is no compiled one either.
    assert_eq!(
        Catalog::compile("[messages]\na = 1\n"),
        Err(CatalogError::MissingLanguage)
    );
}

#[test]
fn files_that_are_not_compiled_catalogs_are_refused() {
    let good = Catalog::compile(AR).unwrap();
    for len in 0..good.len() {
        // Cut short: refused, never a panic.
        let cut = File::new(good[..len].to_vec());
        assert!(Catalog::open(cut).is_err(), "cut at {len}");
    }
    let mut other = good.clone();
    other[0] = b'X';
    assert_eq!(
        Catalog::open(File::new(other)).err(),
        Some(CatalogError::Corrupt)
    );
    let mut newer = good.clone();
    newer[4] = 2;
    assert_eq!(
        Catalog::open(File::new(newer)).err(),
        Some(CatalogError::Corrupt)
    );
    // A text catalog is no compiled one.
    assert!(Catalog::open(File::new(AR.as_bytes().to_vec())).is_err());
    // The card pulled before it opened.
    let pulled = File::new(good);
    pulled.pulled.set(true);
    assert_eq!(Catalog::open(pulled).err(), Some(CatalogError::Corrupt));
}

/// `n` messages `key.NNNN = message N <text>` in a catalog `ar`.
fn many(n: usize, text: &str) -> String {
    let mut source = String::from("[meta]\nlanguage = ar\nname = عربي\n[messages]\n");
    for i in 0..n {
        source.push_str(&format!("key.{i:04} = message {i} {text}\n"));
    }
    source
}

/// `n` messages `key.NNNN = english N` in English.
fn english(n: usize) -> String {
    let mut source = String::from("[meta]\nlanguage = en\n[messages]\n");
    for i in 0..n {
        source.push_str(&format!("key.{i:04} = english {i}\n"));
    }
    source
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// A damaged file (bytes changed anywhere) is refused or gives
    /// messages, its own or none, and never panics.
    #[test]
    fn a_damaged_file_never_panics(changes in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8)) {
        let mut bytes = Catalog::compile(&many(80, "د")).unwrap();
        for (at, value) in changes {
            let at = at.index(bytes.len());
            bytes[at] = value;
        }
        if let Ok(catalog) = Catalog::open(File::new(bytes)) {
            for n in 0..80 {
                let _ = catalog.get(&format!("key.{n:04}"));
            }
            let _ = catalog.get("other");
        }
    }

    /// Generated catalogs give every message on the card as in the text.
    #[test]
    fn generated_catalogs_agree(
        messages in prop::collection::btree_map("[a-z][a-z0-9._-]{0,20}", "[^\"\\\\\n\r]{0,60}", 0..200),
    ) {
        let mut source = String::from("[meta]\nlanguage = xx\n[messages]\n");
        for (key, message) in &messages {
            source.push_str(&format!("{key} = \"{message}\"\n"));
        }
        let text = Catalog::parse(source.clone()).unwrap();
        let (mut card, _) = opened(&source);
        prop_assert_eq!(card.len(), text.len());
        for key in text.keys() {
            if card.read_bytes() > MAX_READ / 2 {
                card.forget_read();
            }
            prop_assert_eq!(card.get(key), text.get(key));
        }
        prop_assert_eq!(card.get("not-a-key-at-all!"), None);
    }
}

/// A store plugin's catalog on the card, as the kernel compiles it.
const STATS: &str = "[meta]\nlanguage = en\nname = English\n\n[messages]\n\
    stats.books = Books\nstats.authors = Authors\nstats.year.ring = Ring\n";

/// A key outside a compiled catalog's keys is known not to be in it without
/// a read of the card and without being remembered (#305): the kernel's own
/// texts and those of other plugins are asked of every catalog on the card
/// first, after every change of screen.
#[test]
fn a_key_outside_a_compiled_catalogs_keys_is_not_read_from_the_card() {
    let (card, file) = opened(STATS);
    let opened = file.reads.get();
    for key in [
        "library.title",
        "settings.wifi",
        "status.battery",
        "zzz",
        "a",
    ] {
        assert_eq!(card.get(key), None);
    }
    assert_eq!(file.reads.get(), opened, "nothing read");
    assert_eq!(card.read_bytes(), 0, "nothing remembered");
    // Its own keys are read, the first and the last too; one between them
    // that it lacks is read once and remembered.
    assert_eq!(card.get("stats.authors"), Some("Authors"));
    assert_eq!(card.get("stats.year.ring"), Some("Ring"));
    assert_eq!(card.get("stats.books"), Some("Books"));
    let read = file.reads.get();
    assert!(read > opened);
    assert_eq!(card.get("stats.none"), None);
    assert_eq!(card.get("stats.none"), None);
    assert!(file.reads.get() <= read + 1);
    // Through a translator: a built-in text is found in the catalog after it
    // with no read of the card catalog asked first.
    let mut t = Translator::new();
    t.add(
        Catalog::parse(
            "[meta]\nlanguage = en\nname = English\n[messages]\nlibrary.title = Library\n",
        )
        .unwrap(),
    );
    let file = File::new(Catalog::compile(STATS).unwrap());
    t.add(Catalog::open(file.clone()).unwrap());
    let opened = file.reads.get();
    assert_eq!(t.tr("library.title"), "Library");
    assert_eq!(t.tr("stats.books"), "Books");
    assert_eq!(file.reads.get(), opened + 1);
}

/// A catalog that overrides texts of others (a language pack) spans their
/// keys and still wins; one compiled before the keys were said, or whose
/// keys are not plain, is read as before.
#[test]
fn a_catalog_that_spans_other_keys_still_wins_and_one_without_its_keys_is_read() {
    let pack = "[meta]\nlanguage = en\nname = English\n[messages]\n\
        a.first = A\nlibrary.title = My books\nzz.last = Z\n";
    let mut t = Translator::new();
    t.add(
        Catalog::parse(
            "[meta]\nlanguage = en\nname = English\n[messages]\nlibrary.title = Library\n",
        )
        .unwrap(),
    );
    t.add(Catalog::open(File::new(Catalog::compile(pack).unwrap())).unwrap());
    assert_eq!(t.tr("library.title"), "My books");
    // As compiled before: the meta text ends without the keys.
    let mut old = Catalog::compile(STATS).unwrap();
    let said = "first-key = stats.authors\nlast-key = stats.year.ring\n";
    let at = old
        .windows(said.len())
        .position(|w| w == said.as_bytes())
        .expect("the keys are said at the end of the meta text");
    old[at..at + said.len()].fill(b'\n');
    let file = File::new(old);
    let card = Catalog::open(file.clone()).unwrap();
    let opened = file.reads.get();
    assert_eq!(card.get("library.title"), None);
    assert_eq!(file.reads.get(), opened + 1, "read to learn it");
    assert_eq!(card.get("stats.books"), Some("Books"));
    // Keys that are not plain say none.
    let odd = "[meta]\nlanguage = en\nname = English\n[messages]\n\"a key\" = A\nb = B\n";
    if let Ok(bytes) = Catalog::compile(odd) {
        assert!(!bytes.windows(9).any(|w| w == b"first-key"));
    }
}
