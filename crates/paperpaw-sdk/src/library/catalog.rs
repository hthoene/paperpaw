//! The catalog: every book under the library folder with what the
//! library knows of it (author, title, series, number, tags, how far it
//! is read, whether it is a favourite or was opened, when it was added,
//! the fields plugins keep in it), for views of the library, its filters
//! and searches, without reading each book's details.
//!
//! It is kept in `.paperpaw/library/catalog.txt`, a book a line, and only
//! the kernel writes it, one change at a time, in the background: made
//! anew from the library folder (`Walk`) when the library is first shown
//! after a start, when books came or went without a word and for another
//! library folder, taking each book whose file did not change from the
//! old catalog, others from their details, opening only books not seen
//! before; caught up (`Rewrite`) after books came or moved. Until a new
//! one is complete, the old one is read; where there is none yet (a new
//! card, thousands of books), the walk puts the books it found so far
//! into one at checkpoints, which views hear of as of a catalog made, so
//! they can be read at once. It is read a piece at a time, never held in
//! memory.
//!
//! How far a book is read, whether it is a favourite, and that a book was
//! removed change while the catalog stands: they are appended to a short
//! log beside it (`status.log`) and read with it ([`Changes`]), and
//! folded into it at a calm moment once the log is long, so a page turned
//! never rewrites the catalog. What was noted before a catalog was made
//! is in it, and goes from the log.
//!
//! The first line of a catalog of this format is a header (`\tcatalog\t2`
//! and its [`generation`]); a catalog of format 1 (six columns, no
//! status) is still read.













// ---- Reading the catalog ----------------------------------------------






















// ---- Status changes ---------------------------------------------------








// ---- Queries ----------------------------------------------------------






mod fallible;
mod casing;
mod scan;
mod view;

#[cfg(any(test, feature = "kernel"))]
mod making;
#[cfg(any(test, feature = "kernel"))]
mod publish;

#[cfg(test)]
mod tests;
