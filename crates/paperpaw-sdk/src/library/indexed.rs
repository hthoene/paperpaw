//! Whether a book's details say it was read from the book, without
//! reading the details: the file is scanned a piece at a time through a
//! line buffer of 128 bytes (the keys that matter are short, and a long
//! line, a field's value, is skipped), so nothing here is allocated
//! beyond the two file names and nothing of the file is held (the details
//! can be 16 KiB, and [`Book`](super::Book) as much again).







