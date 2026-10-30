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
