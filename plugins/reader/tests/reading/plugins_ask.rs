//! Plugins that ask the reader for the words of the page, that fill its
//! contents slot, and that filter its blocks.

use super::*;

/// The page words a plugin asked the reader for.
type WordsAsked = Rc<RefCell<Option<paperpaw_sdk::reading::PageWords>>>;

const WORDS_HEARD: paperpaw_sdk::event::Event<paperpaw_sdk::reading::PageWords> =
    paperpaw_sdk::event::Event::new("asker.words");

/// A plugin whose action `asker.ask` asks for the page's words.
struct Asker(WordsAsked);

impl Plugin for Asker {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("asker"), "Asker", Version::new(0, 1, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(
            paperpaw_sdk::ActionSpec::new(Id::new_static("asker.ask"), "ask").handler(
                |_: &paperpaw_sdk::Env<'_>, _, out: &mut paperpaw_sdk::Commands| {
                    out.push(paperpaw_sdk::reading::ask_page_words(&WORDS_HEARD));
                },
            ),
        )?;
        let asked = self.0.clone();
        r.on(WORDS_HEARD, move |_, words, _| {
            *asked.borrow_mut() = Some(words.clone());
        })
    }
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn any_plugin_reads_the_words_of_the_page() {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    let text = include_str!("../../../../samples/library/books/alice-in-wonderland.txt");
    storage.write(BOOK, text.as_bytes()).unwrap();
    let asked = WordsAsked::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Reader)
        .plugin(Asker(asked.clone()))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage,
        heard: Heard::default(),
        now: 0,
        book: BOOK,
    };
    reading.settle(effects);
    // No book open: no answer.
    reading.run(Id::new_static("asker.ask"));
    assert!(asked.borrow().is_none());
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    reading.run(Id::new_static("asker.ask"));
    let words = asked.borrow().clone().expect("the reader answered");
    assert_eq!(words.path, BOOK);
    assert!(words.words.len() > 20, "{}", words.words.len());
    let kept = reading.kept();
    assert_eq!(
        words.start,
        paperpaw_sdk::document::Locator::new(kept.0, kept.1)
    );
    // Each word is the book's text at its place.
    for word in &words.words[..10] {
        let from = word.start.offset as usize;
        assert!(
            text[from..].starts_with(word.text.trim_end_matches('-')),
            "{word:?}"
        );
    }
}

#[test]
fn a_place_kept_by_the_reader_before_is_read() {
    let storage = Shared::default();
    let mut first = Reading::new(storage.clone());
    first.open();
    for _ in 0..3 {
        first.run(core_actions::NEXT_PAGE);
    }
    let left = first.kept();
    // Kept where the reader kept it before positions were shared.
    storage
        .remove(&paperpaw_sdk::positions::file(BOOK))
        .unwrap();
    let dir = format!(
        ".paperpaw/plugins/reader/books/{:016x}",
        paperpaw_sdk::digest::fnv1a64(BOOK)
    );
    let old = format!("[progress]\nsection = {}\noffset = {}\n", left.0, left.1);
    storage
        .write(&format!("{dir}/progress.ini"), old.as_bytes())
        .unwrap();
    let mut again = Reading::new(storage);
    again.open();
    reading_runs_nothing(&mut again);
    assert_eq!(again.kept(), left);
}

/// Lets a moment pass without input.
fn reading_runs_nothing(reading: &mut Reading) {
    reading.now += 10;
    let effects = reading.kernel.tick(Instant(reading.now));
    reading.settle(effects);
}

#[test]
fn a_book_closed_says_where_as_an_xpointer() {
    let epub = include_bytes!("../../../../samples/library/books/alice-in-wonderland.epub");
    let mut reading = Reading::with_book(Shared::default(), EPUB, epub);
    reading.open();
    for _ in 0..5 {
        reading.run(core_actions::NEXT_PAGE);
    }
    reading.run(core_actions::BACK);
    let heard = reading.heard.borrow();
    let (name, closed) = heard.last().unwrap();
    assert_eq!(*name, "closed");
    let xpointer = closed.xpointer.clone().unwrap_or_default();
    assert!(xpointer.starts_with("/body/DocFragment["), "{xpointer:?}");
}

/// The params a screen of one's own was opened with.
type OpenedWith = Rc<RefCell<Option<Params>>>;

/// A plugin whose contents take the place of the reader's.
struct OwnContents(OpenedWith);

/// A screen that shows nothing.
struct Blank;

impl paperpaw_sdk::Screen for Blank {
    fn draw(&mut self, _: &mut paperpaw_sdk::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
        paperpaw_sdk::Handled::No
    }
}

impl Plugin for OwnContents {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("contents"),
            "Contents",
            Version::new(0, 1, 0),
        )
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let opened = self.0.clone();
        r.add_screen(paperpaw_sdk::ScreenSpec::new(
            Id::new_static("contents.list"),
            "contents",
            Box::new(move |_, params| {
                *opened.borrow_mut() = Some(params.clone());
                Box::new(Blank)
            }),
        ))?;
        r.bind_slot(
            slots::TOC,
            Id::new_static("contents.list"),
            paperpaw_sdk::Priority::PREFERRED,
        )
    }
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn the_contents_are_a_slot_another_plugin_fills() {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    let text = include_str!("../../../../samples/library/books/alice-in-wonderland.txt");
    storage.write(BOOK, text.as_bytes()).unwrap();
    let opened = OpenedWith::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Reader)
        .plugin(OwnContents(opened.clone()))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage,
        heard: Heard::default(),
        now: 0,
        book: BOOK,
    };
    reading.settle(effects);
    reading.open();
    reading.run(core_actions::NEXT_PAGE);
    let at = reading.kept();
    reading.run(actions::TOC);
    assert_eq!(
        reading.kernel.active_screen().map(Id::as_str),
        Some("contents.list")
    );
    let params = opened.borrow().clone().expect("opened");
    assert_eq!(params.get(params::PATH), Some(BOOK));
    assert_eq!(params.get(params::OFFSET), Some(at.1.to_string().as_str()));
}

/// The books a block filter was told of.
type Told = Rc<RefCell<Vec<String>>>;

/// A plugin that leaves out every paragraph naming the Rabbit.
struct NoRabbit(Told);

impl Plugin for NoRabbit {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("norabbit"),
            "No rabbit",
            Version::new(0, 1, 0),
        )
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        use paperpaw_sdk::filters::{TextFilterSpec, Verdict};
        let told = self.0.clone();
        r.add_text_filter(TextFilterSpec::block(
            Id::new_static("norabbit.filter"),
            "no rabbit",
            Box::new(move |_, cx, block| {
                let mut told = told.borrow_mut();
                if !told.iter().any(|t| t == cx.path) {
                    told.push(cx.path.to_string());
                }
                match block.text.contains("Rabbit") {
                    true => Verdict::Drop,
                    false => Verdict::Keep,
                }
            }),
            Box::new(|_| 1),
        ))
    }
}

#[test]
#[ignore = "needs native activation (format removal)"]
fn a_block_filter_leaves_paragraphs_out() {
    let storage = Shared::default();
    storage.create_dir("books").unwrap();
    let text = include_str!("../../../../samples/library/books/alice-in-wonderland.txt");
    storage.write(BOOK, text.as_bytes()).unwrap();
    let told = Told::default();
    let asked = WordsAsked::default();
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Reader)
        .plugin(NoRabbit(told.clone()))
        .plugin(Asker(asked.clone()))
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut reading = Reading {
        kernel,
        storage,
        heard: Heard::default(),
        now: 0,
        book: BOOK,
    };
    reading.settle(effects);
    reading.open();
    for _ in 0..8 {
        reading.run(core_actions::NEXT_PAGE);
        reading.run(Id::new_static("asker.ask"));
        let words = asked.borrow().clone().expect("the reader answered");
        assert!(!words.text().contains("Rabbit"), "{}", words.text());
    }
    assert_eq!(*told.borrow(), [BOOK.to_string()]);
}
