use super::*;

/// Where a list is read from: its window's first reply, and the starts of
/// the windows before it.
#[derive(Debug)]
pub(super) struct Reading {
    pub(super) start: CatalogCursor,
    pub(super) starts: Vec<CatalogCursor>,
}

impl Reading {
    pub(super) const fn from(start: CatalogCursor, starts: Vec<CatalogCursor>) -> Self {
        Self { start, starts }
    }
}

/// Where the focus goes once a window is read.
#[derive(Debug, Clone, Copy)]
pub(super) enum Then {
    Keep,
    FirstBook,
    LastRow,
}

/// What to read for a list and where to leave the focus.
#[derive(Debug)]
pub(super) struct Plan {
    pub(super) selection: Selection,
    pub(super) reading: Reading,
    pub(super) at_least: usize,
    pub(super) focus: usize,
    pub(super) then: Then,
}

/// Where to read a window of a list next, and where the focus goes.
#[derive(Debug)]
pub(super) struct Step {
    pub(super) reading: Reading,
    pub(super) at_least: usize,
    pub(super) focus: usize,
    pub(super) then: Then,
}

/// "More": a window with room grows, a full one slides on to what follows
/// it (the starts of the ones before are kept for "Previous").
pub(super) fn step_on<T>(
    window: &Window<CatalogCursor, T>,
    starts: &[CatalogCursor],
    focus: usize,
) -> Result<Step, Error> {
    let mut starts = cursors(starts)?;
    if window.can_grow() {
        return Ok(Step {
            reading: Reading::from(window.start, starts),
            at_least: window.books.len() + 32,
            focus,
            then: Then::Keep,
        });
    }
    let next = window.next.ok_or(Error::State)?;
    push(&mut starts, window.start)?;
    Ok(Step {
        reading: Reading::from(next, starts),
        at_least: 0,
        focus: 0,
        then: Then::FirstBook,
    })
}

/// "Previous": the window before, read in full, the focus on the row that
/// goes on again; none when there is none.
pub(super) fn step_back(starts: &[CatalogCursor]) -> Result<Option<Step>, Error> {
    let mut starts = cursors(starts)?;
    let Some(start) = starts.pop() else {
        return Ok(None);
    };
    Ok(Some(Step {
        reading: Reading::from(start, starts),
        at_least: MOST_BOOKS,
        focus: 0,
        then: Then::LastRow,
    }))
}

impl Plan {
    pub(super) fn of(selection: Selection, step: Step) -> Self {
        Self {
            selection,
            reading: step.reading,
            at_least: step.at_least,
            focus: step.focus,
            then: step.then,
        }
    }
}

/// A copy of window starts, fallibly.
pub(super) fn cursors(from: &[CatalogCursor]) -> Result<Vec<CatalogCursor>, Error> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(from.len())
        .map_err(|_| Error::NoMemory)?;
    for cursor in from {
        push(&mut copy, *cursor)?;
    }
    Ok(copy)
}
