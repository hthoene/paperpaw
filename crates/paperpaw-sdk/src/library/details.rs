//! The text of a book's details file, built without an infallible
//! allocation and not at all where it would be over what is read back:
//! the same text as [`config::set_all`] makes from the entries (a key set
//! twice keeps its first place and its last value; a section or key a file
//! would not parse is left out; entries without a section first, then each
//! section in the order its first key came), but a device short of memory
//! gets [`Error::OutOfMemory`] instead of an abort, and the size is
//! counted, and compared with [`MAX_DETAILS`], before the text is made.















