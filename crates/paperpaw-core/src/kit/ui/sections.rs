//! Entries under the headings of their sections, on pages (see
//! [`Sections`]), in memory that does not grow with the entries: only how
//! many each section has is kept, and where the entries of a page are is
//! worked out when asked. For lists of any length whose entries are read
//! a page at a time, e.g. from an index on the storage.
//!
//! Every entry is as high as every other, every heading too. A heading
//! never ends a page, and a section that goes on over a page break has its
//! heading again at the top of the next page (as [`Rows`](crate::kit::ui::Rows)).
