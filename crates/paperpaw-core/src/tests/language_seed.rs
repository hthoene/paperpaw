//! A language the program carries as a pack (German on the X4, #251): written
//! to the card as a data plugin where it is the language, at a start or when
//! chosen, offered before that, and left alone where the card has the pack.

use super::*;
use paperpaw_platform::buttons;
use paperpaw_sdk::Hint;

const SOURCE: &str = "[meta]\nlanguage = xx\nname = Xx\n[messages]\n\
                      core.hint.core.back = Atrás\ncore.hint.core.confirm = Elegir\n";

static SEED: crate::LanguageSeed = crate::LanguageSeed {
    tag: "xx",
    name: "Xx",
    plural: "one-other",
    plugin: "language-xx",
    plugin_ini: "[plugin]\nid = language-xx\nname = Xx\nversion = 1.0.0\nformats = 4\n",
    catalog: |sink| {
        let compiled = paperpaw_i18n::Catalog::compile(SOURCE).unwrap();
        // In two pieces: written with `write`, then `append`.
        let (head, tail) = compiled.split_at(compiled.len() / 2);
        sink(head) && sink(tail)
    },
};

fn kernel(language: &str, card: paperpaw_sdk::MemoryStorage) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .storage(card)
        .language_seed(&SEED)
        .button_row(&[buttons::BACK, buttons::CONFIRM])
        .user_config(alloc::format!("[core]\nlanguage = {language}\n"))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel
}

fn words(kernel: &Kernel) -> Vec<&str> {
    kernel
        .hint
        .shown
        .iter()
        .filter_map(|hint| match hint {
            Hint::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn offered(kernel: &Kernel) -> bool {
    let mut found = false;
    kernel.shared.translator.each_language(&mut |tag, name| {
        found |= tag == "xx" && name == "Xx";
        core::ops::ControlFlow::Continue(())
    });
    found
}

#[test]
fn a_start_in_the_language_writes_the_pack_and_shows_it() {
    let kernel = kernel("xx", paperpaw_sdk::MemoryStorage::new());
    let storage = &kernel.shared.storage;
    assert!(storage.exists("plugins/language-xx/plugin.ini"));
    assert!(storage.exists("plugins/language-xx/i18n/xx.ppcat"));
    assert!(!storage.exists("plugins/.language-xx.seed"));
    assert_eq!(words(&kernel), ["Elegir"]);
}

#[test]
fn the_language_is_offered_and_chosen_while_running() {
    let mut kernel = kernel("en", paperpaw_sdk::MemoryStorage::new());
    assert!(offered(&kernel));
    assert!(
        !kernel
            .shared
            .storage
            .exists("plugins/language-xx/plugin.ini")
    );
    assert_eq!(words(&kernel), ["Select"]);
    kernel
        .set_setting(Instant(20), settings::LANGUAGE.as_str(), "xx")
        .unwrap();
    assert!(
        kernel
            .shared
            .storage
            .exists("plugins/language-xx/plugin.ini")
    );
    assert_eq!(words(&kernel), ["Elegir"]);
}

#[test]
fn a_pack_on_the_card_is_not_written_again() {
    let catalog = paperpaw_i18n::Catalog::compile(SOURCE).unwrap();
    let card = paperpaw_sdk::MemoryStorage::new()
        .with(
            "plugins/language-xx/plugin.ini",
            b"[plugin]\nid = language-xx\nname = Xx\nversion = 2.10.0\nformats = 4\n".to_vec(),
        )
        .with("plugins/language-xx/i18n/xx.ppcat", catalog);
    let kernel = kernel("xx", card);
    // Its own plugin.ini (version 2.0.0, longer than the program's), not the program's.
    let written = SEED.plugin_ini.len() as u64;
    let on_card = kernel.shared.storage.size("plugins/language-xx/plugin.ini");
    assert_ne!(on_card.unwrap(), written);
    assert_eq!(words(&kernel), ["Elegir"]);
}

#[test]
fn a_pack_that_cannot_be_written_leaves_nothing_and_says_so() {
    struct Full;
    impl paperpaw_platform::Storage for Full {
        fn read_at(
            &self,
            _: &str,
            _: u64,
            _: &mut [u8],
        ) -> Result<usize, paperpaw_platform::StorageError> {
            Err(paperpaw_platform::StorageError::NotFound)
        }
        fn size(&self, _: &str) -> Result<u64, paperpaw_platform::StorageError> {
            Err(paperpaw_platform::StorageError::NotFound)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            Err(paperpaw_platform::StorageError::Unsupported)
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
            Err(paperpaw_platform::StorageError::Unsupported)
        }
        fn list(
            &self,
            _: &str,
            _: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
        ) -> Result<(), paperpaw_platform::StorageError> {
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<(), paperpaw_platform::StorageError> {
            Ok(())
        }
    }
    let mut kernel = Kernel::builder(DEVICE)
        .storage(Full)
        .language_seed(&SEED)
        .user_config("[core]\nlanguage = xx\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains("language-xx"))
    );
}
