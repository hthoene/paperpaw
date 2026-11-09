//! A dead word in a literal pool (Kernel ruling, todo 186): the toolchain
//! leaves a constant in a literal pool that no `l32r` reads any more
//! (`0xff00`, a constant that does not fit `movi`, after the code that used it
//! was folded away). It is data nothing can reach, so the scan lets it be, and
//! says so; anything else between functions that no `l32r` reads stays code
//! that no function covers (refused).
//!
//! A word is let through only if **all** of this holds:
//!
//! 1. it lies in a literal region: a gap that is followed by a function (the
//!    literal pool before a function or before the first function symbol),
//!    never inside a function's range or after the last function;
//! 2. it is a whole word on a 4-byte boundary inside the gap;
//! 3. nothing reaches any byte of it: no relocation resolves into it
//!    (`S + A`), no word of the image holds an address in it, no call, jump
//!    or branch of a scanned function lands in it (and no `l32r` reads it:
//!    then it is a literal and not uncovered at all);
//! 4. the image has at most [`MAX_BYTES`] such bytes: past that, a plugin is
//!    refused as before (a tripwire, not a budget).
//!
//! The word is never scanned as code (not a frame, an entry or a start).

/// The most bytes of dead words one image may have.
pub const MAX_BYTES: usize = 64;

/// What the rest of the image points at, as `.text` offsets, sorted.
#[derive(Debug, Default, Clone)]
pub struct Reach {
    /// Relocation targets (`S + A`) and the words of the image that hold an
    /// address inside `.text`.
    pub pointed: Vec<usize>,
}

/// Whether some value of the sorted `values` is in `word..word + 4`.
pub(crate) fn hits(values: &[usize], word: usize) -> bool {
    let n = values.partition_point(|&v| v < word);
    values.get(n).is_some_and(|&v| v < word + 4)
}

/// The `.text` offset a 24-bit call, jump or branch at `at` goes to (the
/// word `w`, little endian), if it is one: `call*` (op0 5), `j`, `beqz`...,
/// `beqi`..., `bltui`/`bgeui`, `bt`/`bf`, `loop*` (op0 6, but not `entry`) and
/// the compare-and-branch forms (op0 7). A loop's end is the word after its
/// body: counted as a target, too.
pub(crate) fn wide_target(at: usize, w: u32) -> Option<i64> {
    let at = at as i64;
    let sign = |value: u32, bits: u32| i64::from((value << (32 - bits)) as i32 >> (32 - bits));
    match w & 0xF {
        // CALLN: the word the call lands on.
        5 => Some((at & !3) + 4 + (sign(w >> 6, 18) << 2)),
        6 => {
            let (n, m) = ((w >> 4) & 3, (w >> 6) & 3);
            match n {
                0 => Some(at + 4 + sign(w >> 6, 18)),
                1 => Some(at + 4 + sign(w >> 12, 12)),
                // `entry` has no target.
                3 if m == 0 => None,
                _ => Some(at + 4 + sign(w >> 16, 8)),
            }
        }
        7 => Some(at + 4 + sign(w >> 16, 8)),
        _ => None,
    }
}

/// The `.text` offset a 16-bit `beqz.n` or `bnez.n` at `at` goes to.
pub(crate) fn narrow_target(at: usize, w: u32) -> Option<i64> {
    let (t, r) = ((w >> 4) & 0xF, (w >> 12) & 0xF);
    (w & 0xF == 0xC && t & 8 != 0).then(|| at as i64 + 4 + i64::from(((t & 3) << 4) | r))
}
