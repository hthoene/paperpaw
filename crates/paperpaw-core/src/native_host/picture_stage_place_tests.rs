//! The stage is asked for every picture with an image id, with its place:
//! a book's page, a comic's page, a cover, a small picture, the sleep
//! screen's, a plugin's own.
use super::*;
use crate::native_host::stages_wire::core_format_v1::FmtImage;

/// A screen of 31 pixels: a picture of 8 by 4 is no small one on it.
const SCREEN: Shown = Shown {
    whole_pages: false,
    sleeping: false,
    long_side: 31,
};

/// The file `c/<name>` of 8 by 4, every pixel gray 100.
fn file(storage: &MemoryStorage, name: &str) {
    let path = std::format!("c/{name}");
    let mut writer = GrayWriter::new(storage, &path);
    writer.start(8, 4).unwrap();
    for _ in 0..4 {
        writer.row(&[100; 8]).unwrap();
    }
    writer.finish().unwrap();
}

/// The picture `index` of document 7 (the file `c/<name>`, or `part` of it)
/// as an image of `A`: its id.
fn mapped(t: &ImageTable, storage: &MemoryStorage, name: &str, index: u32, part: &[u8]) -> u32 {
    let image = FmtImage::new(name, index, 8, 4, 0, part);
    t.map_fmt_image(storage, A, 7, "c", &image, true, true)
        .unwrap()
        .unwrap()
        .image
}

fn one(answer: Result<Vec<u8>, i32>) -> Fake {
    Fake {
        answers: std::vec![answer],
        asked: Vec::new(),
    }
}

/// The place and the path the only subscriber was asked with.
fn asked(fake: &Fake) -> (u32, std::string::String) {
    let mut rows = stage::input_decode(&fake.asked[0].1).unwrap();
    let Some(Ok(stage::Record::PictureAsk(asked))) = rows.next() else {
        panic!("no picture.ask");
    };
    (asked.place, asked.path.into())
}

#[test]
fn a_picture_of_a_books_page_is_asked_for_with_its_book_and_drawn_with_the_answer() {
    let storage = MemoryStorage::new();
    file(&storage, "a.gray");
    let t = ImageTable::new();
    let id = mapped(&t, &storage, "a.gray", 6, &[]);
    assert!(t.tone_pending());
    let wanted = t.tone_wanted().unwrap();
    assert_eq!((wanted.id, wanted.document), (id, Some((7, 6))));
    // A page's picture is a page's, however small and whoever shows it.
    let small = Shown {
        sleeping: true,
        long_side: 800,
        ..SCREEN
    };
    assert_eq!(wanted.place(small), tone::PLACE_READER);
    let place = wanted.place(SCREEN);
    let mut fake = one(tone(&darker(), 0));
    let told = (place, "books/a.epub");
    assert!(!ask(
        &mut fake,
        &[subscriber(7)],
        &storage,
        &t,
        &wanted,
        told
    ));
    assert_eq!(asked(&fake), (0, "books/a.epub".into()));
    assert_eq!(shown(&t, &storage, id), 50);
    // Asked once: the same picture handed out again waits for nothing.
    assert_eq!(mapped(&t, &storage, "a.gray", 6, &[]), id);
    assert!(t.tone_wanted().is_none() && !t.tone_pending());
}

#[test]
fn a_comics_page_is_asked_for_once_and_its_panels_take_the_pages_answer() {
    let storage = MemoryStorage::new();
    file(&storage, "p.gray");
    let t = ImageTable::new();
    let words = |part: [u32; 4]| -> Vec<u8> { part.iter().flat_map(|v| v.to_le_bytes()).collect() };
    let left = mapped(&t, &storage, "p.gray", 2, &words([0, 0, 4, 4]));
    let wanted = t.tone_wanted().unwrap();
    let comic = Shown {
        whole_pages: true,
        ..SCREEN
    };
    assert_eq!(wanted.place(comic), tone::PLACE_COMIC);
    assert_eq!((wanted.width, wanted.height, wanted.part), (4, 4, true));
    let mut fake = one(tone(&darker(), 0));
    let told = (wanted.place(comic), "comics/b.cbz");
    ask(&mut fake, &[subscriber(7)], &storage, &t, &wanted, told);
    assert_eq!(asked(&fake), (1, "comics/b.cbz".into()));
    // The next panel of the page: nobody is asked, nothing is read.
    let right = mapped(&t, &storage, "p.gray", 2, &words([4, 0, 4, 4]));
    assert_ne!(left, right);
    let wanted = t.tone_wanted().unwrap();
    let mut fake = one(tone(&[], 0));
    ask(&mut fake, &[subscriber(7)], &storage, &t, &wanted, told);
    assert!(fake.asked.is_empty());
    for id in [left, right] {
        let mut fb = Framebuffer::new(8, 8, PixelFormat::Gray8).unwrap();
        t.draw(A, id, &mut fb.canvas(), &storage, 0, 0, 4, 4, 0)
            .unwrap();
        assert_eq!(fb.canvas().gray_at(Point::new(2, 2)).unwrap(), 50);
    }
}

#[test]
fn a_cover_is_asked_for_as_a_cover_and_a_small_one_as_a_thumbnail() {
    let storage = MemoryStorage::new();
    file(&storage, "cover.gray");
    let t = ImageTable::new();
    // The cover of an open document, then the library's own (`adopt_cover`).
    let id = mapped(&t, &storage, "cover.gray", u32::MAX, &[]);
    let id = t.adopt_cover(A, id).unwrap();
    let wanted = t.tone_wanted().unwrap();
    assert_eq!((wanted.id, wanted.document), (id, Some((0, u32::MAX))));
    assert_eq!(wanted.place(SCREEN), tone::PLACE_COVER);
    // At most a quarter of the screen's long side: a small picture.
    let wide = Shown {
        long_side: 32,
        ..SCREEN
    };
    assert_eq!(wanted.place(wide), tone::PLACE_THUMBNAIL);
    let mut fake = one(tone(&darker(), 0));
    let told = (wanted.place(SCREEN), "");
    ask(&mut fake, &[subscriber(7)], &storage, &t, &wanted, told);
    assert_eq!(asked(&fake), (2, "".into()));
    assert_eq!(shown(&t, &storage, id), 50);
    // A plugin's own picture: small, a thumbnail; else its own.
    let (t, wanted) = prepared(&storage);
    assert_eq!(wanted.document, None);
    assert_eq!(wanted.place(SCREEN), tone::PLACE_UI);
    assert_eq!(wanted.place(wide), tone::PLACE_THUMBNAIL);
    drop(t);
}

#[test]
fn the_pictures_of_the_sleep_screens_plugin_are_asked_for_as_the_sleep_screens() {
    let storage = MemoryStorage::new();
    file(&storage, "cover.gray");
    let t = ImageTable::new();
    let cover = mapped(&t, &storage, "cover.gray", u32::MAX, &[]);
    let asleep = Shown {
        sleeping: true,
        long_side: 800,
        ..SCREEN
    };
    // The last book's cover on the sleep screen, small or not.
    let wanted = t.tone_wanted().unwrap();
    assert_eq!(wanted.id, cover);
    assert_eq!(wanted.place(asleep), tone::PLACE_SLEEP);
    let mut fake = one(tone(&darker(), 0));
    let told = (wanted.place(asleep), "");
    ask(&mut fake, &[subscriber(7)], &storage, &t, &wanted, told);
    assert_eq!(asked(&fake).0, 4);
    assert_eq!(shown(&t, &storage, cover), 50);
    // And the picture the sleep plugin prepared for it.
    let (_t, wanted) = prepared(&storage);
    assert_eq!(wanted.place(asleep), tone::PLACE_SLEEP);
}

#[test]
fn a_picture_drawn_before_its_answer_is_to_be_drawn_again() {
    let storage = MemoryStorage::new();
    file(&storage, "a.gray");
    let t = ImageTable::new();
    // Its id was made and drawn in one frame: the stage is asked after.
    let id = mapped(&t, &storage, "a.gray", 6, &[]);
    assert_eq!(shown(&t, &storage, id), 100);
    let wanted = t.tone_wanted().unwrap();
    assert!(wanted.drawn);
    let told = (tone::PLACE_READER, "");
    let mut fake = one(tone(&darker(), 0));
    assert!(ask(
        &mut fake,
        &[subscriber(7)],
        &storage,
        &t,
        &wanted,
        told
    ));
    assert_eq!(shown(&t, &storage, id), 50);
    // An answer that says nothing changes nothing: no draw for it.
    file(&storage, "b.gray");
    let id = mapped(&t, &storage, "b.gray", 7, &[]);
    assert_eq!(shown(&t, &storage, id), 100);
    let wanted = t.tone_wanted().unwrap();
    let mut fake = one(tone(&[], 0));
    assert!(!ask(
        &mut fake,
        &[subscriber(7)],
        &storage,
        &t,
        &wanted,
        told
    ));
    // A curve that changes no gray is kept as none.
    let same: Vec<u8> = (0..=255).collect();
    assert!(t.tone_answer(A, id, &same, 0));
    assert_eq!(shown(&t, &storage, id), 100);
}

#[test]
fn without_a_subscriber_the_declarations_are_searched_once() {
    let t = ImageTable::new();
    assert!(!t.tones_unsubscribed_at(3));
    t.tones_unsubscribed(3);
    assert!(t.tones_unsubscribed_at(3));
    // Declarations that changed are searched again.
    assert!(!t.tones_unsubscribed_at(4));
}
