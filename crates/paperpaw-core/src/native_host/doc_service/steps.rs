//! What the document host's background steps found (`doc_cover_job`,
//! `doc_prepare`): a look at the job, a step of the preparing loop.

/// What one look at the library's cover job found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JobStep {
    /// There is no cover job (or it was closed by the host: its row is freed).
    None,
    /// It is still preparing.
    Wait,
    /// Its preparation is over and it is closed: `ready` (else failed), and the
    /// key of its book's path (`lib_cover::path_key`).
    Ended { ready: bool, key: u64 },
    /// A details job ended (closed): whether its book's details were kept, and
    /// the key of a book that failed (0: none).
    DetailsEnded { filled: bool, key: u64 },
}

/// What one step of the preparing loop did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrepareStep {
    /// No document is preparing.
    Idle,
    /// A step was made; the next may be made at once (the next pass).
    More,
    /// The step could not be made now (the format is not started, a table or
    /// the card is busy): the loop tries again later, not at once.
    Later,
}
