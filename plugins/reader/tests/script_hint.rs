//! A book in a script the reading font has no letters of asks for the
//! packs of letters from the store, once for each book: in the language it
//! declares and in its text; what the store could not get is asked for
//! again at the next opening, and a pack the book does not need is not
//! asked for (the rare Hangul syllables).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{BUTTONS, Shared};
use paperpaw_core::Kernel;
use paperpaw_plugin_reader::Reader;
use paperpaw_sdk::builtin::events::PluginsAnswered;
use paperpaw_sdk::builtin::{events, params, slots};
use paperpaw_sdk::{
    Id, Instant, Manifest, Params, Plugin, Priority, RegisterError, Registrar, Storage, Version,
};
use paperpaw_zip::testing::ZipBuilder;

const BOOK_IN: &str = "books/lang.epub";

/// What happened: the notices shown and the plugins asked for (one request
/// each, its ids separated by spaces).
#[derive(Default)]
struct Seen {
    notices: Vec<String>,
    asked: Vec<String>,
}

type Log = Rc<RefCell<Seen>>;

/// What the plugin that installs plugins answers: nothing, or this.
type Answer = Rc<RefCell<Option<PluginsAnswered>>>;

/// A plugin that shows notices as nothing and keeps their texts, and
/// stands in for the store: it keeps what is asked for and answers as told.
struct StoreAndNotices(Log, Answer);

/// A notice, shown as nothing.
struct Notice;

impl paperpaw_sdk::Screen for Notice {
    fn draw(&mut self, _: &mut paperpaw_sdk::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}

    fn on_action(
        &mut self,
        _: &paperpaw_sdk::Id,
        _: &mut paperpaw_sdk::Cx<'_, '_>,
    ) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

impl Plugin for StoreAndNotices {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("notices"), "Notices", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let log = self.0.clone();
        r.add_screen(paperpaw_sdk::ScreenSpec::new(
            Id::new_static("notices.notice"),
            "notice",
            Box::new(move |_, params| {
                log.borrow_mut()
                    .notices
                    .push(params.get(params::TEXT).unwrap_or_default().to_string());
                Box::new(Notice)
            }),
        ))?;
        r.bind_slot(
            slots::NOTICE,
            Id::new_static("notices.notice"),
            Priority::DEFAULT,
        )?;
        let (log, answer) = (self.0.clone(), self.1.clone());
        r.on(events::PLUGINS_REQUESTED, move |_, request, out| {
            assert_eq!(request.by.as_str(), "reader");
            log.borrow_mut().asked.push(request.plugins.clone());
            if let Some(answer) = answer.borrow().clone() {
                out.push(events::PLUGINS_ANSWERED.emit(answer));
            }
        })
    }
}

/// A plugin that is there, as a pack installed is (it does nothing).
struct Present(&'static str);

impl Plugin for Present {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static(self.0), "Pack", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, _: &mut Registrar) -> Result<(), RegisterError> {
        Ok(())
    }
}

/// A one-chapter EPUB with `text`, saying it is in `language` (none if
/// empty).
fn epub_in(language: &str, text: &str) -> Vec<u8> {
    let container = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    let language = match language {
        "" => String::new(),
        tag => format!("<dc:language>{tag}</dc:language>"),
    };
    let opf = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Lang</dc:title>{language}</metadata>
  <manifest><item id="c0" href="c0.xhtml" media-type="application/xhtml+xml"/></manifest>
  <spine><itemref idref="c0"/></spine>
</package>"#
    );
    let page = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>c0</title></head><body><p>{text}</p></body></html>"#
    );
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", container)
        .deflated("OEBPS/content.opf", opf)
        .deflated("OEBPS/c0.xhtml", page)
        .build()
}

/// What a book in `language` with `text` makes happen on opening it over
/// `storage`, with the fonts `fonts` and the plugins `present` there, the
/// store answering `answer`.
fn opened_with(
    (language, text): (&str, &str),
    storage: &Shared,
    fonts: impl Plugin + 'static,
    present: &[&'static str],
    answer: Option<PluginsAnswered>,
) -> Seen {
    storage.create_dir("books").ok();
    storage.write(BOOK_IN, &epub_in(language, text)).unwrap();
    let log = Log::default();
    let mut builder = Kernel::builder(BUTTONS)
        .storage(storage.clone())
        .plugin(fonts)
        .plugin(Reader)
        .plugin(StoreAndNotices(log.clone(), Rc::new(RefCell::new(answer))));
    for id in present {
        builder = builder.plugin(Present(id));
    }
    let mut kernel = builder.build().unwrap();
    kernel.start(Instant(0));
    let mut now = 10;
    let book = Params::new().with(params::PATH, BOOK_IN);
    let mut effects = kernel.open_slot(Instant(now), slots::READER, book);
    for _ in 0..20_000 {
        let Some(at) = effects.wake_at() else {
            break;
        };
        if at.0 > now + 1000 {
            break;
        }
        now = now.max(at.0);
        effects = kernel.tick(Instant(now));
    }
    log.take()
}

/// The plugins asked for on opening a book in `language`, over `storage`,
/// with the fonts of a device of packs (Latin alone).
fn asked(language: &str, storage: &Shared) -> Vec<String> {
    asked_in((language, "Some words."), storage)
}

fn asked_in(book: (&str, &str), storage: &Shared) -> Vec<String> {
    let fonts = || paperpaw_plugin_fonts_default::DefaultFontPacks;
    opened_with(book, storage, fonts(), &[], None).asked
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn a_book_in_greek_asks_for_the_pack_once() {
    let storage = Shared::default();
    assert_eq!(asked("el", &storage), ["script-greek"]);
    // Not again, the next time it is opened.
    assert!(asked("el", &storage).is_empty());
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn each_script_asks_for_its_own_pack() {
    for (language, pack) in [
        ("ru", "script-cyrillic"),
        ("ar", "script-arabic"),
        ("fa", "script-arabic"),
        ("ur", "script-arabic"),
        ("he", "script-hebrew"),
        ("ko", "script-korean"),
    ] {
        assert_eq!(asked(language, &Shared::default()), [pack], "{language}");
    }
}

#[test]
fn books_in_latin_script_or_with_no_language_ask_for_nothing() {
    for language in ["en", "de", "fr", "pt-BR", ""] {
        let asked = asked(language, &Shared::default());
        assert!(asked.is_empty(), "{language}: {asked:?}");
    }
}

/// The text says more than the language: a book that mixes scripts asks
/// for each pack (in one request), also where it declares none.
#[test]
#[ignore = "needs native activation (format removal)"]
fn a_book_asks_for_the_packs_its_text_needs() {
    let mixed = "Words, then Καλημέρα and שלום and مرحبا, and Привет.";
    let storage = Shared::default();
    let mut ids: Vec<String> = asked_in(("en", mixed), &storage)
        .join(" ")
        .split(' ')
        .map(str::to_string)
        .collect();
    ids.sort();
    assert_eq!(
        ids,
        [
            "script-arabic",
            "script-cyrillic",
            "script-greek",
            "script-hebrew"
        ]
    );
    assert!(asked_in(("en", mixed), &storage).is_empty());
    assert_eq!(
        asked_in(("", "Καλημέρα"), &Shared::default()),
        ["script-greek"]
    );
}

/// Fonts that have the letters (the scalable Literata has Greek and
/// Cyrillic, as an installed pack does for its script): nothing is asked
/// for. The script it lacks still is.
#[test]
#[ignore = "needs native activation (format removal)"]
fn a_font_with_the_letters_asks_for_nothing() {
    use paperpaw_plugin_fonts_default::DefaultFonts;
    for language in ["el", "ru"] {
        let storage = Shared::default();
        let seen = opened_with((language, "Some words."), &storage, DefaultFonts, &[], None);
        assert!(seen.asked.is_empty(), "{language}: {:?}", seen.asked);
    }
    let seen = opened_with(
        ("ar", "Some words."),
        &Shared::default(),
        DefaultFonts,
        &[],
        None,
    );
    assert_eq!(seen.asked, ["script-arabic"]);
}

/// What the store says is shown as it is, and a pack it could not get is
/// asked for at the next opening; *Not now* and a pack installed are not
/// asked for again.
#[test]
#[ignore = "needs native activation (format removal)"]
fn what_the_store_could_not_get_is_asked_for_again() {
    let by = Id::new_static("reader");
    let answers = |installed: &str, missing: &str, declined: bool, error: &str| {
        Some(PluginsAnswered::new(
            by.clone(),
            installed.to_string(),
            missing.to_string(),
            declined,
            error.to_string(),
        ))
    };
    let storage = Shared::default();
    let fonts = || paperpaw_plugin_fonts_default::DefaultFontPacks;
    let book = ("el", "Some words.");
    // No connection: said as it is, asked for again.
    let seen = opened_with(
        book,
        &storage,
        fonts(),
        &[],
        answers("", "script-greek", false, "No connection."),
    );
    assert_eq!(seen.asked, ["script-greek"]);
    assert_eq!(seen.notices, ["No connection."]);
    // Again, and this time the reader says *Not now*: nothing is said.
    let declined = answers("", "script-greek", true, "");
    let seen = opened_with(book, &storage, fonts(), &[], declined);
    assert_eq!(seen.asked, ["script-greek"]);
    assert!(seen.notices.is_empty(), "{:?}", seen.notices);
    // Not asked for any more.
    let seen = opened_with(book, &storage, fonts(), &[], None);
    assert!(seen.asked.is_empty());
}

/// The rare Hangul syllables are asked for only where the Korean letters
/// are there already and the book has syllables the fonts still lack.
#[test]
#[ignore = "needs native activation (format removal)"]
fn the_rare_hangul_syllables_are_asked_for_only_when_they_are_needed() {
    let rare = "한국어 똠방각하 뷁";
    // Without the Korean letters, only those are asked for.
    assert_eq!(
        asked_in(("ko", rare), &Shared::default()),
        ["script-korean"]
    );
    // With them (here the fonts still lack every syllable, as a pack that
    // lacked these would), the rest of the syllables the book has.
    let fonts = || paperpaw_plugin_fonts_default::DefaultFontPacks;
    let seen = opened_with(
        ("ko", rare),
        &Shared::default(),
        fonts(),
        &["script-korean"],
        None,
    );
    assert_eq!(seen.asked, ["script-korean-extra"]);
    // A book with no syllable the fonts lack (jamo only) asks for nothing.
    let seen = opened_with(
        ("ko", "ㅋㅋㅋ"),
        &Shared::default(),
        fonts(),
        &["script-korean"],
        None,
    );
    assert!(seen.asked.is_empty(), "{:?}", seen.asked);
}
