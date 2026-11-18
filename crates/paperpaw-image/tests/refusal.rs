//! A refused allocation while a picture is decoded is an `OutOfMemory`
//! answer: never an abort, a panic or a picture that differs from a clean
//! run. The n-th allocation of the decoder is refused, for each n, over real
//! baseline and progressive JPEGs and PNGs (palette, colour, see-through).
//!
//! The only test in its binary: the allocator below is the process's. Set
//! `ALLOC_TRACE=1` to see where a refusal aborts, `ALLOC_SITES=1` to list
//! allocations made outside `try_reserve` (developer aids).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_image::{Fit, Format, ImageError, ImageSink, decode};

std::thread_local! {
    /// Allocations counted since the sweep began.
    static COUNT: Cell<u64> = const { Cell::new(0) };
    /// The allocation to refuse, 1-based; 0 none.
    static REFUSE: Cell<u64> = const { Cell::new(0) };
    static ARMED: Cell<bool> = const { Cell::new(false) };
    /// Above 0: the allocator's own bookkeeping is allocating.
    static PAUSED: Cell<u32> = const { Cell::new(0) };
    static SITES: std::cell::RefCell<std::collections::BTreeMap<String, u64>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

struct Refusing;

/// Whether to refuse this allocation.
fn refused() -> bool {
    ARMED
        .try_with(|armed| {
            if !armed.get() || PAUSED.with(Cell::get) > 0 {
                return false;
            }
            PAUSED.with(|paused| paused.set(1));
            if std::env::var_os("ALLOC_SITES").is_some() {
                note_site();
            }
            let count = COUNT.with(|count| {
                count.set(count.get() + 1);
                count.get()
            });
            let refuse = count == REFUSE.with(Cell::get);
            if refuse && std::env::var_os("ALLOC_TRACE").is_some() {
                eprintln!(
                    "refused allocation {count}:\n{}",
                    std::backtrace::Backtrace::force_capture()
                );
            }
            PAUSED.with(|paused| paused.set(0));
            refuse
        })
        .unwrap_or(false)
}

/// Notes the first `paperpaw` frame of an allocation that did not come
/// through a `try_*` call.
fn note_site() {
    let text = std::backtrace::Backtrace::force_capture().to_string();
    if text.contains("try_reserve") {
        return;
    }
    let mut lines = text.lines();
    let mut site = "(no paperpaw frame)".to_string();
    while let Some(line) = lines.next() {
        if line.contains("paperpaw") && !line.contains("refusal::") {
            site = format!("{} {}", line.trim(), lines.next().unwrap_or("").trim());
            break;
        }
    }
    SITES.with(|sites| *sites.borrow_mut().entry(site).or_default() += 1);
}

// SAFETY: allocations and frees are the system allocator's; a refusal is a
// null return, which `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refused() {
            return core::ptr::null_mut();
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if refused() {
            return core::ptr::null_mut();
        }
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refused() {
            return core::ptr::null_mut();
        }
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

/// What a sink asks for, and whether it takes colour and see-through rows
/// itself (`Own`) or leaves them to the trait's default conversions.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Gray,
    Colour,
    Alpha,
    DefaultColour,
    DefaultAlpha,
}

/// A sink that keeps a hash of what it is given, and allocates nothing.
#[derive(Default, PartialEq, Eq, Debug)]
struct Hash {
    colour: bool,
    alpha: bool,
    width: u32,
    height: u32,
    rows: u32,
    hash: u64,
}

impl Hash {
    fn mix(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.hash = (self.hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.rows += 1;
    }
}

impl ImageSink for Hash {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        self.width = width;
        self.height = height;
        self.hash = 0xcbf2_9ce4_8422_2325;
        Ok(())
    }
    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        self.mix(gray);
        Ok(())
    }
    fn wants_color(&self) -> bool {
        self.colour
    }
    fn row_rgb(&mut self, rgb: &[u8]) -> Result<(), ImageError> {
        self.mix(rgb);
        Ok(())
    }
    fn wants_alpha(&self) -> bool {
        self.alpha
    }
    fn row_alpha(&mut self, gray_alpha: &[u8]) -> Result<(), ImageError> {
        self.mix(gray_alpha);
        Ok(())
    }
}

/// The same, taking colour and see-through rows by the trait's defaults
/// (which make gray rows of them).
#[derive(Default, PartialEq, Eq, Debug)]
struct Plain(Hash);

impl ImageSink for Plain {
    fn start(&mut self, width: u32, height: u32) -> Result<(), ImageError> {
        self.0.start(width, height)
    }
    fn row(&mut self, gray: &[u8]) -> Result<(), ImageError> {
        self.0.row(gray)
    }
    fn wants_color(&self) -> bool {
        self.0.colour
    }
    fn wants_alpha(&self) -> bool {
        self.0.alpha
    }
}

fn picture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(path).expect("a test picture")
}

/// What each way of asking is run at.
const MODES: [(u32, Kind); 6] = [
    (120, Kind::Gray),
    (120, Kind::Colour),
    (4000, Kind::Gray),
    (120, Kind::Alpha),
    (120, Kind::DefaultColour),
    (120, Kind::DefaultAlpha),
];

/// Decodes `data` with the n-th allocation refused (0: none).
fn run(data: &[u8], side: u32, kind: Kind, nth: u64) -> (Result<(), ImageError>, Hash, u64) {
    let format = Format::detect(data).expect("a known format");
    let mut hash = Hash {
        colour: matches!(kind, Kind::Colour | Kind::DefaultColour),
        alpha: matches!(kind, Kind::Alpha | Kind::DefaultAlpha),
        ..Hash::default()
    };
    let mut plain = Plain::default();
    plain.0.colour = hash.colour;
    plain.0.alpha = hash.alpha;
    let sink: &mut dyn ImageSink = match kind {
        Kind::DefaultColour | Kind::DefaultAlpha => &mut plain,
        _ => &mut hash,
    };
    let mut source = data;
    COUNT.with(|count| count.set(0));
    REFUSE.with(|refuse| refuse.set(nth));
    ARMED.with(|armed| armed.set(true));
    let result = decode(format, &mut source, Fit::side(side), sink);
    ARMED.with(|armed| armed.set(false));
    let count = COUNT.with(Cell::get);
    let seen = if matches!(kind, Kind::DefaultColour | Kind::DefaultAlpha) {
        plain.0
    } else {
        hash
    };
    (result, seen, count)
}

const FILES: [&str; 8] = [
    "alice-cover.jpg",
    "alice-progressive.jpg",
    "kafka-title.jpg",
    "palette.png",
    "colors-palette.png",
    "colors-rgba.png",
    "gray-alpha.png",
    "bilevel.png",
];

/// Developer aid: lists the allocations of clean runs that do not go
/// through `try_reserve` (`ALLOC_SITES=1 cargo test -- --ignored`).
#[test]
#[ignore = "developer aid: lists the decoders' infallible allocation sites"]
fn list_infallible_sites() {
    for name in FILES {
        let data = picture(name);
        for (side, kind) in MODES {
            run(&data, side, kind, 0).0.expect("decodes");
        }
    }
    SITES.with(|sites| {
        for (site, n) in sites.borrow().iter() {
            eprintln!("{n:6} {site}");
        }
        eprintln!("{} sites", sites.borrow().len());
    });
}

#[test]
fn a_refused_allocation_while_decoding_is_out_of_memory() {
    for name in FILES {
        let data = picture(name);
        for (side, kind) in MODES {
            let (clean, picture, total) = run(&data, side, kind, 0);
            clean.expect("a clean run decodes");
            assert!(picture.rows > 0, "{name}: no rows");
            assert!(
                total > 2,
                "{name}: {total} allocations is not representative"
            );
            let mut refusals = 0;
            for nth in 1..=total {
                let (result, seen, _) = run(&data, side, kind, nth);
                match result {
                    Err(ImageError::OutOfMemory) => refusals += 1,
                    Err(other) => panic!("{name}@{side}: allocation {nth} refused: {other:?}"),
                    // A progressive JPEG whose coefficients do not fit is
                    // decoded at a smaller scale or in gray, by design
                    // (`progressive::decode`): a whole picture, smaller or
                    // with fewer details.
                    Ok(()) if name.contains("progressive") => {
                        assert!(
                            seen.rows == seen.height
                                && seen.width > 0
                                && seen.width <= picture.width
                                && seen.height <= picture.height,
                            "{name}@{side}: allocation {nth}: {seen:?}"
                        );
                    }
                    Ok(()) => assert_eq!(seen, picture, "{name}@{side}: allocation {nth}"),
                }
                // Nothing half-made stays: a clean run after it decodes as before.
                let (again, seen, _) = run(&data, side, kind, 0);
                again.expect("a retry decodes");
                assert_eq!(seen, picture, "{name}@{side}: retry after {nth}");
            }
            assert!(
                refusals > 0,
                "{name}@{side}: no refusal answered OutOfMemory"
            );
        }
    }
}
