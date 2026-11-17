//! The catalog and its log read without an infallible allocation and in
//! about 1.5 KiB of stack at most (`each_head`: a 512-byte chunk and 520
//! bytes of line head; `try_path_at` adds one more head). Lines are looked
//! at by their first bytes only (a catalog line starts with the book's
//! path, at most 512 bytes; a log line is short), so no line is ever held
//! whole. A device short of memory gets [`Error::OutOfMemory`] instead of
//! an abort, and nothing is returned half read.












