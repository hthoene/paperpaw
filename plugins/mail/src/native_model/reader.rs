//! What the reader of a mail shows and does, host-free: the headers and the
//! text as one text, the tasks that fetch a mail (and mark it read), and the
//! stars and read marks in the folder's list. Everything allocated is asked
//! for.
use super::browser::Words;
use super::fail::Fail;
use super::mem::{Error, copy, push, push_char, push_str, string_with};
use super::session::Task;
use super::store::{self, Letter, Row};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;

/// The letter as the reader shows it: the sender, the copies, the date and the
/// attachments (each where there is something to say), a blank line, the text,
/// and where it was cut, a line that says so.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn shown(words: &mut dyn Words, letter: &Letter) -> Result<String, Fail> {
    let mut out = string_with(letter.text.len() + 256)?;
    let when = words.when(letter.date)?;
    let attachments = join(&letter.attachments)?;
    for (key, value) in [
        ("mail.from", letter.from.as_str()),
        ("mail.cc", letter.cc.as_str()),
        ("mail.date-label", when.as_str()),
        ("mail.attachments", attachments.as_str()),
    ] {
        if !value.is_empty() {
            push_str(&mut out, &words.tr(key)?)?;
            push_str(&mut out, ": ")?;
            push_str(&mut out, value)?;
            push_char(&mut out, '\n')?;
        }
    }
    push_char(&mut out, '\n')?;
    push_str(&mut out, &letter.text)?;
    if letter.truncated {
        push_str(&mut out, "\n\n")?;
        push_str(&mut out, &words.tr("mail.cut")?)?;
    }
    Ok(out)
}

fn join(items: &[String]) -> Result<String, Error> {
    let total = items.iter().map(|i| i.len() + 2).sum::<usize>();
    let mut out = string_with(total)?;
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            push_str(&mut out, ", ")?;
        }
        push_str(&mut out, item)?;
    }
    Ok(out)
}

/// What fetching the mail `uid` of `folder` does on the server: its text to
/// the account's raw file, and where the settings say so, the `\Seen` flag.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn fetch_tasks(
    account: u8,
    folder: &str,
    uid: u32,
    mark_read: bool,
) -> Result<Vec<Task>, Fail> {
    let mut tasks = Vec::new();
    push(
        &mut tasks,
        Task::Body {
            folder: copy(folder)?,
            uid,
            out: store::raw_file(account, folder, uid)?,
        },
    )?;
    if mark_read {
        push(&mut tasks, flag_task(folder, uid, "\\Seen", true)?)?;
    }
    Ok(tasks)
}

/// Setting (`on`) or clearing a flag of the mail `uid` of `folder`.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn flag_task(folder: &str, uid: u32, flag: &'static str, on: bool) -> Result<Task, Fail> {
    Ok(Task::Flag {
        folder: copy(folder)?,
        uid,
        flag,
        on,
    })
}

/// Changes the row of `uid` in the folder's list with `change`, and writes the
/// list again; whether the mail was in it.
///
/// # Errors
/// [`Fail::Storage`], [`Fail::NoMemory`].
pub fn change_row(
    storage: &dyn Storage,
    account: u8,
    folder: &str,
    uid: u32,
    change: impl FnOnce(&mut Row) -> Result<(), Error>,
) -> Result<bool, Fail> {
    let file = store::index_file(account, folder)?;
    let mut rows = store::read_index(storage, &file)?;
    let Some(row) = rows.iter_mut().find(|r| r.uid == uid) else {
        return Ok(false);
    };
    change(row)?;
    store::write_index(storage, &file, &rows)?;
    Ok(true)
}

/// Whether the mail `uid` is starred in the folder's list.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn starred(storage: &dyn Storage, account: u8, folder: &str, uid: u32) -> Result<bool, Fail> {
    let rows = store::read_index(storage, &store::index_file(account, folder)?)?;
    Ok(rows.iter().any(|r| r.uid == uid && r.flagged()))
}

/// `flags` with the letter `flag` set or cleared.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn with_flag(flags: &str, flag: char, on: bool) -> Result<String, Error> {
    if on && flags.contains(flag) {
        return copy(flags);
    }
    let mut out = string_with(flags.len() + 1)?;
    for c in flags.chars().filter(|c| *c != flag) {
        push_char(&mut out, c)?;
    }
    if on {
        push_char(&mut out, flag)?;
    }
    Ok(out)
}

/// Where the parts of the reader go, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// The text.
    pub area: paperpaw_sdk::Rect,
    /// The line of the status, above the buttons.
    pub note: paperpaw_sdk::Rect,
    /// *Reply*, *Reply all* and *Star*, side by side.
    pub buttons: [paperpaw_sdk::Rect; 3],
}

/// The reader on a `width` by `height` screen under a header of `header`
/// pixels: the text from the header to the status line, the status line of
/// `note` pixels above a row of three buttons `row` high.
#[must_use]
pub fn layout(
    width: u16,
    height: u16,
    header: u16,
    gap: u16,
    margin: u16,
    row: u16,
    note: u16,
) -> Layout {
    use paperpaw_sdk::Rect;
    let top = header.saturating_add(gap);
    let bottom = height.saturating_sub(row.saturating_add(gap));
    let note_h = note.saturating_add(gap);
    let area = Rect::new(
        margin / 2,
        top,
        width.saturating_sub(margin),
        bottom.saturating_sub(top.saturating_add(note_h)),
    );
    let whole = width.saturating_sub(margin.saturating_mul(2));
    let each = whole.saturating_sub(gap.saturating_mul(2)) / 3;
    let button = |i: u16| {
        Rect::new(
            margin.saturating_add(i.saturating_mul(each.saturating_add(gap))),
            bottom,
            each,
            row,
        )
    };
    Layout {
        area,
        note: Rect::new(0, bottom.saturating_sub(note_h), width, note_h),
        buttons: [button(0), button(1), button(2)],
    }
}

/// Where a key or a gesture takes the text that shows `rows` of `total` lines
/// from line `top`.
#[must_use]
pub fn scrolled(top: usize, rows: usize, total: usize, step: Step) -> usize {
    let page = isize::try_from(rows).unwrap_or(isize::MAX);
    let few = (page / 3).max(1);
    let by = match step {
        Step::Down => few,
        Step::Up => -few,
        Step::PageDown => page,
        Step::PageUp => -page,
    };
    let last = total.saturating_sub(rows);
    top.saturating_add_signed(by).min(last)
}

/// What moves the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// A few lines on.
    Down,
    /// A few lines back.
    Up,
    /// A page on.
    PageDown,
    /// A page back.
    PageUp,
}

impl Step {
    /// The step a key action means.
    #[must_use]
    pub fn of_action(action: &str) -> Option<Self> {
        match action {
            "core.down" => Some(Self::Down),
            "core.up" => Some(Self::Up),
            "core.next-page" | "core.next" => Some(Self::PageDown),
            "core.prev-page" | "core.previous" => Some(Self::PageUp),
            _ => None,
        }
    }
}

/// The button the focus moves to from `focus` (none while the text is
/// scrolled) on a left or right key.
#[must_use]
pub fn focus_after(focus: Option<usize>, right: bool) -> usize {
    let from = focus.map_or(if right { -1 } else { 3 }, |f| {
        isize::try_from(f).unwrap_or(0)
    });
    usize::try_from((from + if right { 1 } else { -1 }).rem_euclid(3)).unwrap_or(0)
}
