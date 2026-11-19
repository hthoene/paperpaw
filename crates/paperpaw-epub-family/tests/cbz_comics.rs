//! Comics: the book's `ComicInfo.xml` (title, series, number, writer,
//! language, manga), pages that run right to left, and panel by panel, with
//! hostile `ComicInfo.xml` files and pictures.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_epub_family::{
    CbzDocument, DIRECTION, IMAGE_PART, LEFT_TO_RIGHT, PANELS, RIGHT_TO_LEFT,
};
use paperpaw_sdk::document::{BlockKind, Document, ImageRef, Locator, PageProgression};
use paperpaw_sdk::image::{GrayFile, PngWriter};
use paperpaw_sdk::{MemoryStorage, OpenOptions, Storage};
use paperpaw_zip::testing::ZipBuilder;

const W: u32 = 200;
const H: u32 = 300;

/// A page of white with a gray rectangle per `(x, y, w, h, gray)`, as a PNG.
fn page(rects: &[(u32, u32, u32, u32, u8)], fill: u8) -> Vec<u8> {
    let scratch = MemoryStorage::new();
    let mut png = PngWriter::new(&scratch, "p.png", W, H).unwrap();
    for y in 0..H {
        let mut row = vec![fill; W as usize];
        for &(rx, ry, rw, rh, gray) in rects {
            if (ry..ry + rh).contains(&y) {
                row[rx as usize..(rx + rw) as usize].fill(gray);
            }
        }
        png.row(&scratch, "p.png", &row).unwrap();
    }
    paperpaw_sdk::read_file(&scratch, "p.png", 1 << 20).unwrap()
}

/// Two panels side by side (gray 10 left, 100 right) over one wide one (50).
fn three_panels() -> Vec<u8> {
    page(
        &[
            (10, 10, 85, 130, 10),
            (105, 10, 85, 130, 100),
            (10, 150, 180, 140, 50),
        ],
        255,
    )
}

fn comic(png: Vec<u8>, info: Option<&str>) -> MemoryStorage {
    let mut zip = ZipBuilder::new().stored("1.png", png);
    if let Some(info) = info {
        zip = zip.deflated("ComicInfo.xml", info.as_bytes());
    }
    MemoryStorage::new().with("c.cbz", zip.build())
}

fn options(settings: &[(&str, &str)]) -> OpenOptions {
    OpenOptions::default().settings(
        settings
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
    )
}

fn open(storage: &MemoryStorage, settings: &[(&str, &str)]) -> CbzDocument {
    CbzDocument::open_options(storage, "c.cbz", &options(settings)).unwrap()
}

/// The part of its picture the block at `at` shows (`fmt.image.part`): `x, y,
/// width, height`, or none for a page shown whole.
fn part(doc: &CbzDocument, storage: &MemoryStorage, at: Locator) -> Option<[u32; 4]> {
    let mut params = at.section.to_le_bytes().to_vec();
    params.extend_from_slice(&at.offset.to_le_bytes());
    let bytes = doc.extended(storage, IMAGE_PART, &params)?;
    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    Some(words.try_into().unwrap())
}

/// The gray at the middle of the panel a block shows: a part of the page's
/// own picture, of which no copy is made.
fn middle(doc: &CbzDocument, storage: &MemoryStorage, at: Locator) -> u8 {
    let block = doc.block_at(storage, at).unwrap().unwrap();
    assert_eq!(block.kind, BlockKind::Image);
    let image = block.image.unwrap();
    // The page's file, whole: nothing is made when a page shows the panel.
    assert_eq!((image.width, image.height), (W, H));
    assert_eq!(doc.prepare_image(storage, &image), None);
    let [x, y, w, h] = part(doc, storage, at).unwrap();
    assert!(w > 0 && h > 0 && x + w <= W && y + h <= H);
    let file = GrayFile::open(storage, &image.path).unwrap();
    let mut row = vec![0; W as usize];
    assert!(file.read_row(storage, y + h / 2, &mut row));
    row[(x + w / 2) as usize]
}

const INFO: &str = "<?xml version=\"1.0\"?><ComicInfo><Title>The Start</Title>\
    <Series>Tales</Series><Number>7</Number><Writer>Ann, Bob</Writer>\
    <LanguageISO>ja</LanguageISO><Manga>YesAndRightToLeft</Manga></ComicInfo>";

#[test]
fn comic_info_gives_title_series_number_writer_and_language() {
    let storage = comic(three_panels(), Some(INFO));
    let doc = open(&storage, &[]);
    let m = doc.metadata();
    assert_eq!(m.title.as_deref(), Some("Tales: The Start"));
    assert_eq!(m.series.as_deref(), Some("Tales"));
    assert_eq!(m.series_index.as_deref(), Some("7"));
    assert_eq!(m.authors, ["Ann", "Bob"]);
    assert_eq!(m.language.as_deref(), Some("ja"));
}

#[test]
fn manga_runs_right_to_left_by_comic_info_or_setting() {
    let storage = comic(three_panels(), Some(INFO));
    assert_eq!(
        open(&storage, &[]).metadata().direction,
        PageProgression::RightToLeft
    );
    // The setting wins over the book.
    assert_eq!(
        open(&storage, &[(DIRECTION, LEFT_TO_RIGHT)])
            .metadata()
            .direction,
        PageProgression::LeftToRight
    );
    let plain = comic(
        three_panels(),
        Some("<ComicInfo><Manga>No</Manga></ComicInfo>"),
    );
    assert_eq!(
        open(&plain, &[]).metadata().direction,
        PageProgression::LeftToRight
    );
    assert_eq!(
        open(&plain, &[(DIRECTION, RIGHT_TO_LEFT)])
            .metadata()
            .direction,
        PageProgression::RightToLeft
    );
    let none = comic(three_panels(), None);
    assert_eq!(
        open(
            &none,
            &[("format-cbz.direction", "format-cbz.direction.book")]
        )
        .metadata()
        .direction,
        PageProgression::LeftToRight
    );
}

#[test]
fn panels_are_off_unless_asked_for() {
    let storage = comic(three_panels(), None);
    for settings in [vec![], vec![(PANELS, "false")]] {
        let doc = open(&storage, &settings);
        assert_eq!(doc.section_len(0), 1);
        assert!(
            doc.block_at(&storage, Locator::new(0, 0))
                .unwrap()
                .is_some()
        );
        assert!(
            doc.block_at(&storage, Locator::new(0, 1))
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn panels_step_through_a_page_each_as_a_part_of_the_pages_picture() {
    let storage = comic(three_panels(), None);
    let doc = open(&storage, &[(PANELS, "true")]);
    assert!(doc.whole_pages());
    assert_eq!(doc.sections(), 1);
    // Reading order: the left panel, the right one, the wide one below.
    assert_eq!(middle(&doc, &storage, Locator::new(0, 0)), 10);
    assert_eq!(middle(&doc, &storage, Locator::new(0, 1)), 100);
    assert_eq!(middle(&doc, &storage, Locator::new(0, 2)), 50);
    assert!(
        doc.block_at(&storage, Locator::new(0, 3))
            .unwrap()
            .is_none()
    );
    // Each block is one position: offsets `k` to `k + 1`.
    let second = doc.block_at(&storage, Locator::new(0, 1)).unwrap().unwrap();
    assert_eq!((second.start, second.end), (1, 2));
    // Every panel names the page's one picture (no enlarged copy on the
    // card), each with its own part of it.
    let first = doc.block_at(&storage, Locator::new(0, 0)).unwrap().unwrap();
    let image: ImageRef = second.image.unwrap();
    assert_eq!(first.image.unwrap().path, image.path);
    let parts: Vec<_> = (0..3)
        .map(|k| part(&doc, &storage, Locator::new(0, k)).unwrap())
        .collect();
    assert!(parts[0] != parts[1] && parts[1] != parts[2] && parts[0] != parts[2]);
    assert_eq!(part(&doc, &storage, Locator::new(0, 3)), None);
}

#[test]
fn right_to_left_reads_the_right_panel_of_a_row_first() {
    let storage = comic(three_panels(), None);
    let doc = open(&storage, &[(PANELS, "true"), (DIRECTION, RIGHT_TO_LEFT)]);
    assert_eq!(middle(&doc, &storage, Locator::new(0, 0)), 100);
    assert_eq!(middle(&doc, &storage, Locator::new(0, 1)), 10);
    assert_eq!(middle(&doc, &storage, Locator::new(0, 2)), 50);
}

#[test]
fn a_page_without_panels_is_shown_whole() {
    for png in [
        page(&[], 255),
        page(&[], 100),
        // One panel is no panels.
        page(&[(10, 10, 180, 280, 30)], 255),
    ] {
        let storage = comic(png, None);
        let doc = open(&storage, &[(PANELS, "true")]);
        let whole = doc.block_at(&storage, Locator::new(0, 0)).unwrap().unwrap();
        assert_eq!(whole.kind, BlockKind::Image);
        assert!(
            doc.block_at(&storage, Locator::new(0, 1))
                .unwrap()
                .is_none()
        );
        // The page itself, not a panel of it.
        assert!(whole.image.is_some());
        assert_eq!(part(&doc, &storage, Locator::new(0, 0)), None);
    }
}

#[test]
fn the_panels_are_kept_and_the_position_stays_after_reopening() {
    let storage = comic(three_panels(), None);
    {
        let doc = open(&storage, &[(PANELS, "true")]);
        assert_eq!(middle(&doc, &storage, Locator::new(0, 1)), 100);
    }
    let again = open(&storage, &[(PANELS, "true")]);
    let block = again
        .block_at(&storage, Locator::new(0, 1))
        .unwrap()
        .unwrap();
    assert_eq!((block.start, block.end), (1, 2));
    assert_eq!(middle(&again, &storage, Locator::new(0, 2)), 50);
}

#[test]
fn hostile_comic_info_is_bounded_and_never_panics() {
    let deep = format!(
        "<ComicInfo>{}<Title>T</Title>{}</ComicInfo>",
        "<a>".repeat(200_000),
        "</a>".repeat(200_000)
    );
    let long = format!(
        "<ComicInfo><Title>{}</Title><Series>S</Series></ComicInfo>",
        "é".repeat(500_000)
    );
    let entities = format!(
        "<ComicInfo><Writer>{}</Writer><Manga>yesandrighttoleft</Manga></ComicInfo>",
        "&amp;&#0;&#x110000;&bogus;".repeat(20_000)
    );
    let many = format!(
        "<ComicInfo>{}</ComicInfo>",
        "<Genre>a,b,c</Genre>".repeat(50_000)
    );
    let unterminated = format!("<ComicInfo><Title>{}", "x".repeat(300_000));
    let binary: String = (0..4000).map(|i| char::from((i % 251) as u8)).collect();
    for info in [
        deep.as_str(),
        &long,
        &entities,
        &many,
        &unterminated,
        &binary,
        "<ComicInfo><Title",
        "<![CDATA[",
        "<ComicInfo><Title><![CDATA[x</Title></ComicInfo>",
    ] {
        let storage = comic(three_panels(), Some(info));
        let doc = open(&storage, &[(PANELS, "true")]);
        let m = doc.metadata();
        // What is kept is bounded.
        assert!(m.title.as_ref().is_none_or(|t| t.len() < 16 * 1024));
        assert!(m.authors.iter().all(|a| a.len() < 16 * 1024));
        assert!(m.subjects.len() <= 100_000);
        assert!(
            doc.block_at(&storage, Locator::new(0, 0))
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn hostile_pictures_are_no_panels_and_never_panic() {
    // Not pictures, cut short, huge claims, noise.
    let noise: Vec<u8> = (0..60_000_u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    let mut png = three_panels();
    let cut = png.len() / 3;
    png.truncate(cut);
    for data in [b"not a picture".to_vec(), png, noise, Vec::new()] {
        let storage =
            MemoryStorage::new().with("c.cbz", ZipBuilder::new().stored("1.png", data).build());
        let doc = open(&storage, &[(PANELS, "true")]);
        let block = doc.block_at(&storage, Locator::new(0, 0)).unwrap().unwrap();
        assert!(matches!(
            block.kind,
            BlockKind::Paragraph | BlockKind::Image
        ));
        assert!(
            doc.block_at(&storage, Locator::new(0, 1))
                .unwrap()
                .is_none()
        );
    }
    // A part asked for where there is none is none: a page or a panel the
    // book does not have, parameters that are not two numbers, panels off.
    let storage = comic(three_panels(), None);
    let doc = open(&storage, &[(PANELS, "true")]);
    // A block is read before its part is asked for: the page is decoded then.
    assert!(
        doc.block_at(&storage, Locator::new(0, 0))
            .unwrap()
            .is_some()
    );
    assert!(part(&doc, &storage, Locator::new(0, 0)).is_some());
    assert_eq!(part(&doc, &storage, Locator::new(9, 0)), None);
    assert_eq!(part(&doc, &storage, Locator::new(0, 99)), None);
    assert_eq!(doc.extended(&storage, IMAGE_PART, &[0; 7]), None);
    let whole = open(&storage, &[]);
    assert_eq!(part(&whole, &storage, Locator::new(0, 0)), None);
    let _ = storage.size("c.cbz");
}
