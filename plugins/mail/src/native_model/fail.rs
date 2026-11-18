//! What can go wrong in the native readers that touch the card: the card
//! says no, or an allocation is refused.
use super::mem::Error;
use paperpaw_sdk::StorageError;

/// Why a read or a write of what Mail keeps failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fail {
    /// An allocation was refused (not the card's fault: it is never
    /// reported as [`StorageError::CARD_GONE`]).
    NoMemory,
    /// The card or the host's storage said no.
    Storage(StorageError),
}

impl From<Error> for Fail {
    fn from(_: Error) -> Self {
        Self::NoMemory
    }
}

impl From<StorageError> for Fail {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}
