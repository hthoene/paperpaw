//! Looking words up with the real kernel and StarDict dictionaries made
//! here: a word found (its definitions paged and scrolled, whatever its
//! case and the spaces around it, punctuation inside it), a word not
//! found, no dictionary at all, damaged dictionaries beside good ones,
//! dictionaries in folders below, the index prepared once, and any word
//! at all without a panic.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::{InputEvent, PointerEvent, PointerPhase, PointerTool, Rotation};
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Point, Storage, StorageError,
};
use proptest::prelude::*;

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const TOUCH: DeviceInfo = DEVICE.with_input(InputCaps::new(false, true, false, &[]));

/// Lines in each definition: several pages of the panel.
const LINES: usize = 30;

/// The definition of `word` in the dictionary `name`.
fn definition(name: &str, word: &str) -> String {
    (0..LINES)
        .map(|i| format!("{name} line {i} on {word}."))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A StarDict dictionary `name` of plain text definitions at `dir` in
/// `storage`, with `words` (put in StarDict's order here).
fn dictionary(storage: &MemoryStorage, dir: &str, name: &str, words: &[&str]) {
    let mut words: Vec<&str> = words.to_vec();
    words.sort_by(|a, b| {
        a.to_ascii_lowercase()
            .cmp(&b.to_ascii_lowercase())
            .then_with(|| a.cmp(b))
    });
    let (mut idx, mut dict) = (Vec::new(), Vec::new());
    for word in words.iter() {
        let text = definition(name, word);
        idx.extend_from_slice(word.as_bytes());
        idx.push(0);
        idx.extend_from_slice(&(dict.len() as u32).to_be_bytes());
        idx.extend_from_slice(&(text.len() as u32).to_be_bytes());
        dict.extend_from_slice(text.as_bytes());
    }
    let ifo = format!(
        "StarDict's dict ifo file\nversion=2.4.2\nbookname={name}\nwordcount={}\nidxfilesize={}\nsametypesequence=m\n",
        words.len(),
        idx.len()
    );
    storage.create_dir(dir).unwrap();
    storage
        .write(&format!("{dir}/{name}.ifo"), ifo.as_bytes())
        .unwrap();
    storage.write(&format!("{dir}/{name}.idx"), &idx).unwrap();
    storage.write(&format!("{dir}/{name}.dict"), &dict).unwrap();
}

const WORDS: [&str; 5] = ["apple", "Hus", "hus", "e.g.", "zebra"];

/// A storage the test looks into while kernels have it.
#[derive(Clone, Default)]
struct Card(Rc<MemoryStorage>);

impl std::ops::Deref for Card {
    type Target = MemoryStorage;
    fn deref(&self) -> &MemoryStorage {
        &self.0
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

/// A card with one dictionary in the default folder.
fn card() -> Card {
    let storage = Card::default();
    dictionary(&storage, "dictionaries/test", "Test", &WORDS);
    storage
}

struct Lookup {
    kernel: Kernel,
    now: u64,
    /// The screen as last rendered, by rows.
    image: Vec<Vec<bool>>,
}

impl Lookup {
    fn new(device: DeviceInfo, storage: Card, config: &str) -> Self {
        let mut kernel = Kernel::builder(device)
            .storage(storage)
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_dictionary::DictionaryPlugin)
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut lookup = Self {
            kernel,
            now: 0,
            image: Vec::new(),
        };
        lookup.settle(effects);
        lookup
    }

    fn settle(&mut self, mut effects: paperpaw_core::Effects) {
        for _ in 0..100_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    /// Looks `word` up, as the reader asks for it, and waits until the
    /// dictionaries are prepared.
    fn look_up(&mut self, word: &str) {
        self.now += 100;
        let params = Params::new()
            .with(params::WORD, word)
            .with(params::LANGUAGE, "en");
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::LOOKUP, params);
        self.settle(effects);
    }

    fn press(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn screen(&self) -> String {
        self.kernel
            .active_screen()
            .map(|id| id.as_str().to_string())
            .unwrap_or_default()
    }

    /// What the panel describes: the word, the page.
    fn described(&mut self) -> Vec<String> {
        self.render();
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }

    /// The page of the definitions shown and their pages, if more than
    /// one.
    fn page(&mut self) -> Option<(usize, usize)> {
        self.described().iter().find_map(|label| {
            let (page, pages) = label.split_once(" / ")?;
            Some((page.trim().parse().ok()?, pages.trim().parse().ok()?))
        })
    }

    /// Renders what changed into [`Lookup::image`].
    fn render(&mut self) {
        let display = DEVICE.display;
        if self.image.is_empty() {
            self.image = vec![vec![false; usize::from(display.width)]; usize::from(display.height)];
        }
        let image = &mut self.image;
        self.kernel.render(&mut |y, view| {
            for row in 0..view.height {
                let Some(line) = image.get_mut(usize::from(y + row)) else {
                    continue;
                };
                for x in 0..view.width {
                    line[usize::from(x)] = view.pixel(x, row).luma() < 128;
                }
            }
        });
    }

    /// The panel below its header, as pixels.
    fn body(&mut self) -> Vec<Vec<bool>> {
        self.render();
        let top = usize::from(DEVICE.display.height) * 9 / 20 + 60;
        self.image[top..].to_vec()
    }

    fn tap(&mut self, point: Point) {
        self.now += 1000;
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 40;
            let event = InputEvent::Pointer(PointerEvent::new(
                0,
                phase,
                point,
                PointerTool::Finger,
                1000,
            ));
            let effects = self.kernel.handle(Instant(self.now), event);
            // Timers wait while the finger is down: a long press is not
            // meant.
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
    }
}

#[test]
fn a_word_found_shows_its_definition_page_by_page() {
    let mut lookup = Lookup::new(DEVICE, card(), "");
    lookup.look_up("apple");
    assert_eq!(lookup.screen(), "dictionary.popup");
    assert!(lookup.described().iter().any(|l| l == "apple"));
    let (page, pages) = lookup.page().expect("pages of definitions");
    assert_eq!(page, 1);
    assert!(pages >= 2, "{pages}");
    for expected in 2..=pages {
        lookup.press(actions::NEXT_PAGE);
        assert_eq!(lookup.page(), Some((expected, pages)));
    }
    // Not past the last page, nor before the first.
    lookup.press(actions::DOWN);
    assert_eq!(lookup.page(), Some((pages, pages)));
    for _ in 0..pages + 2 {
        lookup.press(actions::UP);
    }
    assert_eq!(lookup.page(), Some((1, pages)));
    // Confirm closes it.
    lookup.press(actions::CONFIRM);
    assert_ne!(lookup.screen(), "dictionary.popup");
}

#[test]
fn case_and_spaces_around_the_word_do_not_matter() {
    let mut lookup = Lookup::new(DEVICE, card(), "");
    for word in ["APPLE", "  Apple\t", "ZeBrA"] {
        lookup.look_up(word);
        assert!(lookup.page().is_some(), "{word:?}");
        lookup.press(actions::CONFIRM);
    }
    // Both spellings of a word: twice the definitions.
    lookup.look_up("apple");
    let (_, one) = lookup.page().unwrap();
    lookup.press(actions::CONFIRM);
    lookup.look_up("hus");
    let (_, two) = lookup.page().unwrap();
    assert!(two > one, "{two} for {one}");
}

#[test]
fn punctuation_inside_a_headword_is_part_of_it() {
    let mut lookup = Lookup::new(DEVICE, card(), "");
    lookup.look_up("e.g.");
    assert!(lookup.page().is_some());
}

#[test]
fn a_word_not_found_says_so() {
    let mut lookup = Lookup::new(DEVICE, card(), "");
    lookup.look_up("quagga");
    assert_eq!(lookup.screen(), "dictionary.popup");
    assert!(lookup.described().iter().any(|l| l == "quagga"));
    assert_eq!(lookup.page(), None);
    // The message names the word: another word, another message.
    let quagga = lookup.body();
    lookup.press(actions::NEXT_PAGE);
    assert_eq!(lookup.screen(), "dictionary.popup");
    lookup.press(actions::CONFIRM);
    lookup.look_up("okapi");
    assert_ne!(lookup.body(), quagga);
    // Punctuation around a word is not taken away here (the reader
    // does): not found as it is.
    lookup.press(actions::CONFIRM);
    lookup.look_up("\u{201c}apple,\u{201d}");
    assert_eq!(lookup.page(), None);
}

#[test]
fn without_dictionaries_it_says_where_they_go() {
    let mut lookup = Lookup::new(DEVICE, Card::default(), "");
    lookup.look_up("apple");
    assert_eq!(lookup.screen(), "dictionary.popup");
    assert_eq!(lookup.page(), None);
    // The same message for every word.
    let apple = lookup.body();
    lookup.press(actions::CONFIRM);
    lookup.look_up("zebra");
    assert_eq!(lookup.body(), apple);
}

#[test]
fn damaged_dictionaries_are_skipped_and_good_ones_used() {
    let storage = card();
    // An .ifo that is none, an index cut short, definitions missing.
    storage.create_dir("dictionaries/bad").unwrap();
    storage
        .write("dictionaries/bad/noise.ifo", b"\xff\xfe not a dictionary")
        .unwrap();
    dictionary(&storage, "dictionaries/cut", "Cut", &WORDS);
    let idx = storage.get("dictionaries/cut/Cut.idx").unwrap();
    storage
        .write("dictionaries/cut/Cut.idx", &idx[..idx.len() / 2 + 3])
        .unwrap();
    dictionary(&storage, "dictionaries/gone", "Gone", &WORDS);
    storage.remove("dictionaries/gone/Gone.dict").unwrap();
    // Counts larger than the files.
    dictionary(&storage, "dictionaries/lies", "Lies", &WORDS);
    let ifo = "StarDict's dict ifo file\nversion=2.4.2\nbookname=Lies\nwordcount=4000000000\nidxfilesize=4000000000\nsametypesequence=m\n";
    storage
        .write("dictionaries/lies/Lies.ifo", ifo.as_bytes())
        .unwrap();
    let mut lookup = Lookup::new(DEVICE, storage, "");
    for word in ["apple", "zebra", "quagga"] {
        lookup.look_up(word);
        assert_eq!(lookup.screen(), "dictionary.popup", "{word}");
        lookup.press(actions::CONFIRM);
    }
    lookup.look_up("apple");
    assert!(lookup.page().is_some());
}

#[test]
fn dictionaries_are_found_in_folders_below_but_not_too_deep_or_hidden() {
    let storage = Card::default();
    dictionary(&storage, "dicts/a/b/c", "Deep", &["deep"]);
    dictionary(&storage, "dicts/a/b/c/d/e", "Deeper", &["deeper"]);
    dictionary(&storage, "dicts/.hidden", "Hidden", &["hidden"]);
    let mut lookup = Lookup::new(DEVICE, storage, "[dictionary]\nfolder = \"/dicts/\"\n");
    lookup.look_up("deep");
    assert!(lookup.page().is_some());
    for word in ["deeper", "hidden"] {
        lookup.press(actions::CONFIRM);
        lookup.look_up(word);
        assert_eq!(lookup.page(), None, "{word}");
    }
}

#[test]
fn the_index_is_prepared_once_and_kept() {
    let storage = card();
    let mut lookup = Lookup::new(DEVICE, storage.clone(), "");
    lookup.look_up("apple");
    assert!(lookup.page().is_some());
    // Kept on the card, in the dictionary's own folder.
    let mut kept = Vec::new();
    let _ = storage.list(".paperpaw/plugins/dictionary", &mut |e| {
        kept.push(e.name.to_string());
    });
    assert_eq!(kept.len(), 1, "{kept:?}");
    // Another start: the word is there at once, without preparing.
    let mut again = Lookup::new(DEVICE, storage, "");
    again.now += 100;
    let params = Params::new().with(params::WORD, "zebra");
    let effects = again
        .kernel
        .open_slot(Instant(again.now), slots::LOOKUP, params);
    assert!(again.page().is_some());
    again.settle(effects);
}

#[test]
fn taps_scroll_on_and_back_and_close_at_the_end() {
    let mut lookup = Lookup::new(TOUCH, card(), "");
    lookup.look_up("apple");
    let (_, pages) = lookup.page().unwrap();
    // The right part scrolls on, the left part back.
    let (right, left) = (Point::new(400, 700), Point::new(40, 700));
    lookup.tap(right);
    assert_eq!(lookup.page(), Some((2, pages)));
    lookup.tap(left);
    assert_eq!(lookup.page(), Some((1, pages)));
    for _ in 1..pages {
        lookup.tap(right);
    }
    assert_eq!(lookup.page(), Some((pages, pages)));
    // At the end a tap on the right closes the panel.
    lookup.tap(right);
    assert_ne!(lookup.screen(), "dictionary.popup");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48)))]

    /// Any word: the panel opens and shows, and closes.
    #[test]
    fn any_word_is_looked_up_without_a_panic(word in "\\PC{0,300}") {
        let mut lookup = Lookup::new(DEVICE, card(), "");
        lookup.look_up(&word);
        prop_assert_eq!(lookup.screen(), "dictionary.popup");
        lookup.described();
        lookup.press(actions::NEXT_PAGE);
        lookup.press(actions::CONFIRM);
        prop_assert_ne!(lookup.screen(), "dictionary.popup");
    }
}

/// Providers besides the dictionaries: one online, one offline answering
/// later; and a screen for the online lookup.
struct Providers;

const WEB: paperpaw_sdk::Id = paperpaw_sdk::Id::new_static("providers.web");
const SLOW: paperpaw_sdk::Id = paperpaw_sdk::Id::new_static("providers.slow");
const ONLINE_SCREEN: paperpaw_sdk::Id = paperpaw_sdk::Id::new_static("providers.online");

/// Many lines from `source`: pages of the panel.
fn long(source: &str) -> String {
    (0..LINES)
        .map(|i| format!("{source} line {i}."))
        .collect::<Vec<_>>()
        .join("\n")
}

struct Blank;

impl paperpaw_sdk::Screen for Blank {
    fn draw(&mut self, _: &mut paperpaw_sdk::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}

    fn on_action(
        &mut self,
        _: &paperpaw_sdk::Id,
        _: &mut paperpaw_sdk::Cx<'_, '_>,
    ) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

impl paperpaw_sdk::Plugin for Providers {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            paperpaw_sdk::Id::new_static("providers"),
            "Providers",
            paperpaw_sdk::SDK_VERSION,
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        use paperpaw_sdk::lookup::{LookupAnswer, LookupProviderSpec};
        r.add_lookup_provider(
            LookupProviderSpec::new(WEB, "providers.web", |_, q| {
                LookupAnswer::Pending(vec![q.answer(&WEB, "Web", long("Web"))])
            })
            .online(),
        )?;
        r.add_lookup_provider(LookupProviderSpec::new(SLOW, "providers.slow", |_, q| {
            LookupAnswer::Pending(vec![q.answer(&SLOW, "Slow", long("Slow"))])
        }))?;
        // One that never answers.
        r.add_lookup_provider(LookupProviderSpec::new(
            paperpaw_sdk::Id::new_static("providers.never"),
            "providers.never",
            |_, _| LookupAnswer::Pending(Vec::new()),
        ))?;
        r.add_screen(paperpaw_sdk::ScreenSpec::new(
            ONLINE_SCREEN,
            "providers.online",
            Box::new(|_, _| Box::new(Blank)),
        ))?;
        r.bind_slot(
            slots::LOOKUP_ONLINE,
            ONLINE_SCREEN,
            paperpaw_sdk::Priority::DEFAULT,
        )
    }
}

impl Lookup {
    /// With the providers, a network, and the dictionaries of `storage`.
    fn with_providers(storage: Card) -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .network(true)
            .user_config("[core]\nlanguage = en\n".to_string())
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_dictionary::DictionaryPlugin)
            .plugin(Providers)
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut lookup = Self {
            kernel,
            now: 0,
            image: Vec::new(),
        };
        lookup.settle(effects);
        lookup
    }

    /// Lets `ms` pass: providers that did not answer are waited for no
    /// more.
    fn pass(&mut self, ms: u64) {
        self.now += ms;
        let effects = self.kernel.tick(Instant(self.now));
        self.settle(effects);
    }

    /// Opens the panel for `word`, without letting time pass.
    fn open(&mut self, word: &str) -> paperpaw_core::Effects {
        self.now += 100;
        let params = Params::new()
            .with(params::WORD, word)
            .with(params::LANGUAGE, "en");
        self.kernel
            .open_slot(Instant(self.now), slots::LOOKUP, params)
    }
}

#[test]
fn a_provider_answering_later_is_shown_and_not_waited_for_forever() {
    let mut lookup = Lookup::with_providers(card());
    lookup.look_up("apple");
    // Waiting for the one that never answers, then no more.
    assert!(lookup.described().iter().any(|l| l.contains("Looking up")));
    lookup.pass(21_000);
    assert!(!lookup.described().iter().any(|l| l.contains("Looking up")));
    // The dictionary's entry and the slow one's: more pages than one
    // entry has.
    let (_, pages) = lookup.page().unwrap();
    let mut alone = Lookup::new(DEVICE, card(), "");
    alone.look_up("apple");
    assert!(pages > alone.page().unwrap().1, "{pages}");
}

#[test]
fn online_answers_given_while_the_dictionaries_are_prepared_stay() {
    // The dictionaries are prepared after the panel shows: online is
    // asked meanwhile.
    let mut asked = Lookup::with_providers(card());
    let effects = asked.open("apple");
    asked.press(actions::MENU);
    asked.settle(effects);
    asked.pass(21_000);
    let with_online = asked.page().unwrap().1;
    let mut offline = Lookup::with_providers(card());
    offline.look_up("apple");
    offline.pass(21_000);
    assert!(with_online > offline.page().unwrap().1, "{with_online}");
}

#[test]
fn once_the_providers_were_asked_online_is_not_offered_again() {
    let mut lookup = Lookup::with_providers(card());
    lookup.look_up("apple");
    lookup.press(actions::MENU);
    assert_eq!(lookup.screen(), "dictionary.popup");
    // Neither while they answer nor after: no second request.
    assert!(!lookup.described().iter().any(|l| l.starts_with("Online")));
    lookup.press(actions::MENU);
    assert_ne!(lookup.screen(), "providers.online");
}
