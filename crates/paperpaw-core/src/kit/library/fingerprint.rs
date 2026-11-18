//! What a book is by its content, apart from where it lies: its size and
//! KOReader's partial MD5 ([`md5::partial`]). The path's hash
//! ([`key`](super::key)) names a book's files; the fingerprint, kept in
//! them beside it, says which file they were made of, so the same book
//! is known again where it was renamed or moved by other means than
//! PaperPaw, and another file at the same path is told from it.
//!
//! Reading it takes 12 reads of 1 KiB at most, whatever the book's size:
//! a reader takes it once when it opens a book and keeps it while it is
//! open.

pub use paperpaw_sdk::library::Fingerprint;

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_digest::md5;
    use paperpaw_sdk::MemoryStorage;

    /// The size and KOReader's partial MD5, as text and back; text that
    /// is no fingerprint reads as none.
    #[test]
    fn a_fingerprint_is_the_size_and_the_partial_md5() {
        let data: alloc::vec::Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let storage = MemoryStorage::new().with("books/b.epub", data);
        let found = Fingerprint::of(&storage, "books/b.epub").unwrap();
        assert_eq!(found.size, 5000);
        assert_eq!(
            Some(found.md5.as_str()),
            md5::partial(&storage, "books/b.epub").as_deref()
        );
        assert_eq!(Fingerprint::parse(&found.text()), Some(found.clone()));
        assert_eq!(Fingerprint::of(&storage, "books/none.epub"), None);
        for bad in [
            "",
            "5000",
            "x 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b",
            "5000 0b6e",
            "5000 zz6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b",
        ] {
            assert_eq!(Fingerprint::parse(bad), None, "{bad}");
        }
    }
}
