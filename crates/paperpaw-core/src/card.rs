//! Why a shell has no card to give the kernel as its storage.

/// What is wrong with the card a shell found nothing usable on (see
/// [`KernelBuilder::card_trouble`](crate::KernelBuilder::card_trouble)).
/// Without a card nothing is kept: no books, settings or progress, and no
/// mark that the setup was done, which would open it at every start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CardTrouble {
    /// No card is in the slot, or it does not answer.
    Missing,
    /// The card answers, but its blocks cannot be read.
    Unreadable,
    /// No trouble that keeps the card from being used: it is exFAT, which
    /// PaperPaw reads and writes as an experiment. The reader is told
    /// once, which the card remembers.
    ExFat,
    /// It has no FAT or exFAT file system, nor a partition with one.
    NoFat,
}

/// On an exFAT card whose reader was told that exFAT is an experiment.
pub(crate) const EXFAT_TOLD: &str = ".paperpaw/core/exfat-told";

impl CardTrouble {
    /// The key of the text that tells the reader.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Missing => "core.card.missing",
            Self::Unreadable => "core.card.unreadable",
            Self::ExFat => "core.card.exfat-experimental",
            Self::NoFat => "core.card.no-fat",
        }
    }
}
