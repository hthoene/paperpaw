//! One catalog line seen without holding it: a book's path and numbers read
//! into fixed buffers on the stack (a line can be 16 KiB), with the whole
//! lengths of its texts and the counts of the tags and fields; a text
//! itself (title, author, series, a tag, a field) is read from a byte on
//! into the caller's buffer by [`try_text_at`], so no frame here holds
//! more than the path and one chunk (about 0.9 KiB). The memory error is
//! [`Error::OutOfMemory`].
























