//! What the library says when it has no books to list: it is preparing, it is
//! counting the books found so far, the books folder could not be read, the
//! memory card is gone, or every book is in the archive. Free of host calls:
//! the plugin reads the host's catalog state and the card and asks this.
use paperpaw_sdk::native::{Error, library::CatalogPhase};

/// What the library is up to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Condition {
    /// The catalog is made: the books are the library's.
    Ready,
    /// No catalog yet: the host is making the first one.
    Preparing,
    /// The catalog is being made anew: the books found so far, `books` of them.
    Scanning { books: u32 },
    /// The books folder could not be read.
    Failed,
    /// The card is gone until PaperPaw starts again.
    CardGone,
}

impl Condition {
    /// The conditions the catalog state and the card make: a state the host
    /// would not give is a folder that could not be read; a card that does not
    /// answer is gone.
    ///
    /// # Errors
    /// `NoMemory` stays one: the plugin could not even ask.
    pub fn of(state: Result<(CatalogPhase, u32), Error>, card: bool) -> Result<Self, Error> {
        match state {
            Ok((CatalogPhase::Ready, _)) => Ok(Self::Ready),
            Ok((CatalogPhase::Walking, books)) => Ok(Self::Scanning { books }),
            Ok((CatalogPhase::Empty, _)) | Err(Error::Busy) => Ok(Self::Preparing),
            // A phase a newer host added is read as a catalog that is there.
            Ok((_, _)) => Ok(Self::Ready),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) if !card => Ok(Self::CardGone),
            Err(_) => Ok(Self::Failed),
        }
    }

    /// Whether the books of a list are the final ones (not a count so far).
    #[must_use]
    pub const fn counting(self) -> bool {
        matches!(self, Self::Scanning { .. })
    }

    /// The keys of what an empty list says: its title and its words, where
    /// the condition says anything.
    #[must_use]
    pub const fn words(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Preparing => Some(("files.list.preparing", "")),
            Self::Failed => Some(("files.list.failed.title", "files.list.failed")),
            Self::CardGone => Some(("files.list.card-gone.title", "files.list.card-gone")),
            Self::Ready | Self::Scanning { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    #[test]
    fn the_catalog_state_says_what_the_library_is_up_to() {
        let of = |state, card| Condition::of(state, card).unwrap();
        assert_eq!(of(Ok((CatalogPhase::Ready, 5)), true), Condition::Ready);
        assert_eq!(
            of(Ok((CatalogPhase::Walking, 12)), true),
            Condition::Scanning { books: 12 }
        );
        assert_eq!(of(Ok((CatalogPhase::Empty, 0)), true), Condition::Preparing);
        assert_eq!(of(Err(Error::Busy), true), Condition::Preparing);
        assert_eq!(
            of(Ok((CatalogPhase::Unknown(9), 0)), true),
            Condition::Ready
        );
        // A refused ask is a folder that could not be read, or a card that is gone.
        assert_eq!(of(Err(Error::Io), true), Condition::Failed);
        assert_eq!(of(Err(Error::Io), false), Condition::CardGone);
        assert_eq!(
            Condition::of(Err(Error::NoMemory), true),
            Err(Error::NoMemory)
        );
    }

    #[test]
    fn only_the_conditions_without_books_have_words_and_only_a_walk_counts() {
        assert!(Condition::Ready.words().is_none());
        assert!(Condition::Scanning { books: 1 }.words().is_none());
        assert!(Condition::Scanning { books: 1 }.counting());
        assert!(!Condition::Ready.counting());
        for condition in [Condition::Preparing, Condition::Failed, Condition::CardGone] {
            assert!(condition.words().is_some());
        }
    }
}
