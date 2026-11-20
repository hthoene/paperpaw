//! What drawing text from a pack with deflated bitmaps costs (#428), for
//! `valgrind --tool=callgrind` by hand: `PP_BENCH_PACK` names a pack as the
//! builder made it, `PP_BENCH_MODE` is `flat` or `deflated`, `PP_BENCH_PAGES`
//! the pages drawn, each of other text, at 24 px and then 32 px.

#![allow(clippy::unwrap_used, reason = "a measurement")]

use paperpaw_text::{Font, GlyphCache, deflate_bitmaps};

const ONE: &str = "It was a bright cold day in April, and the clocks were striking thirteen. Winston Smith, his chin nuzzled into his breast in an effort to escape the vile wind, slipped quickly through the glass doors of Victory Mansions, though not quickly enough to prevent a swirl of gritty dust from entering along with him. The hallway smelt of boiled cabbage and old rag mats. At one end of it a coloured poster, too large for indoor display, had been tacked to the wall.";
const TWO: &str = "Call me Ishmael. Some years ago, never mind how long precisely, having little or no money in my purse, and nothing particular to interest me on shore, I thought I would sail about a little and see the watery part of the world. It is a way I have of driving off the spleen and regulating the circulation. Whenever I find myself growing grim about the mouth; whenever it is a damp, drizzly November in my soul; then, I account it high time to get to sea.";

#[test]
#[ignore = "a measurement, run by hand"]
fn what_a_page_costs() {
    let Ok(path) = std::env::var("PP_BENCH_PACK") else {
        return;
    };
    let flat = std::fs::read(path).unwrap();
    let pack = if std::env::var("PP_BENCH_MODE").is_ok_and(|m| m == "deflated") {
        deflate_bitmaps(&flat, &|block| {
            miniz_oxide::deflate::compress_to_vec(block, 9)
        })
        .unwrap()
    } else {
        flat
    };
    let pages: usize = std::env::var("PP_BENCH_PAGES")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(1);
    let font = Font::from_data(pack, &[]).unwrap();
    let mut cache = GlyphCache::new(12 * 1024);
    let mut ink = 0usize;
    for page in 0..pages {
        for size in [24.0, 32.0] {
            for text in [ONE, TWO, ONE, TWO] {
                for ch in text.chars().cycle().skip(page * 7).take(500) {
                    ink += usize::from(cache.glyph(0, &font, ch, size).width);
                }
            }
        }
    }
    assert!(ink > 0);
    eprintln!("cache {} bytes", cache.bytes());
}
