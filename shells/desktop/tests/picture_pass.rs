//! A page with pictures in the native reader (#252): its text is shown
//! first, a page turn's refresh; its pictures follow in the next frame,
//! which refreshes only their boxes (`command.redraw_area`) and does not
//! count towards the periodic full refresh (*Full refresh* every n pages).
//! Each frame's time on the emulator is printed (`--nocapture`); the X4's
//! are measured on the device (#250).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;
use std::time::{Duration, Instant as Clock};

use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, profiles};
use paperpaw_platform::{ButtonState, InputEvent, Rect, RefreshMode, buttons};

const BOOK: &str = "books/lighthouse.epub";

/// Pages turned in each test.
const TURNS: usize = 16;
/// *Full refresh* every this many page turns.
const INTERVAL: usize = 3;

/// One frame shown: its region, its refresh, how long the kernel call that
/// drew and showed it took, and the panel after it (RGB rows).
#[derive(Clone)]
struct Frame {
    region: Rect,
    mode: RefreshMode,
    took: Duration,
    panel: Vec<u8>,
    stride: usize,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} {:?} {:?}", self.mode, self.region, self.took)
    }
}

impl Frame {
    /// The frame just shown on `device`.
    fn shown(device: &Device, (region, mode): (Rect, RefreshMode), started: Clock) -> Self {
        Self {
            region,
            mode,
            took: started.elapsed(),
            panel: device.panel.rgb_bytes(),
            stride: usize::from(device.panel.width()),
        }
    }

    fn area(&self) -> u32 {
        u32::from(self.region.width) * u32::from(self.region.height)
    }

    /// Whether every pixel of `rect` on the panel after this frame is the
    /// same: an empty box, no picture in it.
    fn blank(&self, rect: Rect) -> bool {
        let pixel = |x: u16, y: u16| {
            let at = (usize::from(y) * self.stride + usize::from(x)) * 3;
            self.panel.get(at..at + 3)
        };
        let first = pixel(rect.x, rect.y);
        (rect.y..rect.y + rect.height)
            .all(|y| (rect.x..rect.x + rect.width).all(|x| pixel(x, y) == first))
    }
}

/// A small picture (37 × 23): its box is far from half the screen.
const SMALL: (&[u8], &str) = (
    include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png"),
    "image/png",
);
/// A book's cover (about 60 % of the X4's screen once fitted to the page).
const LARGE: (&[u8], &str) = (
    include_bytes!("../../../crates/paperpaw-image/tests/data/alice-cover.jpg"),
    "image/jpeg",
);

/// A book of one long chapter: paragraphs of text with `picture` after every
/// fifteenth (about every fifth page), so that the periodic full refresh
/// comes between pictures.
fn book((picture, kind): (&[u8], &str)) -> Vec<u8> {
    // The first page has a picture too: an opened book shows it after its text.
    let mut body =
        String::from("<h1>The Lighthouse</h1><p><img src=\"fig\" alt=\"The lamp\"/></p>");
    for i in 0..120 {
        body.push_str(&format!(
            "<p>{i}. The keeper climbed the stairs every evening, one hundred and twelve \
             steps, and lit the lamp before the last light of the day had left the water.</p>"
        ));
        if i % 15 == 14 {
            body.push_str("<p><img src=\"fig\" alt=\"The lamp\"/></p>");
        }
    }
    let chapter = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\">\
         <head><title>The Lighthouse</title></head><body>{body}</body></html>"
    );
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
        <package xmlns=\"http://www.idpf.org/2007/opf\" version=\"2.0\" unique-identifier=\"id\">\
        <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>The Lighthouse</dc:title>\
        <dc:identifier id=\"id\">lighthouse-252</dc:identifier><dc:language>en</dc:language></metadata>\
        <manifest><item id=\"c1\" href=\"c1.xhtml\" media-type=\"application/xhtml+xml\"/>\
        <item id=\"fig\" href=\"fig\" media-type=\"{kind}\"/>\
        <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/></manifest>\
        <spine toc=\"ncx\"><itemref idref=\"c1\"/></spine></package>"
    );
    let ncx = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
        <ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\"><navMap>\
        <navPoint id=\"n1\" playOrder=\"1\"><navLabel><text>The Lighthouse</text></navLabel>\
        <content src=\"c1.xhtml\"/></navPoint></navMap></ncx>";
    let container = "<?xml version=\"1.0\"?>\
        <container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
        <rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/>\
        </rootfiles></container>";
    paperpaw_zip::testing::ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated("META-INF/container.xml", container)
        .deflated("OEBPS/content.opf", opf)
        .deflated("OEBPS/toc.ncx", ncx)
        .deflated("OEBPS/c1.xhtml", chapter)
        .stored("OEBPS/fig", picture.to_vec())
        .build()
}

/// The kernel's wake-ups due now, each drawn and shown; what they showed.
fn wake_ups(device: &mut Device, frames: &mut Vec<Frame>) {
    for _ in 0..16 {
        if !device.next_wake().is_some_and(|at| at <= device.now()) {
            return;
        }
        let mut shown = None;
        let started = Clock::now();
        device.call(|kernel, now| {
            let effects = kernel.tick(now);
            shown = effects.refresh();
            effects
        });
        if let Some(shown) = shown {
            frames.push(Frame::shown(device, shown, started));
        }
    }
}

/// Time passing by `millis`, a little at a time, its wake-ups run; what
/// they showed.
fn wait(device: &mut Device, millis: u64) -> Vec<Frame> {
    let mut frames = Vec::new();
    for _ in 0..millis / 50 {
        device.advance(50);
        wake_ups(device, &mut frames);
    }
    frames
}

/// A book with `picture` open on the X4, prepared, *Full refresh* every
/// [`INTERVAL`]; the frames its opening showed.
fn reading(picture: (&[u8], &str)) -> (Device, Vec<Frame>) {
    let files = MemoryStorage::new()
        .with(BOOK, book(picture))
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            ".paperpaw/plugins/reader/state.ini",
            format!("[reader]\nlast = {BOOK}\n"),
        )
        .with(
            "paperpaw.ini",
            format!("[help]\nshow = false\n[core]\nfull-refresh-interval = {INTERVAL}\n"),
        );
    // Refreshes are checked here, not memory (tests/memory.rs does).
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    let opening = wait(&mut device, 10_000);
    let mut screen = None;
    device.call(|kernel, _| {
        screen = kernel.active_screen().map(ToString::to_string);
        paperpaw_core::Effects::default()
    });
    assert_eq!(screen.as_deref(), Some("reader.screen"));
    (device, opening)
}

/// One page turned forward: the frames shown for it, in order.
fn turn(device: &mut Device) -> Vec<Frame> {
    let mut frames = Vec::new();
    for state in [ButtonState::Pressed, ButtonState::Released] {
        let mut shown = None;
        let started = Clock::now();
        device.call(|kernel, now| {
            let effects = kernel.handle(
                now,
                InputEvent::Button {
                    name: buttons::PAGE_FORWARD,
                    state,
                },
            );
            shown = effects.refresh();
            effects
        });
        if let Some(shown) = shown {
            frames.push(Frame::shown(device, shown, started));
        }
        wake_ups(device, &mut frames);
        device.advance(200);
        wake_ups(device, &mut frames);
    }
    // What a page turn leaves to its timers (hints, the clock) is not its.
    wait(device, 2_000);
    frames
}

fn print(what: &str, frames: &[Frame]) {
    let shown: Vec<String> = frames
        .iter()
        .map(|f| {
            format!(
                "{:?} {}x{}@{},{} {} µs",
                f.mode,
                f.region.width,
                f.region.height,
                f.region.x,
                f.region.y,
                f.took.as_micros()
            )
        })
        .collect();
    eprintln!("{what}: {}", shown.join(" | "));
}

/// The screen's area in pixels.
fn whole() -> u32 {
    let screen = profiles::find("xteink-x4").unwrap().device.display;
    u32::from(screen.width) * u32::from(screen.height)
}

/// [`TURNS`] pages turned in a book with `picture`, each turn's frames
/// printed; and the frames of its opening.
fn turns(picture: (&[u8], &str)) -> (Vec<Frame>, Vec<Vec<Frame>>) {
    let (mut device, opening) = reading(picture);
    print("open", &opening);
    let turns: Vec<Vec<Frame>> = (0..TURNS).map(|_| turn(&mut device)).collect();
    for (i, frames) in turns.iter().enumerate() {
        print(&format!("turn {}", i + 1), frames);
    }
    (opening, turns)
}

/// The turns whose page had a picture: a frame more, of the picture's box
/// only (smaller than the screen; not a page where small).
fn pictured(turns: &[Vec<Frame>]) -> Vec<usize> {
    let whole = whole();
    for (i, frames) in turns.iter().enumerate() {
        let first = frames.first().expect("a page turn shows a frame");
        assert!(
            first.area() * 2 >= whole,
            "turn {}: first frame not a page: {frames:?}",
            i + 1
        );
        assert!(frames.len() <= 2, "turn {}: {frames:?}", i + 1);
        if let Some(follow) = frames.get(1) {
            assert!(follow.area() < whole, "turn {}: {follow:?}", i + 1);
            // No band of the text's frame drew the picture: its box was
            // empty after it, and the follow-up filled it.
            assert!(
                first.blank(follow.region),
                "turn {}: the picture was drawn with the text",
                i + 1
            );
            assert!(
                !follow.blank(follow.region),
                "turn {}: the follow-up drew no picture",
                i + 1
            );
        }
    }
    let pictured: Vec<usize> = (0..turns.len()).filter(|&i| turns[i].len() > 1).collect();
    assert!(
        pictured.len() >= 2,
        "pictures drawn a frame after their text on turns {pictured:?} only"
    );
    pictured
}

#[test]
#[ignore = "needs native activation (#243): the reader of this build is the interface-6 one"]
fn small_pictures_follow_their_page_and_do_not_count_as_a_page() {
    let (_, turns) = turns(SMALL);
    let whole = whole();
    for i in pictured(&turns) {
        let follow = &turns[i][1];
        assert_eq!(follow.mode, RefreshMode::Partial, "turn {}", i + 1);
        assert!(follow.area() * 2 < whole, "turn {}: {follow:?}", i + 1);
    }
    // Full refreshes come as without the pictures' frames: every INTERVAL
    // page-sized refreshes, and after a page with pictures (their ghost,
    // `drawing.rs`). Counted here without the pictures' frames; had they
    // counted, the periodic one would come earlier.
    let mut since = 0;
    let mut periodic = 0;
    for (i, frames) in turns.iter().enumerate() {
        // The book's first page has a picture too.
        let leaving = i == 0 || turns[i - 1].len() > 1;
        let due = since + 1 >= INTERVAL;
        let full = frames[0].mode == RefreshMode::Full;
        assert_eq!(
            full,
            leaving || due,
            "turn {}: {} page refreshes since the last full one",
            i + 1,
            since
        );
        periodic += usize::from(full && !leaving);
        since = if full { 0 } else { since + 1 };
    }
    assert!(
        periodic >= 2,
        "the periodic full refresh came {periodic} times"
    );
}

/// A large picture is shown in grays on the X4, which its panel refreshes
/// in full: still only the box, not the screen.
#[test]
#[ignore = "needs native activation (#243): the reader of this build is the interface-6 one"]
fn a_large_picture_refreshes_only_its_box() {
    let (opening, turns) = turns(LARGE);
    pictured(&turns);
    // The first page of the book has the cover's picture: shown text first
    // too, the picture's box after.
    let n = opening.len();
    assert!(n >= 2, "opening: {opening:?}");
    let (page, follow) = (&opening[n - 2], &opening[n - 1]);
    assert!(
        page.area() * 2 >= whole()
            && follow.area() < whole()
            && page.blank(follow.region)
            && !follow.blank(follow.region),
        "opening: {opening:?}"
    );
}
