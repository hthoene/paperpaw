//! Text drawn and measured with what lies around it: the base direction
//! of its line, its language and the characters before and after it.
//!
//! [`draw_text_in`] and [`measure_in`] put a text's characters in the
//! order they are shown (UAX #9: right-to-left letters and the numbers
//! among them), give Arabic letters the form they take where they stand
//! (joined to the letters around, also across the text's edges), draw
//! mirrored brackets in right-to-left runs and keep combining marks on
//! their letters. Text in one direction with nothing around it draws as
//! [`draw_text`](crate::draw_text) draws it.
//!
//! A text of up to [`STACK`] characters is set without the heap; a longer
//! one takes a buffer of a few bytes per character while it is set.

use alloc::vec;

use paperpaw_graphics::Canvas;
use paperpaw_lang::{Direction, Lang};

use crate::anchors::{self, Anchors};
use crate::arabic::{self, Context, Form, NO_GLYPH};
use crate::bidi::{self, Carry, Level};
use crate::{Font, GlyphCache, TextStyle, raster};

/// The most characters set without the heap.
const STACK: usize = 64;

/// What lies around a text that is drawn or measured on its own: a part
/// of a line, a label, a word in a run of another style.
#[derive(Copy, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Around {
    /// The nearest character before the text that is not a mark, if it
    /// goes on from one: Arabic letters join it.
    pub before: Option<char>,
    /// The nearest character after the text that is not a mark.
    pub after: Option<char>,
    /// The direction of the line the text is on: `Auto` takes it from the
    /// text's first strong letter (UAX #9 P2, P3), else left to right.
    pub base: Direction,
    /// The language of the text: [`Lang::NONE`] if unknown. Reserved for
    /// the forms a language gives letters; not read yet.
    pub lang: Lang,
    /// [`Around::NO_JOIN`], [`Around::NO_MIRROR`], [`Around::NO_KERN`];
    /// bits 3 to 31 are zero now (see [`Around::with_flags`]).
    pub flags: u32,
}

impl Around {
    /// No joining across the text's edges, whatever `before` and `after`
    /// are.
    pub const NO_JOIN: u32 = 1;
    /// Brackets and other mirrored characters drawn as they are in
    /// right-to-left runs.
    pub const NO_MIRROR: u32 = 2;
    /// No kerning.
    pub const NO_KERN: u32 = 4;

    /// Nothing around: a whole line in the direction of its first strong
    /// letter.
    pub const NONE: Around = Around {
        before: None,
        after: None,
        base: Direction::Auto,
        lang: Lang::NONE,
        flags: 0,
    };

    /// A text on a line of direction `base`, in language `lang`.
    pub const fn new(base: Direction, lang: Lang) -> Around {
        Around {
            base,
            lang,
            ..Around::NONE
        }
    }

    /// The same, after `before` and before `after` (see
    /// [`Around::before`]).
    #[must_use]
    pub const fn with_context(mut self, before: Option<char>, after: Option<char>) -> Around {
        self.before = before;
        self.after = after;
        self
    }

    /// The same with `flags` ([`Around::NO_JOIN`], [`Around::NO_MIRROR`],
    /// [`Around::NO_KERN`]); the other bits are zero now and get a
    /// meaning only with a new revision of the text functions.
    #[must_use]
    pub const fn with_flags(mut self, flags: u32) -> Around {
        self.flags = flags;
        self
    }

    const fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

impl Default for Around {
    fn default() -> Self {
        Around::NONE
    }
}

paperpaw_table::shared! {
/// Width of `text` in pixels, set as [`draw_text_in`] sets it.
pub fn measure_in(font: &Font, size: f32, text: &str, around: &Around) -> f32 {
    if plain(text, around) {
        return crate::measure(font, size, text);
    }
    lay(font, size, text, around, false, |_, _, _, _| {})
} @ text::TEXT_MEASURE_IN
}

paperpaw_table::shared! {
/// Draws one line of `text` in `style` with its baseline at `baseline`,
/// its left edge at `x`, as what lies `around` it asks: in the order its
/// characters are shown, Arabic letters joined, brackets mirrored in
/// right-to-left runs. Returns its width, as [`measure_in`] gives it.
/// Newlines are not interpreted.
pub fn draw_text_in(
    canvas: &mut Canvas<'_>,
    cache: &mut GlyphCache,
    style: &TextStyle<'_>,
    x: f32,
    baseline: i32,
    text: &str,
    around: &Around,
) -> f32 {
    let based = style.based || around.before.is_some();
    if plain(text, around) {
        return crate::draw_text(canvas, cache, &style.based(based), x, baseline, text);
    }
    let (font, index, size) = (style.font, style.font_index, style.size);
    // The letter marks sit on (its key and pen) and the mark drawn last on
    // it (its anchors and place from the letter's pen).
    let mut letter: Option<(char, i32)> = None;
    let mut before: Option<(Anchors, (i32, i32))> = None;
    lay(font, size, text, around, based, |key, pen, mark, lone| {
        let mut placed = None;
        if !mark {
            letter = Some((key, raster::floor(x + pen + 0.5)));
            before = None;
        } else if let Some((on, at)) = letter
            && !lone
        {
            let anchors = cache.anchors(index, font, key, size);
            if anchors::is_held(&anchors) {
                let held = cache.anchors(index, font, on, size);
                let before_it = before.as_ref().map(|(a, p)| (a, *p));
                let offset = anchors::place(&anchors, before_it, &held)
                    .unwrap_or_else(|| centred(cache, style, on, key));
                placed = Some((at.saturating_add(offset.0), offset.1));
                before = Some((anchors, offset));
            }
        }
        let glyph = cache.glyph(index, font, key, size);
        let (gx, dy) = match placed {
            Some((at, dy)) => (at.saturating_add(i32::from(glyph.left)), dy),
            None => (crate::glyph_x(x + pen, glyph.left, lone), 0),
        };
        crate::paint(canvas, &glyph, gx, baseline.saturating_add(dy), style);
    })
} @ text::TEXT_DRAW_TEXT_IN
}

/// Where a mark with anchors goes on a letter that holds none of its
/// classes: its ink centred over the letter's, at the height its glyph
/// gives it.
fn centred(cache: &mut GlyphCache, style: &TextStyle<'_>, letter: char, mark: char) -> (i32, i32) {
    let centre = |cache: &mut GlyphCache, key: char| {
        let glyph = cache.glyph(style.font_index, style.font, key, style.size);
        i32::from(glyph.left) + i32::from(glyph.width) / 2
    };
    let letter = centre(cache, letter);
    (letter - centre(cache, mark), 0)
}

/// Whether `text` draws as [`draw_text`](crate::draw_text) draws it:
/// left to right, nothing to join, shape or mirror, kerned.
fn plain(text: &str, around: &Around) -> bool {
    !around.has(Around::NO_KERN)
        && around.base != Direction::Rtl
        && !bidi::needs_bidi(text)
        && !joins(around.before)
        && !joins(around.after)
}

/// Whether Arabic letters in a text would join `ch`.
fn joins(ch: Option<char>) -> bool {
    ch.is_some_and(|ch| {
        !matches!(
            arabic::joining(ch),
            arabic::Joining::NonJoining | arabic::Joining::Transparent
        )
    })
}

/// Sets `text`: calls `put(key, pen, mark, lone)` for each glyph in the
/// order it is shown, `pen` from the text's left edge, `mark` for a
/// combining mark (after its letter), `lone` for one with no letter;
/// returns the width.
fn lay(
    font: &Font,
    size: f32,
    text: &str,
    around: &Around,
    based: bool,
    put: impl FnMut(char, f32, bool, bool),
) -> f32 {
    let count = text.chars().count().min(usize::from(u16::MAX));
    if count <= STACK {
        let mut keys = ['\0'; STACK];
        let mut levels = [Level::LTR; STACK];
        let mut order = [0u16; STACK];
        let mut marks = [false; STACK];
        let set = Set {
            keys: &mut keys[..count],
            levels: &mut levels[..count],
            order: &mut order[..count],
            marks: &mut marks[..count],
        };
        set.lay(font, size, text, around, based, put)
    } else {
        let mut keys = vec!['\0'; count];
        let mut levels = vec![Level::LTR; count];
        let mut order = vec![0u16; count];
        let mut marks = vec![false; count];
        let set = Set {
            keys: &mut keys,
            levels: &mut levels,
            order: &mut order,
            marks: &mut marks,
        };
        set.lay(font, size, text, around, based, put)
    }
}

/// The room a text is set in: per character its key, level and whether
/// it is a mark, and the order they are shown in. All of one length.
struct Set<'a> {
    keys: &'a mut [char],
    levels: &'a mut [Level],
    order: &'a mut [u16],
    marks: &'a mut [bool],
}

impl Set<'_> {
    fn lay(
        self,
        font: &Font,
        size: f32,
        text: &str,
        around: &Around,
        based: bool,
        mut put: impl FnMut(char, f32, bool, bool),
    ) -> f32 {
        let Set {
            keys,
            levels,
            order,
            marks,
        } = self;
        let n = keys.len();
        let paragraph = match around.base {
            Direction::Rtl => Level::RTL,
            Direction::Ltr => Level::LTR,
            _ => bidi::paragraph_level(text).unwrap_or(Level::LTR),
        };
        if paragraph.is_rtl() || bidi::needs_bidi(text) {
            let mut carry = Carry::new(paragraph);
            carry.resolve(text.chars(), n, levels);
            bidi::reset_line_end(text, levels, paragraph);
        }
        let context = match around.has(Around::NO_JOIN) {
            true => Context::NONE,
            false => Context::new(around.before, around.after),
        };
        // An alef whose lam-alef ligature the font lacks is drawn itself.
        let mut alef = false;
        let shaped = text.chars().zip(arabic::keys(text, context));
        for (i, (ch, key)) in shaped.take(n).enumerate() {
            let rtl = levels.get(i).is_some_and(|l| l.is_rtl());
            let mut key = key;
            if core::mem::take(&mut alef) && key == NO_GLYPH {
                key = shown(font, arabic::form_key(ch, Form::Final), ch);
            } else if key != ch && key != NO_GLYPH && !font.has_char(key) {
                alef = ('\u{FEF5}'..='\u{FEFC}').contains(&key);
                key = arabic::letter_of(key).0;
            } else if rtl && key == ch && !around.has(Around::NO_MIRROR) {
                key = bidi::mirror(ch);
            }
            if let (Some(k), Some(m)) = (keys.get_mut(i), marks.get_mut(i)) {
                *k = key;
                *m = crate::is_mark(ch) || arabic::is_mark(ch);
            }
        }
        let shown = bidi::visual_order(levels, order);
        let kern = !around.has(Around::NO_KERN);
        let mut pen = 0.0;
        // A mark before the text's first letter has nothing to sit on.
        let first = marks.iter().position(|mark| !mark).unwrap_or(n);
        let mut before: Option<(usize, char)> = None;
        let mut emit = |i: usize, pen: &mut f32| {
            let (Some(&key), Some(&mark), Some(level)) = (keys.get(i), marks.get(i), levels.get(i))
            else {
                return;
            };
            if key == NO_GLYPH {
                return;
            }
            if let Some((b, left)) = before
                && kern
                && b + 1 == i
                && !level.is_rtl()
                && !levels.get(b).is_some_and(|l| l.is_rtl())
            {
                *pen += font.kern(left, key, size);
            }
            put(key, *pen, mark, mark && !based && i < first);
            *pen += font.advance(key, size);
            before = Some((i, key));
        };
        for &at in order.iter().take(shown) {
            let i = usize::from(at);
            let rtl = levels.get(i).is_some_and(|l| l.is_rtl());
            if rtl && marks.get(i) == Some(&true) && has_base(marks, levels, i) {
                // Drawn after its letter, as in left-to-right text.
                continue;
            }
            emit(i, &mut pen);
            if rtl {
                let mut j = i + 1;
                while marks.get(j) == Some(&true) && levels.get(j).is_some_and(|l| l.is_rtl()) {
                    emit(j, &mut pen);
                    j += 1;
                }
            }
        }
        pen
    }
}

/// Whether the right-to-left mark at `i` follows a right-to-left letter,
/// with only such marks between them.
fn has_base(marks: &[bool], levels: &[Level], i: usize) -> bool {
    let mut k = i;
    while k > 0 {
        k -= 1;
        if !levels.get(k).is_some_and(|l| l.is_rtl()) {
            return false;
        }
        if marks.get(k) == Some(&false) {
            return true;
        }
    }
    false
}

/// `key` if the font has it, else `ch`.
fn shown(font: &Font, key: char, ch: char) -> char {
    if font.has_char(key) { key } else { ch }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::float_cmp,
    reason = "tests panic on purpose when something is wrong; advances are exact"
)]
mod tests {
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::*;
    use crate::{BitmapFont, Glyph, LineMetrics, PackBuilder};

    /// A pack of glyphs that draw nothing, `chars` each one pixel wide,
    /// the ligature of lam and alef and the final form of beh among
    /// them where `forms`, `a` and `v` kerned by -0.5.
    fn font(chars: &str, forms: bool) -> Font {
        let metrics = LineMetrics {
            ascent: 8.0,
            descent: 2.0,
            line_height: 10.0,
        };
        let mut glyphs: Vec<(char, Glyph)> = chars
            .chars()
            .map(|c| {
                (
                    c,
                    Glyph::empty(if crate::is_mark(c) || arabic::is_mark(c) {
                        0.0
                    } else {
                        1.0
                    }),
                )
            })
            .collect();
        if forms {
            glyphs.push(('\u{FEFB}', Glyph::empty(1.0)));
            glyphs.push(('\u{FE90}', Glyph::empty(1.0)));
        }
        let mut builder = PackBuilder::new();
        builder.kerning(&[('a', 'v', -0.05)]);
        builder.add_strike(10.0, metrics, glyphs);
        Font::Bitmap(BitmapFont::new(builder.build()).unwrap())
    }

    /// The keys of `text` from the left, as set with `around`.
    fn shown(font: &Font, text: &str, around: &Around) -> String {
        let mut keys = String::new();
        lay(font, 10.0, text, around, false, |key, _, _, _| {
            keys.push(key)
        });
        keys
    }

    const LATIN: &str = "av (x) ";
    const HEBREW: &str = "אבג\u{5B0}";
    const ARABIC: &str = "\u{644}\u{627}\u{628}";

    fn all() -> Font {
        font(&alloc::format!("{LATIN}{HEBREW}{ARABIC}"), false)
    }

    #[test]
    fn plain_text_is_set_as_before() {
        let font = all();
        let kern = font.kern('a', 'v', 10.0);
        assert!(kern < 0.0);
        assert_eq!(measure_in(&font, 10.0, "av", &Around::NONE), 2.0 + kern);
        assert_eq!(
            measure_in(&font, 10.0, "av", &Around::NONE),
            crate::measure(&font, 10.0, "av")
        );
        assert_eq!(shown(&font, "a (v)", &Around::NONE), "a (v)");
        let unkerned = Around::NONE.with_flags(Around::NO_KERN);
        assert_eq!(measure_in(&font, 10.0, "av", &unkerned), 2.0);
    }

    #[test]
    fn right_to_left_text_is_shown_from_its_end() {
        let font = all();
        assert_eq!(shown(&font, "אבג", &Around::NONE), "גבא");
        // In a right-to-left line Latin words keep their order, and stay
        // kerned; brackets are mirrored, unless asked not to be.
        let rtl = Around::new(Direction::Rtl, Lang::NONE);
        assert_eq!(shown(&font, "av אב", &rtl), "בא av");
        let kern = font.kern('a', 'v', 10.0);
        assert_eq!(measure_in(&font, 10.0, "av אב", &rtl), 5.0 + kern);
        assert_eq!(shown(&font, "(א)", &rtl), "(א)");
        assert_eq!(
            shown(&font, "(א)", &rtl.with_flags(Around::NO_MIRROR)),
            ")א("
        );
        // A Hebrew word in a left-to-right line.
        let ltr = Around::new(Direction::Ltr, Lang::NONE);
        assert_eq!(shown(&font, "a אב v", &ltr), "a בא v");
    }

    #[test]
    fn marks_stay_on_their_letters() {
        let font = all();
        // Sheva after gimel, drawn after it as in left-to-right text.
        assert_eq!(shown(&font, HEBREW, &Around::NONE), "ג\u{5B0}בא");
        let mut lone = Vec::new();
        lay(
            &font,
            10.0,
            "\u{5B0}א",
            &Around::new(Direction::Rtl, Lang::NONE),
            false,
            |k, _, _, l| {
                lone.push((k, l));
            },
        );
        assert_eq!(lone, [('א', false), ('\u{5B0}', true)]);
    }

    #[test]
    fn arabic_letters_join_also_across_the_edges() {
        let with = font(ARABIC, true);
        let without = font(ARABIC, false);
        // Lam and alef make one glyph where the font has it, else both
        // are drawn as letters.
        assert_eq!(shown(&with, "\u{644}\u{627}", &Around::NONE), "\u{FEFB}");
        assert_eq!(
            shown(&without, "\u{644}\u{627}", &Around::NONE),
            "\u{627}\u{644}"
        );
        assert_eq!(
            measure_in(&without, 10.0, "\u{644}\u{627}", &Around::NONE),
            2.0
        );
        // Beh after a beh of another run takes its final form.
        let after = Around::NONE.with_context(Some('\u{628}'), None);
        assert_eq!(shown(&with, "\u{628}", &after), "\u{FE90}");
        assert_eq!(
            shown(&with, "\u{628}", &after.with_flags(Around::NO_JOIN)),
            "\u{628}"
        );
        assert_eq!(shown(&with, "\u{628}", &Around::NONE), "\u{628}");
    }

    #[test]
    fn long_text_is_set_whole() {
        let font = all();
        let text: String = "אב".repeat(STACK);
        let expected: String = text.chars().rev().collect();
        assert_eq!(shown(&font, &text, &Around::NONE), expected);
        assert_eq!(
            measure_in(&font, 10.0, &text, &Around::NONE),
            (2 * STACK) as f32
        );
    }

    #[test]
    fn around_is_built_whole() {
        let around = Around::new(Direction::Rtl, Lang::parse("ar"))
            .with_context(Some('a'), Some('b'))
            .with_flags(Around::NO_KERN);
        assert_eq!(around.base, Direction::Rtl);
        assert_eq!(
            (around.before, around.after, around.flags),
            (Some('a'), Some('b'), 4)
        );
        assert_eq!(Around::default(), Around::NONE);
    }

    /// A pack with alef (6 px of ink over the baseline) and qamats (2 px,
    /// a row under it), qamats held on alef by `class` where alef holds
    /// that class.
    fn anchored(class: u8) -> Font {
        use crate::anchors::{self, Anchor, Anchors};
        let metrics = LineMetrics {
            ascent: 8.0,
            descent: 2.0,
            line_height: 10.0,
        };
        let ink = |width: u16, height: u16, top: i16, advance: f32| Glyph {
            left: 0,
            top,
            width,
            height,
            advance,
            data: alloc::vec![0xFF; usize::from(width.div_ceil(2) * height)],
        };
        let mut letter = Anchors::NONE;
        letter.push(Anchor {
            class: 0,
            x: 1,
            y: 2,
        });
        let mut mark = Anchors::NONE;
        mark.push(Anchor {
            class: Anchor::OWN | class,
            x: 0,
            y: 0,
        });
        let mut builder = PackBuilder::new();
        builder.add_strike(
            10.0,
            metrics,
            alloc::vec![
                ('\u{5D0}', ink(6, 6, -6, 7.0)),
                ('\u{5B8}', ink(2, 1, 1, 0.0))
            ],
        );
        let keys = alloc::vec![alloc::vec![('\u{5B8}', mark), ('\u{5D0}', letter)]];
        builder.table(BitmapFont::ANCHORS, 1, &anchors::build(&keys));
        Font::Bitmap(BitmapFont::new(builder.build()).unwrap())
    }

    /// The columns and rows with ink after drawing `text` at 10, 20.
    fn ink_of(font: &Font, text: &str) -> Vec<(u16, u16)> {
        let mut frame =
            paperpaw_graphics::Framebuffer::new(40, 30, paperpaw_platform::PixelFormat::Gray8)
                .unwrap();
        {
            let mut canvas = frame.canvas();
            canvas.clear(paperpaw_platform::Color::WHITE);
            let mut cache = GlyphCache::new(4096);
            let style = TextStyle::new(font, 0, 10.0, paperpaw_platform::Color::BLACK);
            let around = Around::new(Direction::Rtl, Lang::NONE);
            draw_text_in(&mut canvas, &mut cache, &style, 10.0, 20, text, &around);
        }
        let view = frame.view();
        (0..30)
            .flat_map(|y| (0..40).map(move |x| (x, y)))
            .filter(|&(x, y)| view.pixel(x, y) != paperpaw_platform::Color::WHITE)
            .collect()
    }

    #[test]
    fn marks_go_where_anchors_meet_else_centred_on_their_letter() {
        // Held: the mark's point on alef's, 1 right and 2 down of its pen.
        let font = anchored(0);
        let alone = ink_of(&font, "\u{5D0}");
        let mark: Vec<_> = ink_of(&font, "\u{5D0}\u{5B8}")
            .into_iter()
            .filter(|p| !alone.contains(p))
            .collect();
        assert_eq!(mark, [(11, 23), (12, 23)]);
        // A class alef does not hold: centred over its ink, at the height
        // the mark's glyph gives it.
        let font = anchored(3);
        let alone = ink_of(&font, "\u{5D0}");
        let mark: Vec<_> = ink_of(&font, "\u{5D0}\u{5B8}")
            .into_iter()
            .filter(|p| !alone.contains(p))
            .collect();
        assert_eq!(mark, [(12, 21), (13, 21)]);
    }
}
