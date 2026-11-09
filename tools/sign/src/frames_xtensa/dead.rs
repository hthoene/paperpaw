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
//!
//! The limits of this rule. It guards against what a compiler and linker make,
//! not against a hand-made file (the scan as a whole is no sandbox): a file
//! made to hide code can hide it in a literal an `l32r` reads, too. The cap of
//! [`MAX_BYTES`] is a tripwire, not a budget (the real plugins have two or
//! three dead words). "Loaded" means a section with the `SHF_ALLOC` flag: the
//! relocations and words of the other sections (debug information) are not
//! references. A relocation section that names no section it relocates is
//! counted (fails closed). A word that decodes as `entry`, a return, `jx` or
//! `callx` is refused whatever else is true ([`looks_like_control`]).
//!
//! What this does not see: an address built at run time (`l32r` and `addi`
//! into `.text`) and a PC-relative jump table. A compiler sends those into
//! function bodies, not into a literal pool, and the S3 has neither `LITBASE`
//! nor FLIX; a relocation (`--emit-relocs`) names every static reference
//! anyway. A word that starts a gap is let through only after a function that
//! ends in a return, a jump or `ill` (the function does not run on into it).

/// Whether the code before `end` ends where control does not go on: `ret`,
/// `retw`, `ret.n`, `retw.n`, `ill` or `j`.
pub(crate) fn ends_control(code: &[u8], end: usize) -> bool {
    let tail = &code[..end.min(code.len())];
    tail.ends_with(&[0x1d, 0xf0])
        || tail.ends_with(&[0x0d, 0xf0])
        || tail.ends_with(&[0x80, 0x00, 0x00])
        || tail.ends_with(&[0x90, 0x00, 0x00])
        || tail.ends_with(&[0x00, 0x00, 0x00])
        || tail
            .len()
            .checked_sub(3)
            .is_some_and(|n| tail[n] & 0x3F == 0x06)
}

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
            let (n, m, r) = ((w >> 4) & 3, (w >> 6) & 3, (w >> 12) & 0xF);
            match n {
                0 => Some(at + 4 + sign(w >> 6, 18)),
                1 => Some(at + 4 + sign(w >> 12, 12)),
                // `entry` has no target.
                3 if m == 0 => None,
                // `loop`, `loopnez`, `loopgtz`: the offset is unsigned.
                3 if m == 1 && (8..=10).contains(&r) => Some(at + 4 + i64::from(w >> 16)),
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

/// Whether any 3-byte window of `word` decodes as `entry` (op0 6, n 3, m 0), as
/// `ret`, `retw`, `jx` or `callx*` (RST0 `st0` with `r` 0), or any 2-byte one
/// as `ret.n` or `retw.n`: a word that reads as control flow is never let
/// through as a constant (cheap, and it costs nothing for a real constant).
pub(crate) fn looks_like_control(word: [u8; 4]) -> bool {
    let wide = (0..=1).any(|n| {
        let w = u32::from(word[n]) | u32::from(word[n + 1]) << 8 | u32::from(word[n + 2]) << 16;
        let (op0, t, r) = (w & 0xF, (w >> 4) & 0xF, (w >> 12) & 0xF);
        let rst0 = op0 == 0 && (w >> 16) == 0 && r == 0;
        (w & 0xFF == 0x36) || (rst0 && (t == 0 || t >> 2 >= 2))
    });
    wide || (0..=2).any(|n| matches!(word[n..n + 2], [0x0d | 0x1d, 0xf0]))
}
