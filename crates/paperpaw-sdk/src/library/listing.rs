//! Sorted, paged listings of the books and folders of a folder, that
//! never hold more than a page.
//!
//! Folders with thousands of books must work on a device with a few
//! hundred kilobytes of RAM, so a listing is never loaded completely.
//! Pages are found by *keyset pagination*: one pass over the directory
//! keeps only the `n` entries that follow (or precede) a known entry in
//! sort order. Each step costs a directory scan but only `n` entries of
//! memory.
