//! What the library's walk reads of a new book: its metadata alone
//! (title, author, series, tags), not its cover, which takes a picture's
//! decoding (seconds for a large one on the X4) and its writing. The
//! details say so (`metadata = on`, `indexed = off`): the walk takes them
//! as they are next time, and a view that shows the book reads its cover
//! when it first does ([`index`](super::index()), one book at a time), as
//! for any book not indexed yet.









