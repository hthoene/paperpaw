//! Filters: stages where plugins look at what passes and may change it.

/// Whether a block stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    Keep,
    /// Left out of the layout (and of what is searched).
    Drop,
}
