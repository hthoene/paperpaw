//! Mark anchors read from OpenType fonts: the GPOS `mark` (mark to base)
//! and `mkmk` (mark to mark) lookups of Arabic and Hebrew, as packs keep
//! them (see [`crate::anchors`]). Other scripts get none: their marks stay
//! where their glyphs put them.
//!
//! A font's classes are numbered per lookup and subtable, in the order of
//! the lookups. A mark tries its mark-to-mark classes first, as a shaper
//! moves a mark onto the mark before it after placing it on its letter.
//!
//! Packs draw an Arabic letter's form as one glyph (its presentation
//! form), where a font may hold the form's anchors on a glyph without the
//! letter's dots, which it adds as marks (`ccmp`). Such a key gets the
//! anchors of that glyph, of the form (`isol`, `fina`, `init`, `medi`),
//! and those of its dots' mark-to-mark points where the dots sit: a mark
//! below beh goes under its dot.

use alloc::vec::Vec;

use read_fonts::tables::gpos::{AnchorTable, DeviceOrVariationIndex, PositionSubtables};
use read_fonts::tables::gsub::{SingleSubst, SubstitutionSubtables};
use read_fonts::tables::layout::{FeatureList, LangSys, ScriptList};
use read_fonts::tables::variations::{DeltaSetIndex, ItemVariationStore};
use read_fonts::types::{F2Dot14, GlyphId, GlyphId16, Tag};
use read_fonts::{FontRef, TableProvider};

use crate::anchors::{Anchor, Anchors};
use crate::arabic::{self, Form};

/// The lookups of one script.
#[derive(Default)]
struct Script {
    /// Mark to base, mark to mark (GPOS), in the order they apply.
    mark: Vec<u16>,
    mkmk: Vec<u16>,
    /// Composition and the forms (GSUB): isolated, final, initial,
    /// medial.
    ccmp: Vec<u16>,
    forms: [Vec<u16>; 4],
}

/// The marks of one font instance.
pub(crate) struct Marks<'a> {
    font: FontRef<'a>,
    coords: &'a [F2Dot14],
    store: Option<ItemVariationStore<'a>>,
    arab: Script,
    hebr: Script,
    /// Per GPOS lookup and subtable, the number of its first class
    /// (`None`: past what a pack holds).
    first: Vec<(u16, usize, Option<u8>)>,
    /// Pixels per font unit.
    scale: f32,
}

/// One of a font's anchors: x and y in font units (y up).
type Point = (f32, f32);

impl<'a> Marks<'a> {
    pub(crate) fn new(font: FontRef<'a>, coords: &'a [F2Dot14]) -> Self {
        let store = match coords.is_empty() {
            true => None,
            false => font
                .gdef()
                .ok()
                .and_then(|gdef| gdef.item_var_store())
                .and_then(Result::ok),
        };
        let script = |tag: &[u8; 4]| {
            let tag = Tag::new(tag);
            let mut s = Script::default();
            if let Ok(gpos) = font.gpos()
                && let (Ok(scripts), Ok(features)) = (gpos.script_list(), gpos.feature_list())
            {
                s.mark = lookups(&scripts, &features, tag, b"mark");
                s.mkmk = lookups(&scripts, &features, tag, b"mkmk");
            }
            if let Ok(gsub) = font.gsub()
                && let (Ok(scripts), Ok(features)) = (gsub.script_list(), gsub.feature_list())
            {
                s.ccmp = lookups(&scripts, &features, tag, b"ccmp");
                for (forms, feature) in s.forms.iter_mut().zip([b"isol", b"fina", b"init", b"medi"])
                {
                    *forms = lookups(&scripts, &features, tag, feature);
                }
            }
            s
        };
        let (arab, hebr) = (script(b"arab"), script(b"hebr"));
        let mut all: Vec<u16> = [&arab, &hebr]
            .iter()
            .flat_map(|s| s.mark.iter().chain(&s.mkmk).copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        let mut first = Vec::new();
        let mut next = 0usize;
        if let Ok(list) = font.gpos().and_then(|g| g.lookup_list()) {
            for &index in &all {
                let Ok(lookup) = list.lookups().get(usize::from(index)) else {
                    continue;
                };
                let counts: Vec<usize> = match lookup.subtables() {
                    Ok(PositionSubtables::MarkToBase(subtables)) => subtables
                        .iter()
                        .map(|t| t.map_or(0, |t| usize::from(t.mark_class_count())))
                        .collect(),
                    Ok(PositionSubtables::MarkToMark(subtables)) => subtables
                        .iter()
                        .map(|t| t.map_or(0, |t| usize::from(t.mark_class_count())))
                        .collect(),
                    _ => Vec::new(),
                };
                for (subtable, count) in counts.into_iter().enumerate() {
                    let fits = next + count <= usize::from(Anchor::OWN);
                    first.push((index, subtable, fits.then_some(next as u8)));
                    next += count;
                }
            }
        }
        let em = font
            .head()
            .map_or(1000.0, |h| f32::from(h.units_per_em().max(1)));
        Self {
            font,
            coords,
            store,
            arab,
            hebr,
            first,
            scale: 1.0 / em,
        }
    }

    /// The anchors of `ch` at `size` pixels, as a pack keeps them.
    pub(crate) fn anchors(&self, ch: char, size: f32) -> Anchors {
        let em = 1.0 / self.scale;
        let mut out = Anchors::NONE;
        let Some(script) = script_of(ch) else {
            return out;
        };
        let charmap = skrifa::MetadataProvider::charmap(&self.font);
        let Some(glyph) = charmap.map(ch).and_then(short) else {
            return out;
        };
        let px = |p: Point| -> Option<(i8, i8)> {
            let (x, y) = (p.0 * size / em, -p.1 * size / em);
            let round = |v: f32| i8::try_from(crate::raster::floor(v + 0.5)).ok();
            Some((round(x)?, round(y)?))
        };
        let push = |out: &mut Anchors, class: u8, p: Point| {
            if let Some((x, y)) = px(p) {
                out.push(Anchor { class, x, y });
            }
        };
        let script = if script { &self.arab } else { &self.hebr };
        let own = self.own(script, glyph);
        if !own.is_empty() {
            for (class, p) in own {
                push(&mut out, class | Anchor::OWN, p);
            }
            for (class, p) in self.held(&script.mkmk, glyph) {
                push(&mut out, class, p);
            }
            return out;
        }
        let mut held = self.held(&script.mark, glyph);
        if held.is_empty() && core::ptr::eq(script, &self.arab) {
            held = self.flattened(script, ch);
        }
        for (class, p) in held {
            push(&mut out, class, p);
        }
        out
    }

    /// Where an Arabic letter's form holds marks, by the glyph the font
    /// draws it from and its dots (see the module's documentation).
    fn flattened(&self, script: &Script, key: char) -> Vec<(u8, Point)> {
        let (letter, form) = arabic::letter_of(key);
        let charmap = skrifa::MetadataProvider::charmap(&self.font);
        let Some(glyph) = charmap.map(letter).and_then(short) else {
            return Vec::new();
        };
        let mut parts = self.composed(&script.ccmp, glyph);
        let Some(base) = parts.first_mut() else {
            return Vec::new();
        };
        let index = match form {
            Form::Isolated => 0,
            Form::Final => 1,
            Form::Initial => 2,
            Form::Medial => 3,
        };
        *base = self.single(&script.forms[index], *base);
        let base = *base;
        let mut held = self.held(&script.mark, base);
        for &dot in &parts[1..] {
            // The dot where it sits on the form, then its points.
            let Some((dx, dy)) = self.own(script, dot).into_iter().find_map(|(class, p)| {
                let (_, at) = held.iter().find(|(c, _)| *c == class)?;
                Some((at.0 - p.0, at.1 - p.1))
            }) else {
                continue;
            };
            for (class, p) in self.held(&script.mkmk, dot) {
                if !held.iter().any(|(c, _)| *c == class) {
                    held.push((class, (p.0 + dx, p.1 + dy)));
                }
            }
        }
        held
    }

    /// A mark's own points, in the order they are tried: those of mark to
    /// mark first.
    fn own(&self, script: &Script, glyph: GlyphId16) -> Vec<(u8, Point)> {
        let mut out = Vec::new();
        for &index in script.mkmk.iter().chain(&script.mark) {
            self.each(index, |subtable, first| {
                let found = match subtable {
                    Sub::Base(t) => (|| {
                        let i = t.mark_coverage().ok()?.get(glyph)?;
                        let marks = t.mark_array().ok()?;
                        let record = marks.mark_records().get(usize::from(i))?;
                        let anchor = record.mark_anchor(marks.offset_data()).ok()?;
                        Some((record.mark_class(), anchor))
                    })(),
                    Sub::Mark(t) => (|| {
                        let i = t.mark1_coverage().ok()?.get(glyph)?;
                        let marks = t.mark1_array().ok()?;
                        let record = marks.mark_records().get(usize::from(i))?;
                        let anchor = record.mark_anchor(marks.offset_data()).ok()?;
                        Some((record.mark_class(), anchor))
                    })(),
                };
                if let Some((class, anchor)) = found
                    && let Some(class) = number(first, class)
                    && !out.iter().any(|(c, _)| *c == class)
                {
                    out.push((class, self.point(&anchor)));
                }
            });
        }
        out
    }

    /// Where `glyph` holds marks, in `lookups` (mark to base: as a
    /// letter; mark to mark: as a mark under another).
    fn held(&self, lookups: &[u16], glyph: GlyphId16) -> Vec<(u8, Point)> {
        let mut out = Vec::new();
        for &index in lookups {
            self.each(index, |subtable, first| {
                let mut add = |class: usize, anchor: AnchorTable<'_>| {
                    if let Some(class) = u16::try_from(class).ok().and_then(|c| number(first, c))
                        && !out.iter().any(|(c, _)| *c == class)
                    {
                        out.push((class, self.point(&anchor)));
                    }
                };
                match subtable {
                    Sub::Base(t) => {
                        let (Some(i), Ok(bases)) = (
                            t.base_coverage().ok().and_then(|c| c.get(glyph)),
                            t.base_array(),
                        ) else {
                            return;
                        };
                        let Ok(record) = bases.base_records().get(usize::from(i)) else {
                            return;
                        };
                        let anchors = record.base_anchors(bases.offset_data());
                        for class in 0..usize::from(t.mark_class_count()) {
                            if let Some(Ok(anchor)) = anchors.get(class) {
                                add(class, anchor);
                            }
                        }
                    }
                    Sub::Mark(t) => {
                        let (Some(i), Ok(marks)) = (
                            t.mark2_coverage().ok().and_then(|c| c.get(glyph)),
                            t.mark2_array(),
                        ) else {
                            return;
                        };
                        let Ok(record) = marks.mark2_records().get(usize::from(i)) else {
                            return;
                        };
                        let anchors = record.mark2_anchors(marks.offset_data());
                        for class in 0..usize::from(t.mark_class_count()) {
                            if let Some(Ok(anchor)) = anchors.get(class) {
                                add(class, anchor);
                            }
                        }
                    }
                }
            });
        }
        out
    }

    /// Calls `f` with each mark subtable of GPOS lookup `index` and the
    /// number of its first class.
    fn each(&self, index: u16, mut f: impl FnMut(Sub<'a>, Option<u8>)) {
        let Ok(list) = self.font.gpos().and_then(|g| g.lookup_list()) else {
            return;
        };
        let Ok(lookup) = list.lookups().get(usize::from(index)) else {
            return;
        };
        let first = |subtable: usize| {
            self.first
                .iter()
                .find(|(l, s, _)| *l == index && *s == subtable)
                .and_then(|(_, _, first)| *first)
        };
        match lookup.subtables() {
            Ok(PositionSubtables::MarkToBase(subtables)) => {
                for (i, t) in subtables.iter().enumerate() {
                    if let Ok(t) = t {
                        f(Sub::Base(t), first(i));
                    }
                }
            }
            Ok(PositionSubtables::MarkToMark(subtables)) => {
                for (i, t) in subtables.iter().enumerate() {
                    if let Ok(t) = t {
                        f(Sub::Mark(t), first(i));
                    }
                }
            }
            _ => {}
        }
    }

    /// `glyph` and the marks the composition `lookups` add after it.
    fn composed(&self, lookups: &[u16], glyph: GlyphId16) -> Vec<GlyphId16> {
        let mut parts = alloc::vec![glyph];
        let Ok(list) = self.font.gsub().and_then(|g| g.lookup_list()) else {
            return parts;
        };
        for &index in lookups {
            let Ok(lookup) = list.lookups().get(usize::from(index)) else {
                continue;
            };
            match lookup.subtables() {
                Ok(SubstitutionSubtables::Multiple(subtables)) => {
                    for t in subtables.iter().flatten() {
                        let Some(i) = t.coverage().ok().and_then(|c| c.get(parts[0])) else {
                            continue;
                        };
                        if let Ok(sequence) = t.sequences().get(usize::from(i)) {
                            let new: Vec<GlyphId16> = sequence
                                .substitute_glyph_ids()
                                .iter()
                                .map(|g| g.get())
                                .collect();
                            if !new.is_empty() {
                                parts.splice(0..1, new);
                            }
                        }
                        break;
                    }
                }
                Ok(SubstitutionSubtables::Single(_)) => {
                    parts[0] = self.single(&[index], parts[0]);
                }
                _ => {}
            }
        }
        parts
    }

    /// `glyph` after the single substitutions of `lookups`.
    fn single(&self, lookups: &[u16], glyph: GlyphId16) -> GlyphId16 {
        let Ok(list) = self.font.gsub().and_then(|g| g.lookup_list()) else {
            return glyph;
        };
        let mut glyph = glyph;
        for &index in lookups {
            let Ok(lookup) = list.lookups().get(usize::from(index)) else {
                continue;
            };
            let subtables = match lookup.subtables() {
                Ok(SubstitutionSubtables::Single(subtables)) => subtables,
                // A form given as a sequence (of one glyph, or the glyph
                // and marks): its first glyph.
                Ok(SubstitutionSubtables::Multiple(subtables)) => {
                    for t in subtables.iter().flatten() {
                        let first = t
                            .coverage()
                            .ok()
                            .and_then(|c| c.get(glyph))
                            .and_then(|i| t.sequences().get(usize::from(i)).ok())
                            .and_then(|s| s.substitute_glyph_ids().first().map(|g| g.get()));
                        if let Some(first) = first {
                            glyph = first;
                            break;
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            for t in subtables.iter().flatten() {
                let to = match &t {
                    SingleSubst::Format1(t) => {
                        t.coverage().ok().and_then(|c| c.get(glyph)).map(|_| {
                            GlyphId16::new(glyph.to_u16().wrapping_add(t.delta_glyph_id() as u16))
                        })
                    }
                    SingleSubst::Format2(t) => t
                        .coverage()
                        .ok()
                        .and_then(|c| c.get(glyph))
                        .and_then(|i| t.substitute_glyph_ids().get(usize::from(i)))
                        .map(|g| g.get()),
                };
                if let Some(to) = to {
                    glyph = to;
                    break;
                }
            }
        }
        glyph
    }

    /// An anchor in font units, at the instance.
    fn point(&self, anchor: &AnchorTable<'_>) -> Point {
        let delta =
            |device: Option<Result<DeviceOrVariationIndex<'_>, _>>| match (&self.store, device) {
                (Some(store), Some(Ok(DeviceOrVariationIndex::VariationIndex(index)))) => {
                    let index = DeltaSetIndex {
                        outer: index.delta_set_outer_index(),
                        inner: index.delta_set_inner_index(),
                    };
                    store
                        .compute_delta(index, self.coords)
                        .map_or(0.0, |d| d.to_f64() as f32)
                }
                _ => 0.0,
            };
        (
            f32::from(anchor.x_coordinate()) + delta(anchor.x_device()),
            f32::from(anchor.y_coordinate()) + delta(anchor.y_device()),
        )
    }
}

/// A mark subtable of GPOS.
enum Sub<'a> {
    Base(read_fonts::tables::gpos::MarkBasePosFormat1<'a>),
    Mark(read_fonts::tables::gpos::MarkMarkPosFormat1<'a>),
}

/// The pack's number of a subtable's class `class`, its first `first`.
fn number(first: Option<u8>, class: u16) -> Option<u8> {
    let n = u16::from(first?).checked_add(class)?;
    u8::try_from(n).ok().filter(|&n| n < Anchor::OWN)
}

/// Whether `ch` is Arabic (`Some(true)`) or Hebrew (`Some(false)`).
fn script_of(ch: char) -> Option<bool> {
    match ch {
        '\u{600}'..='\u{6ff}'
        | '\u{750}'..='\u{77f}'
        | '\u{8a0}'..='\u{8ff}'
        | '\u{fb50}'..='\u{fdff}'
        | '\u{fe70}'..='\u{feff}' => Some(true),
        '\u{590}'..='\u{5ff}' | '\u{fb1d}'..='\u{fb4f}' => Some(false),
        _ => None,
    }
}

fn short(glyph: GlyphId) -> Option<GlyphId16> {
    u16::try_from(glyph.to_u32()).ok().map(GlyphId16::new)
}

/// The lookups of feature `feature` for `script` (its own language
/// system, else the default script's), in the order they apply.
fn lookups(
    scripts: &ScriptList<'_>,
    features: &FeatureList<'_>,
    script: Tag,
    feature: &[u8; 4],
) -> Vec<u16> {
    let feature = Tag::new(feature);
    let Some(lang_sys) = lang_sys(scripts, script) else {
        return Vec::new();
    };
    let mut out: Vec<u16> = lang_sys
        .feature_indices()
        .iter()
        .filter_map(|i| features.get(i.get()).ok())
        .filter(|f| f.tag == feature)
        .flat_map(|f| {
            f.element
                .lookup_list_indices()
                .iter()
                .map(|i| i.get())
                .collect::<Vec<_>>()
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn lang_sys<'a>(scripts: &ScriptList<'a>, script: Tag) -> Option<LangSys<'a>> {
    let index = scripts
        .index_for_tag(script)
        .or_else(|| scripts.index_for_tag(Tag::new(b"DFLT")))?;
    scripts.get(index).ok()?.element.default_lang_sys()?.ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use crate::OutlineFont;
    use crate::anchors::{self, Anchors};

    const HEBREW: &[u8] = include_bytes!("../../../tools/fonts/NotoSerifHebrew.ttf");
    const ARABIC: &[u8] = include_bytes!("../../../tools/fonts/NotoNaskhArabic.ttf");

    /// The ink of `ch` at 20 px moved by `by`: left, top, right, bottom.
    fn ink(font: &OutlineFont, ch: char, by: (i32, i32)) -> (i32, i32, i32, i32) {
        let g = font.rasterize(ch, 20.0);
        let g = g.as_ref();
        let (x, y) = (by.0 + i32::from(g.left), by.1 + i32::from(g.top));
        (x, y, x + i32::from(g.width), y + i32::from(g.height))
    }

    /// The ink of `mark` placed on `letter` by their anchors.
    fn placed(font: &OutlineFont, mark: char, letter: char) -> (i32, i32, i32, i32) {
        let by =
            anchors::place(&font.anchors(mark, 20.0), None, &font.anchors(letter, 20.0)).unwrap();
        ink(font, mark, by)
    }

    #[test]
    fn hebrew_points_sit_on_their_letters() {
        let font = OutlineFont::new(HEBREW, &[]).unwrap();
        let bet = ink(&font, '\u{5d1}', (0, 0));
        // Qamats under bet, dagesh inside it, holam over it.
        let qamats = placed(&font, '\u{5b8}', '\u{5d1}');
        assert!(
            qamats.0 >= bet.0 && qamats.2 <= bet.2 && qamats.1 >= bet.3 - 1,
            "{qamats:?} {bet:?}"
        );
        let dagesh = placed(&font, '\u{5bc}', '\u{5d1}');
        assert!(
            dagesh.0 > bet.0 && dagesh.2 < bet.2 && dagesh.1 > bet.1 && dagesh.3 < bet.3,
            "{dagesh:?} {bet:?}"
        );
        let holam = placed(&font, '\u{5b9}', '\u{5d1}');
        assert!(holam.3 <= bet.1 + 1, "{holam:?} {bet:?}");
        // Shin's dot over its right stroke.
        let shin = ink(&font, '\u{5e9}', (0, 0));
        let dot = placed(&font, '\u{5c1}', '\u{5e9}');
        assert!(
            dot.0 > (shin.0 + shin.2) / 2 && dot.3 <= shin.1 + 1,
            "{dot:?} {shin:?}"
        );
    }

    #[test]
    fn arabic_marks_sit_on_the_form_and_under_its_dots() {
        let font = OutlineFont::new(ARABIC, &[]).unwrap();
        let (initial, kasra, fatha, shadda) = ('\u{fe91}', '\u{650}', '\u{64e}', '\u{651}');
        // The initial form holds its marks elsewhere than the letter alone.
        assert_ne!(font.anchors(initial, 20.0), font.anchors('\u{628}', 20.0));
        // Kasra under beh's dot: under the ink of the whole letter.
        let beh = ink(&font, initial, (0, 0));
        let under = placed(&font, kasra, initial);
        assert!(
            under.1 >= beh.3 - 1 && under.0 < beh.2 && under.2 > beh.0,
            "{under:?} {beh:?}"
        );
        let over = placed(&font, fatha, initial);
        assert!(over.3 <= beh.1 + 1, "{over:?} {beh:?}");
        // Fatha on shadda, higher than it (the font nests it a little into
        // shadda's top).
        let held = font.anchors(shadda, 20.0);
        let on = anchors::place(&held, None, &font.anchors(initial, 20.0)).unwrap();
        let by = anchors::place(
            &font.anchors(fatha, 20.0),
            Some((&held, on)),
            &Anchors::NONE,
        )
        .unwrap();
        let (low, high) = (ink(&font, shadda, on), ink(&font, fatha, by));
        assert!(high.1 < low.1 && high.3 < low.3, "{high:?} {low:?}");
    }

    #[test]
    fn other_scripts_have_none() {
        let font = OutlineFont::new(HEBREW, &[]).unwrap();
        for ch in ['a', '\u{301}', ' ', '\u{10ffff}'] {
            assert!(font.anchors(ch, 20.0).is_empty(), "{ch}");
        }
    }
}
