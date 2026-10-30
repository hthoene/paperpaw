//! Damaged or hostile fonts and packs never make the crate ask for more
//! memory than the font really holds: on the X4 a failed allocation stops
//! the device. Each test measures the largest single allocation it causes.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;

use paperpaw_text::{LineMetrics, PackBuilder, PackSource, StoredPack};

/// The system allocator, noting each thread's largest request.
struct Tracking;

thread_local! {
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

fn note(size: usize) {
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
}

unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Tracking = Tracking;

/// The largest allocation `f` makes.
fn largest_allocation<T>(f: impl FnOnce() -> T) -> (T, usize) {
    LARGEST.with(|largest| largest.set(0));
    let result = f();
    (result, LARGEST.with(Cell::get))
}

/// What the tests allow at most: far less than the gigabytes asked for.
const BOUND: usize = 64 * 1024;

/// A pack in memory.
struct Memory(Vec<u8>);

impl PackSource for Memory {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        let Ok(at) = usize::try_from(at) else {
            return false;
        };
        match self.0.get(at..).and_then(|rest| rest.get(..buf.len())) {
            Some(bytes) => {
                buf.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
}

/// A pack of one strike at 10 px holding a space.
fn pack() -> Vec<u8> {
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    let mut builder = PackBuilder::new();
    builder.add_strike(10.0, metrics, vec![(' ', paperpaw_text::Glyph::empty(3.0))]);
    builder.build()
}

fn u32_le(data: &[u8], at: usize) -> usize {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize
}

#[test]
fn a_stored_glyph_of_absurd_size_is_not_allocated() {
    let mut data = pack();
    // The space's record: as wide and as high as a record can say.
    let record = u32_le(&data, 12 + 20);
    data[record + 12..record + 16].copy_from_slice(&[0xff; 4]);
    let pack = StoredPack::open(Rc::new(Memory(data))).unwrap();
    let (glyph, largest) = largest_allocation(|| pack.render(' ', 10.0));
    assert!(largest <= BOUND, "{largest} bytes");
    assert_eq!((glyph.width, glyph.height), (0, 0));
    assert_eq!(glyph.advance, 3.0);
}

#[test]
fn a_pack_claiming_many_strikes_is_refused_cheaply() {
    let mut data = pack();
    data[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
    let (pack, largest) = largest_allocation(|| StoredPack::open(Rc::new(Memory(data))));
    assert!(pack.is_err());
    assert!(largest <= BOUND, "{largest} bytes");
}

#[test]
fn a_font_directory_is_not_allocated_from_its_header() {
    // A TrueType header claiming 65535 tables, and nothing after it.
    let mut data = vec![0, 1, 0, 0, 0xff, 0xff];
    data.resize(12, 0);
    let (info, largest) = largest_allocation(|| {
        paperpaw_text::sfnt::read_info(&mut |offset, buf: &mut [u8]| {
            let start = (offset as usize).min(data.len());
            let n = buf.len().min(data.len() - start);
            buf[..n].copy_from_slice(&data[start..start + n]);
            n
        })
    });
    assert_eq!(info, None);
    assert!(largest <= BOUND, "{largest} bytes");
}

/// A real font whose em is made tiny (16 units), so that its outlines,
/// scaled to it, reach far beyond any glyph a page shows.
#[cfg(feature = "outline")]
#[test]
fn an_outline_glyph_bitmap_is_bounded() {
    use paperpaw_text::OutlineFont;

    let mut data = include_bytes!("../../../plugins/fonts-default/fonts/SourceSans3.ttf").to_vec();
    let tables = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let head = (0..tables)
        .map(|i| 12 + i * 16)
        .find(|&record| &data[record..record + 4] == b"head")
        .map(|record| u32::from_be_bytes(data[record + 8..record + 12].try_into().unwrap()))
        .unwrap() as usize;
    data[head + 18..head + 20].copy_from_slice(&16u16.to_be_bytes());
    let font = OutlineFont::new(data, &[("wght", 400.0)]).unwrap();

    let size = 32.0;
    let (glyph, largest) = largest_allocation(|| font.rasterize('H', size));
    assert!(largest <= 4 * 1024 * 1024, "{largest} bytes");
    let side = 8.0 * size;
    assert!(
        f32::from(glyph.width) <= side && f32::from(glyph.height) <= side,
        "{}x{}",
        glyph.width,
        glyph.height
    );
}

/// The bytes of table `tag` in font `data`: offset and length.
#[cfg(feature = "outline")]
#[allow(clippy::unwrap_used, reason = "the test's own fonts have the table")]
fn table(data: &[u8], tag: &[u8; 4]) -> (usize, usize) {
    let tables = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let record = (0..tables)
        .map(|i| 12 + i * 16)
        .find(|&r| &data[r..r + 4] == tag)
        .unwrap();
    let word = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
    (word(record + 8), word(record + 12))
}

/// Fonts whose mark lookups (GPOS) and substitutions (GSUB) are damaged
/// here and there: a pack with their anchors is still made, with no more
/// memory asked for at once than for the font undamaged, and drawing the marks with the
/// font itself panics nowhere.
#[cfg(feature = "outline")]
#[test]
fn damaged_mark_lookups_are_read_without_harm() {
    use paperpaw_text::{OutlineFont, build_pack, charset};

    let fonts: [(&[u8], &str, &str); 2] = [
        (
            include_bytes!("../../../tools/fonts/NotoSerifHebrew.ttf"),
            "hebrew",
            "\u{5d1}\u{5b8}\u{5e9}\u{5c1}",
        ),
        (
            include_bytes!("../../../tools/fonts/NotoNaskhArabic.ttf"),
            "arabic",
            "\u{628}\u{650}\u{644}\u{651}\u{64e}",
        ),
    ];
    let mut seed = 0x2545_f491_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize
    };
    for (font, set, text) in fonts {
        // What the font asks for undamaged (its kerning pairs, the pack).
        let chars = || charset::chars(charset::named(set).unwrap());
        let whole = OutlineFont::new(font, &[]).unwrap();
        let (_, usual) = largest_allocation(|| build_pack(&whole, &[20.0], chars()));
        for _ in 0..12 {
            let mut data = font.to_vec();
            for tag in [b"GPOS", b"GSUB"] {
                let (at, len) = table(&data, tag);
                for _ in 0..48 {
                    let i = at + next() % len.min(4096);
                    data[i] = next() as u8;
                }
            }
            let outline = OutlineFont::new(data.clone(), &[]).unwrap();
            let (_, largest) = largest_allocation(|| build_pack(&outline, &[20.0], chars()));
            assert!(
                largest <= usual.max(BOUND),
                "{largest} bytes, {usual} undamaged"
            );
            let font = paperpaw_text::Font::from_data(data, &[]).unwrap();
            let mut frame =
                paperpaw_graphics::Framebuffer::new(80, 40, paperpaw_platform::PixelFormat::Gray8)
                    .unwrap();
            let mut cache = paperpaw_text::GlyphCache::new(16 * 1024);
            let style =
                paperpaw_text::TextStyle::new(&font, 0, 20.0, paperpaw_platform::Color::BLACK);
            let around = paperpaw_text::Around::new(
                paperpaw_lang::Direction::Rtl,
                paperpaw_lang::Lang::NONE,
            );
            paperpaw_text::draw_text_in(
                &mut frame.canvas(),
                &mut cache,
                &style,
                10.0,
                30,
                text,
                &around,
            );
        }
    }
}
