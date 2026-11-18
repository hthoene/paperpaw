//! A pack's kerning table: pairs of characters and how much they move
//! the second one, kept as classes, read in place.
//!
//! Characters that kern alike share a class, as fonts keep them (all the
//! accented forms of `A` kern as `A` does): a table of classes takes a
//! sixth of the space a list of the same pairs would.
//!
//! ```text
//! header   8 bytes   character count n u16, left classes u16,
//!                    right classes u16, reserved u16
//! chars    n u16     code points (below U+10000), sorted
//! left     n u16     each character's class on the left (0: none)
//! right    n u16     each character's class on the right (0: none)
//! matrix   i16       for each left class, for each right class: the
//!                    adjustment in 1/1024 em
//! ```

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Adjustments are kept in this many parts of an em.
pub(crate) const UNITS: f32 = 1024.0;

const HEADER: usize = 8;

/// The most cells a table may hold (at two bytes each): a font kerning
/// more differently than this gets no table.
const MAX_CELLS: usize = 256 * 1024;

/// The table for `pairs` (left, right, adjustment in ems); empty where
/// no pair changes anything.
pub(crate) fn build(pairs: &[(char, char, f32)]) -> Vec<u8> {
    let units = |em: f32| {
        let scaled = em * UNITS;
        let rounded = if scaled < 0.0 {
            scaled - 0.5
        } else {
            scaled + 0.5
        };
        rounded.clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
    };
    let mut kept: Vec<(u16, u16, i16)> = pairs
        .iter()
        .filter_map(|&(l, r, em)| {
            let (l, r) = (u16::try_from(l as u32).ok()?, u16::try_from(r as u32).ok()?);
            Some((l, r, units(em))).filter(|p| p.2 != 0)
        })
        .collect();
    paperpaw_sort::sort(&mut kept);
    kept.dedup_by_key(|p| (p.0, p.1));
    if kept.is_empty() {
        return Vec::new();
    }
    let mut chars: Vec<u16> = kept.iter().flat_map(|p| [p.0, p.1]).collect();
    paperpaw_sort::sort(&mut chars);
    chars.dedup();
    let index = |c: u16| chars.binary_search(&c).unwrap_or(0);
    // Each character's row (what it does on the left) and column.
    let mut rows: Vec<Vec<(u16, i16)>> = alloc::vec![Vec::new(); chars.len()];
    let mut columns: Vec<Vec<(u16, i16)>> = alloc::vec![Vec::new(); chars.len()];
    for &(l, r, v) in &kept {
        rows[index(l)].push((r, v));
        columns[index(r)].push((l, v));
    }
    let (left, left_count) = classes(&rows);
    let (right, right_count) = classes(&columns);
    let cells = usize::from(left_count) * usize::from(right_count);
    if cells > MAX_CELLS {
        return Vec::new();
    }
    let mut matrix = alloc::vec![0i16; cells];
    for &(l, r, v) in &kept {
        let (lc, rc) = (left[index(l)], right[index(r)]);
        let at = usize::from(lc - 1) * usize::from(right_count) + usize::from(rc - 1);
        matrix[at] = v;
    }
    let mut out = Vec::with_capacity(HEADER + chars.len() * 6 + cells * 2);
    // At most as many characters as code points below U+10000.
    out.extend_from_slice(&(chars.len() as u16).to_le_bytes());
    out.extend_from_slice(&left_count.to_le_bytes());
    out.extend_from_slice(&right_count.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    for list in [&chars, &left, &right] {
        for value in list.iter() {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    for value in matrix {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

/// A class for each of `lines` (rows or columns), the same for the same
/// line, 0 for an empty one; and how many classes there are.
fn classes(lines: &[Vec<(u16, i16)>]) -> (Vec<u16>, u16) {
    let mut seen: BTreeMap<&[(u16, i16)], u16> = BTreeMap::new();
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        if line.is_empty() {
            out.push(0);
            continue;
        }
        let next = seen.len() as u16 + 1;
        out.push(*seen.entry(line.as_slice()).or_insert(next));
    }
    (out, seen.len() as u16)
}

/// The adjustment of the pair `left`, `right` in the table at `at` of
/// `data`, in 1/1024 em; 0 for none, also where the table is damaged.
pub(crate) fn lookup(data: &[u8], at: usize, left: char, right: char) -> i16 {
    let (Ok(left), Ok(right)) = (u16::try_from(left as u32), u16::try_from(right as u32)) else {
        return 0;
    };
    let read = |offset: usize| -> Option<u16> {
        let start = at.checked_add(offset)?;
        let bytes = data.get(start..start.checked_add(2)?)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    };
    let find = |n: usize, c: u16| -> Option<usize> {
        let (mut low, mut high) = (0usize, n);
        while low < high {
            let mid = (low + high) / 2;
            match read(HEADER + mid * 2)?.cmp(&c) {
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
                core::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    };
    let value = || -> Option<i16> {
        let n = usize::from(read(0)?);
        let (lefts, rights) = (read(2)?, read(4)?);
        let lc = read(HEADER + n * 2 + find(n, left)? * 2)?;
        if lc == 0 || lc > lefts {
            return None;
        }
        let rc = read(HEADER + n * 4 + find(n, right)? * 2)?;
        if rc == 0 || rc > rights {
            return None;
        }
        let cell = usize::from(lc - 1) * usize::from(rights) + usize::from(rc - 1);
        Some(read(HEADER + n * 6 + cell * 2)? as i16)
    };
    value().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_come_back_from_their_classes() {
        let em = |units: i16| f32::from(units) / UNITS;
        let pairs = [
            ('A', 'V', em(-110)),
            ('\u{c0}', 'V', em(-110)),
            ('A', 'W', em(-90)),
            ('\u{c0}', 'W', em(-90)),
            ('T', 'o', em(-70)),
            ('T', 'e', em(-70)),
            ('V', 'A', em(-100)),
            ('f', 'f', 0.0),
            ('\u{1f600}', 'A', em(5)),
        ];
        let table = build(&pairs);
        // Two left classes alike (A, À), so fewer cells than pairs.
        assert!(table.len() < 8 + 7 * 6 + 4 * 4 * 2, "{}", table.len());
        let mut data = alloc::vec![0xAA; 3];
        data.extend_from_slice(&table);
        for (l, r, v) in pairs {
            let expected = if (l as u32) < 0x10000 {
                (v * UNITS) as i16
            } else {
                0
            };
            assert_eq!(lookup(&data, 3, l, r), expected, "{l}{r}");
        }
        assert_eq!(lookup(&data, 3, 'V', 'o'), 0);
        assert_eq!(lookup(&data, 3, 'x', 'y'), 0);
        // Damaged: cut short, or read from the wrong place, never a panic.
        for at in 0..data.len() {
            for (l, r, _) in pairs {
                lookup(&data[..at], 3, l, r);
                lookup(&data, at, l, r);
            }
        }
        assert!(build(&[('a', 'b', 0.0)]).is_empty());
    }
}
