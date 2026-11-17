//! The Library's first frame after the book walk (#297): on the X4 it took 8.3 s
//! in host slots and 11,760 card blocks. This counts the host slots of the
//! open with the kernel's `slot-counts` feature (the emulator counts like the
//! device; its milliseconds are not the device's, its call counts are).
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::rc::Rc;

use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, headless::run_script, profiles};
use paperpaw_zip::testing::ZipBuilder;

const COVER: &[u8] = include_bytes!("fixtures/cover.jpg");
const BOOKS: usize = 100;

/// A book with a title, an author, a cover and one chapter; each one differs.
fn book(n: usize) -> Vec<u8> {
    let opf = format!(
        "<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"2.0\" \
         unique-identifier=\"id\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
         <dc:title>Alice's Adventures in Wonderland and the Looking-Glass, number {n}</dc:title><dc:creator>Lewis Carroll and John Tenniel, series {}</dc:creator>\
         <dc:language>en</dc:language><dc:identifier id=\"id\">book-{n}</dc:identifier>\
         <meta name=\"cover\" content=\"cover\"/></metadata><manifest>\
         <item id=\"c1\" href=\"c1.xhtml\" media-type=\"application/xhtml+xml\"/>\
         <item id=\"cover\" href=\"cover.jpg\" media-type=\"image/jpeg\"/></manifest>\
         <spine><itemref idref=\"c1\"/></spine></package>",
        n % 17
    );
    ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated(
            "META-INF/container.xml",
            "<?xml version=\"1.0\"?><container version=\"1.0\" \
             xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles>\
             <rootfile full-path=\"content.opf\" media-type=\"application/oebps-package+xml\"/>\
             </rootfiles></container>",
        )
        .deflated("content.opf", opf)
        .deflated(
            "c1.xhtml",
            format!("<html><body><p>Book {n}.</p></body></html>"),
        )
        .stored("cover.jpg", COVER)
        .build()
}

#[test]
fn the_first_frame_of_the_library_after_the_walk_stays_within_its_host_crossings() {
    let mut files = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            "paperpaw.ini",
            "[help]\nshow = false\n[reader]\nopen-last = false\n",
        );
    for n in 0..BOOKS {
        files = files.with(&format!("books/b{n:03}.epub"), book(n));
    }
    paperpaw_emulator::budget::override_limit(None);
    let mut device = Device::in_memory(
        profiles::find("xteink-x4").unwrap(),
        None,
        Rc::new(files),
        true,
        None,
    )
    .unwrap();
    device.start();
    run_script(&mut device, "wait:30000").unwrap();
    let before = paperpaw_emulator::trace::counts();
    slot_counts::reset();
    device.shown.clear();
    run_script(&mut device, "open:files.browser").unwrap();
    run_script(&mut device, "wait:300").unwrap();
    let io = paperpaw_emulator::trace::counts().since(&before);
    let counts = slot_counts::snapshot();
    let total: u32 = counts.iter().map(|(_, n)| *n).sum();
    eprintln!("library: total {total} {counts:?}");
    eprintln!("library: total {total} {counts:?}; storage {io}");
    assert!(!device.shown.is_empty(), "the screen drew nothing");
    let measures = counts
        .iter()
        .find(|(slot, _)| *slot == "text_measure")
        .map_or(0, |(_, n)| *n);
    // Measured, 100 books of long titles: 2,600 slots with 1,845 measures (a
    // text too long for its room was cut one letter at a time, and every band
    // laid out every row of the page); 1,340 and 585 with the cut found by
    // halving; 796 and 308 with only the rows of the band laid out.
    assert!(measures <= 400, "{measures} text measures: {counts:?}");
    assert!(total <= 1000, "{total} host slots: {counts:?}");
    // The books off the page are not read one by one (0 measured).
    assert!(io.reads + io.sizes <= 40, "{io}: {counts:?}");
}
