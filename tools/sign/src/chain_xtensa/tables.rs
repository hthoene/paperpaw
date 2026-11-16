//! Where a jump table sends the flow: the places of a function that another
//! path enters.

use std::collections::BTreeSet;

use super::Image;

/// The fewest words in a jump table: LLVM's default `min-jump-table-entries`.
const JUMP_TABLE_MIN: usize = 4;

impl Image<'_> {
    /// Every address inside a function that a jump table holds: a run of at
    /// least [`JUMP_TABLE_MIN`] words next to each other outside the
    /// functions (data, or the literal pools between them), all inside one
    /// function. That is how LLVM lays out a jump table (it makes none under
    /// four entries, and its words are all in the function's own code). A
    /// number or an address that a lone word of the data happens to equal
    /// is no table: where such words fall depends on where the build puts
    /// everything, and an entry for one made the result of the scan follow
    /// the layout (the same function resolved fewer calls after unrelated code
    /// moved, #271).
    pub(super) fn pointed(&self) -> BTreeSet<u32> {
        let mut found = BTreeSet::new();
        let mut take = |base: u32, bytes: &[u8], skip_functions: bool| {
            let first = (4 - base % 4) % 4;
            let Some(bytes) = bytes.get(first as usize..) else {
                return;
            };
            // The words of the run so far, and the function they are in.
            let mut run: Vec<u32> = Vec::new();
            let mut of = usize::MAX;
            let mut flush = |run: &mut Vec<u32>| {
                if run.len() >= JUMP_TABLE_MIN {
                    found.extend(run.iter().copied());
                }
                run.clear();
            };
            for (n, word) in bytes.as_chunks::<4>().0.iter().enumerate() {
                let value = u32::from_le_bytes(*word);
                let inside = (!skip_functions || self.at(base + first + 4 * n as u32).is_none())
                    .then(|| self.at(value))
                    .flatten();
                match inside {
                    Some(f) if f == of => run.push(value),
                    Some(f) => {
                        flush(&mut run);
                        of = f;
                        run.push(value);
                    }
                    None => {
                        flush(&mut run);
                        of = usize::MAX;
                    }
                }
            }
            flush(&mut run);
        };
        for (base, bytes) in &self.data {
            take(*base, bytes, false);
        }
        for (base, bytes) in &self.exec {
            take(*base, bytes, true);
        }
        found
    }
}
