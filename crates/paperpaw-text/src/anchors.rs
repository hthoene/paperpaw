//! Where marks sit on letters: the anchors table of a font pack (see
//! [`BitmapFont::ANCHORS`](crate::BitmapFont::ANCHORS)), read in place or
//! through a [`PackSource`](crate::PackSource), and where a mark is drawn
//! by them.
//!
//! A key's anchors are points in pixels from its pen and the baseline (y
//! down), each of a class. A mark has its own points, where it is held
//! ([`Anchor::OWN`] set in the class); a letter (or a mark that others sit
//! on) has the points where marks of a class are held. A mark goes where
//! its own point of a class meets the same class's point on what it sits
//! on: the mark before it where that one has the class, else its letter.
//! A mark lists its own points in the order they are tried.

/// The most anchors a key keeps; a pack's further ones are not read.
pub(crate) const MAX: usize = 12;
/// The bytes of a key's record in a strike's part.
pub(crate) const KEY: usize = 12;
/// The bytes of an anchor.
const ANCHOR: usize = 3;
/// The anchors table's head: version, three reserved bytes, length.
const HEAD: usize = 8;

/// One point of a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Anchor {
    /// The class, [`Anchor::OWN`] set for a mark's own point.
    pub(crate) class: u8,
    pub(crate) x: i8,
    pub(crate) y: i8,
}

impl Anchor {
    /// In a class: the point is the mark's own, where it is held.
    pub(crate) const OWN: u8 = 0x80;
}

/// A key's anchors (none for most keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Anchors {
    list: [Anchor; MAX],
    len: u8,
}

impl Anchors {
    pub(crate) const NONE: Anchors = Anchors {
        list: [Anchor {
            class: 0,
            x: 0,
            y: 0,
        }; MAX],
        len: 0,
    };

    /// Adds `anchor`; past [`MAX`] it is left out.
    pub(crate) fn push(&mut self, anchor: Anchor) {
        if let Some(slot) = self.list.get_mut(usize::from(self.len)) {
            *slot = anchor;
            self.len += 1;
        }
    }

    pub(crate) fn as_slice(&self) -> &[Anchor] {
        &self.list[..usize::from(self.len).min(MAX)]
    }

    #[cfg(any(test, feature = "outline"))]
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// A mark's own points, in the order they are tried: class (without
    /// [`Anchor::OWN`]), x, y.
    fn own(&self) -> impl Iterator<Item = (u8, i32, i32)> + '_ {
        self.as_slice()
            .iter()
            .filter(|a| a.class & Anchor::OWN != 0)
            .map(|a| (a.class & !Anchor::OWN, i32::from(a.x), i32::from(a.y)))
    }

    /// Where a mark of `class` is held on this key.
    fn held(&self, class: u8) -> Option<(i32, i32)> {
        self.as_slice()
            .iter()
            .find(|a| a.class == class)
            .map(|a| (i32::from(a.x), i32::from(a.y)))
    }

    /// The class of the first own point, 255 for none (a letter).
    #[cfg(any(test, feature = "outline"))]
    fn attach(&self) -> u8 {
        self.own().next().map_or(255, |(class, _, _)| class)
    }

    /// Reads `count` anchors from `bytes` (three each).
    fn read(bytes: &[u8], count: usize) -> Anchors {
        let mut anchors = Anchors::NONE;
        for a in bytes.as_chunks::<ANCHOR>().0.iter().take(count.min(MAX)) {
            anchors.push(Anchor {
                class: a[0],
                x: a[1] as i8,
                y: a[2] as i8,
            });
        }
        anchors
    }
}

/// Where `mark` (with `anchors`) is drawn: its pen and baseline offset
/// from the pen and baseline of what it sits on. `before` is the mark
/// drawn before it on the same letter (its anchors and place), `letter`
/// the letter's anchors (its pen at 0, 0). `None` where no class meets:
/// the caller places it otherwise.
pub(crate) fn place(
    mark: &Anchors,
    before: Option<(&Anchors, (i32, i32))>,
    letter: &Anchors,
) -> Option<(i32, i32)> {
    if let Some((on, (px, py))) = before {
        for (class, mx, my) in mark.own() {
            if let Some((x, y)) = on.held(class) {
                return Some((px + x - mx, py + y - my));
            }
        }
    }
    mark.own()
        .find_map(|(class, mx, my)| letter.held(class).map(|(x, y)| (x - mx, y - my)))
}

/// Whether `anchors` holds a mark's own point: it is placed by anchors.
pub(crate) fn is_held(anchors: &Anchors) -> bool {
    anchors.own().next().is_some()
}

/// The body of an anchors table (without its head) for strikes whose
/// keys and anchors are given in order of the keys.
#[cfg(any(test, feature = "outline"))]
pub(crate) fn build(strikes: &[alloc::vec::Vec<(char, Anchors)>]) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec![0; strikes.len() * 4];
    for (i, keys) in strikes.iter().enumerate() {
        let part = (HEAD + out.len()) as u32;
        out[i * 4..i * 4 + 4].copy_from_slice(&part.to_le_bytes());
        out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
        let records = out.len();
        out.resize(records + keys.len() * KEY, 0);
        for (k, (key, anchors)) in keys.iter().enumerate() {
            let at = (HEAD + out.len()) as u32;
            for a in anchors.as_slice() {
                out.extend_from_slice(&[a.class, a.x as u8, a.y as u8]);
            }
            let record = records + k * KEY;
            out[record..record + 4].copy_from_slice(&u32::from(*key).to_le_bytes());
            out[record + 4..record + 8].copy_from_slice(&at.to_le_bytes());
            out[record + 8] = anchors.len;
            out[record + 9] = anchors.attach();
        }
    }
    out
}

/// Reads bytes of an anchors table: `len` from `at` (from the table's
/// start) into `buf`, if it has them.
pub(crate) trait Bytes {
    fn bytes(&self, at: usize, buf: &mut [u8]) -> bool;
}

impl Bytes for [u8] {
    fn bytes(&self, at: usize, buf: &mut [u8]) -> bool {
        let Some(src) = at.checked_add(buf.len()).and_then(|end| self.get(at..end)) else {
            return false;
        };
        buf.copy_from_slice(src);
        true
    }
}

fn u32_of(b: &[u8]) -> u32 {
    b.first_chunk::<4>().map_or(0, |b| u32::from_le_bytes(*b))
}

/// The anchors of `key` in strike `strike` of the anchors `table`; `hint`
/// is where the key's record may be (its glyph's index in the strike: a
/// part made for every glyph has them at those places). Nothing for bytes
/// that are not a table of version 1.
pub(crate) fn find(
    table: &(impl Bytes + ?Sized),
    strike: usize,
    key: char,
    hint: usize,
) -> Anchors {
    let mut head = [0u8; HEAD];
    if !table.bytes(0, &mut head) || head[0] != 1 {
        return Anchors::NONE;
    }
    let mut word = [0u8; 4];
    let Some(at) = strike.checked_mul(4).and_then(|s| s.checked_add(HEAD)) else {
        return Anchors::NONE;
    };
    if !table.bytes(at, &mut word) {
        return Anchors::NONE;
    }
    let part = u32_of(&word) as usize;
    if !table.bytes(part, &mut word) {
        return Anchors::NONE;
    }
    let count = u32_of(&word) as usize;
    let records = part.saturating_add(4);
    let mut record = [0u8; KEY];
    let read = |index: usize, record: &mut [u8; KEY]| {
        index < count
            && index
                .checked_mul(KEY)
                .and_then(|r| r.checked_add(records))
                .is_some_and(|at| table.bytes(at, record))
    };
    let target = u32::from(key);
    let mut found = read(hint, &mut record) && u32_of(&record) == target;
    if !found {
        let (mut low, mut high) = (0usize, count);
        while low < high {
            let mid = low + (high - low) / 2;
            if !read(mid, &mut record) {
                return Anchors::NONE;
            }
            match u32_of(&record).cmp(&target) {
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
                core::cmp::Ordering::Equal => {
                    found = true;
                    break;
                }
            }
        }
    }
    if !found {
        return Anchors::NONE;
    }
    let n = usize::from(record[8]).min(MAX);
    let mut bytes = [0u8; MAX * ANCHOR];
    let Some(bytes) = bytes.get_mut(..n * ANCHOR) else {
        return Anchors::NONE;
    };
    if !table.bytes(u32_of(&record[4..]) as usize, bytes) {
        return Anchors::NONE;
    }
    Anchors::read(bytes, n)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    fn anchors(list: &[(u8, i8, i8)]) -> Anchors {
        let mut a = Anchors::NONE;
        for &(class, x, y) in list {
            a.push(Anchor { class, x, y });
        }
        a
    }

    /// The table with its head, as a pack holds it.
    fn table(strikes: &[Vec<(char, Anchors)>]) -> Vec<u8> {
        let body = build(strikes);
        let mut t = alloc::vec![1, 0, 0, 0];
        t.extend_from_slice(&((HEAD + body.len()) as u32).to_le_bytes());
        t.extend_from_slice(&body);
        t
    }

    #[test]
    fn keys_are_found_with_and_without_the_hint() {
        let letter = anchors(&[(0, 5, -10), (1, 4, 2)]);
        let mark = anchors(&[(Anchor::OWN, 2, -8), (2, 2, -12)]);
        let t = table(&[
            alloc::vec![('a', Anchors::NONE), ('b', letter), ('\u{5b8}', mark)],
            alloc::vec![('b', anchors(&[(0, 9, -20)]))],
        ]);
        assert_eq!(find(&t[..], 0, 'b', 1), letter);
        assert_eq!(find(&t[..], 0, 'b', 0), letter);
        assert_eq!(find(&t[..], 0, '\u{5b8}', 99), mark);
        assert_eq!(find(&t[..], 1, 'b', 0).as_slice()[0].x, 9);
        assert!(find(&t[..], 0, 'c', 0).is_empty());
        assert!(find(&t[..], 2, 'b', 0).is_empty());
        assert_eq!(mark.attach(), 0);
        assert_eq!(letter.attach(), 255);
    }

    #[test]
    fn marks_sit_on_their_letter_or_on_the_mark_before() {
        let letter = anchors(&[(0, 6, -10), (1, 5, 1)]);
        let above = anchors(&[(Anchor::OWN, 2, -1), (2, 2, -6)]);
        let second = anchors(&[(Anchor::OWN | 2, 1, 0), (Anchor::OWN, 1, 0)]);
        let below = anchors(&[(Anchor::OWN | 1, 3, 0)]);
        assert_eq!(place(&above, None, &letter), Some((4, -9)));
        // On the mark before, where that one holds its class.
        assert_eq!(
            place(&second, Some((&above, (4, -9))), &letter),
            Some((5, -15))
        );
        // Not on it where it holds none of the mark's classes.
        assert_eq!(
            place(&below, Some((&above, (4, -9))), &letter),
            Some((2, 1))
        );
        assert_eq!(place(&below, None, &Anchors::NONE), None);
        assert!(is_held(&below) && !is_held(&letter));
    }

    #[test]
    fn broken_tables_find_nothing() {
        let t = table(&[alloc::vec![('b', anchors(&[(0, 5, -10)]))]]);
        for cut in 0..t.len() {
            let _ = find(&t[..cut], 0, 'b', 0);
        }
        let mut bad = t.clone();
        bad[0] = 2;
        assert!(find(&bad[..], 0, 'b', 0).is_empty());
        let mut huge = t.clone();
        huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(find(&huge[..], 0, 'b', 0).is_empty());
        let mut count = t;
        count[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        let _ = find(&count[..], 0, 'b', usize::MAX);
    }
}
