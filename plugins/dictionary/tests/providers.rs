//! The panel lists what lookup providers find: an offline provider's
//! entry with the word, an online provider's only when *Online* is
//! chosen, shown when it answers later; providers of other languages are
//! not asked.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::lookup::{LookupAnswer, LookupEntry, LookupProviderSpec};
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, Manifest, MemoryClass, MemoryStorage, Params,
    PixelFormat, Plugin, RegisterError, Registrar, SDK_VERSION,
};

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

const WEB: Id = Id::new_static("glossary.web");

/// An offline glossary of English words, an online one, and a French one
/// that counts how often it is asked.
struct Glossary(Rc<Cell<u32>>);

impl Plugin for Glossary {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("glossary"), "Glossary", SDK_VERSION)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_lookup_provider(
            LookupProviderSpec::new(Id::new_static("glossary.book"), "glossary.book", |_, q| {
                LookupAnswer::Ready(vec![LookupEntry::new("Glossary", long(&q.word))])
            })
            .languages(&["en"]),
        )?;
        r.add_lookup_provider(
            LookupProviderSpec::new(WEB, "glossary.web", |_, q| {
                // Its answer comes as an event, as an HTTP reply's would.
                LookupAnswer::Pending(vec![q.answer(&WEB, "Web", long("online"))])
            })
            .online(),
        )?;
        let asked = self.0.clone();
        r.add_lookup_provider(
            LookupProviderSpec::new(Id::new_static("glossary.fr"), "glossary.fr", move |_, _| {
                asked.set(asked.get() + 1);
                LookupAnswer::Ready(vec![LookupEntry::new("French", "Le glossaire.")])
            })
            .languages(&["fr"]),
        )
    }
}

/// An entry of many lines about `word`: pages of the panel.
fn long(word: &str) -> String {
    (0..30)
        .map(|i| format!("Line {i} on {word}."))
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

struct Lookup {
    kernel: Kernel,
    now: u64,
}

impl Lookup {
    fn new(network: bool, french: Rc<Cell<u32>>) -> Self {
        let mut kernel = Kernel::builder(DEVICE)
            .storage(MemoryStorage::new())
            .network(network)
            .user_config("[core]\nlanguage = en\n")
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_dictionary::DictionaryPlugin)
            .plugin(Glossary(french))
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut lookup = Self { kernel, now: 0 };
        lookup.settle(effects);
        lookup
    }

    fn settle(&mut self, mut effects: paperpaw_core::Effects) {
        for _ in 0..10_000 {
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

    /// What the panel's header says.
    fn shown(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }

    /// The pages of the entries shown (0 for one page).
    fn pages(&mut self) -> usize {
        self.shown()
            .iter()
            .find_map(|label| label.split_once(" / ")?.1.trim().parse().ok())
            .unwrap_or(0)
    }
}

#[test]
fn offline_providers_answer_with_the_word() {
    let french = Rc::new(Cell::new(0));
    let mut lookup = Lookup::new(false, french.clone());
    lookup.look_up("rabbit");
    // The glossary's long entry, without a network no way online.
    assert!(lookup.pages() > 1, "{:?}", lookup.shown());
    assert!(!lookup.shown().iter().any(|l| l.contains("Online")));
    // The French glossary is not asked about an English word.
    assert_eq!(french.get(), 0);
}

#[test]
fn online_providers_answer_when_asked() {
    let mut lookup = Lookup::new(true, Rc::default());
    lookup.look_up("rabbit");
    let offline = lookup.pages();
    assert!(lookup.shown().iter().any(|l| l.contains("Online")));
    lookup.press(actions::MENU);
    // The online entry came, after the glossary's.
    assert!(lookup.pages() > offline, "{:?}", lookup.shown());
    assert!(!lookup.shown().iter().any(|l| l.contains("Online")));
}
