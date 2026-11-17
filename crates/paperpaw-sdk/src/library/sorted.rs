//! Every book of the library sorted by title, kept on the card and read a
//! page at a time, so that a list of any length takes a few kilobytes
//! (the X4 holds no more than 2,000 books of an `Index`).
//!
//! The kernel makes the file of a view's archive folder (one for each,
//! for two at most) after the catalog was written anew, or when a
//! view asks for it (`request`): every book but those in the archive
//! folder, sorted by its title's first letters
//! (`sort_key`) and where its line starts, each
//! with its series (a hash of the name), its place among the books of
//! its series and how many there are. It is written to a file beside it
//! and put in place whole, its end saying of which library it is
//! (`Library::version`), how many
//! books it has and a check of them: a file of another version, cut
//! short or damaged is not read (`Sorted::open` gives `None`), and the
//! view says the library is being prepared until the kernel made it
//! anew.
//!
//! A view opens it once it shows, reads it through once (how many books
//! each filter shows in each block of it, the books read last, the
//! books being read) and then pages it (`Sorted::page`): what changed
//! since it was made (how far a book is read, favourites, books removed;
//! the catalog's log) is taken in as it is read.

use alloc::string::String;
use alloc::vec::Vec;

use super::index::self;
use crate::Storage;

#[cfg(any(test, feature = "kernel"))]
mod build;


















#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heapsort_sorts_as_the_standard_library_does() {
        let mut seed = 0x2545_f491_u32;
        for len in (0..40).chain([255, 256, 257, 1000]) {
            let v: Vec<(u8, [u8; KEY], u32)> = (0..len)
                .map(|n| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let key = (seed % 7).to_be_bytes();
                    (
                        (seed % 3) as u8,
                        [0, 0, 0, 0, key[0], key[1], key[2], key[3]],
                        n,
                    )
                })
                .collect();
            let mut ours = v.clone();
            sort_by(&mut ours, |&(first, key, offset)| {
                ranked(first, key, offset)
            });
            let mut theirs = v;
            theirs.sort_unstable();
            assert_eq!(ours, theirs, "{len}");
        }
    }
}
