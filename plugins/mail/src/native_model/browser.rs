//! What the list screens show, host-free: *Mail* (the folders of every
//! account, with how many mails wait) and a folder (its mails, newest first).
//! Each line is made from what the card keeps and the words of the catalog
//! (through [`Words`]), with every allocation asked for.
use super::account::Account;
use super::fail::Fail;
use super::mem::{Error, copy, push};
use super::session::Task;
use super::store::{self, Counts, Folder, Row};
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Reverse;
use paperpaw_sdk::Storage;
use paperpaw_ui_kit::text;

/// The words a list needs, from the host's catalog and clock.
pub trait Words {
    /// The catalog's text for `key`.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the host says.
    fn tr(&mut self, key: &str) -> Result<String, Error>;
    /// When a mail came: the time of day if today, else the date; nothing
    /// where the mail has no date.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the host says.
    fn when(&mut self, date: i64) -> Result<String, Error>;
}

/// What a line does when chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Target {
    /// Check for mail.
    Check,
    /// Open a folder of an account.
    Folder {
        /// The account (1 to 3).
        account: u8,
        /// The folder as the server names it.
        raw: String,
    },
    /// Open a mail of the folder the screen shows.
    Message {
        /// The server's id of the mail.
        uid: u32,
    },
    /// Write a mail.
    Write,
    /// Open the settings of Mail.
    Settings,
}

/// A line of a list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The left text.
    pub title: String,
    /// The note below.
    pub note: String,
    /// The text at the right.
    pub value: String,
    /// What choosing it does.
    pub target: Target,
}

/// Which list a screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Mode {
    /// The folders of every account.
    Boxes,
    /// One folder.
    Folder {
        /// The account (1 to 3).
        account: u8,
        /// The folder as the server names it.
        raw: String,
    },
}

fn line(title: &str, note: &str, value: &str, target: Target) -> Result<Line, Error> {
    Ok(Line {
        title: copy(title)?,
        note: copy(note)?,
        value: copy(value)?,
        target,
    })
}

/// The translation key of a folder the server marks for a purpose (or the
/// inbox), so that it reads in the reader's language; for servers that mark
/// none, the usual English names of those folders.
#[must_use]
pub fn special_name(folder: &Folder) -> Option<&'static str> {
    if folder.is_inbox() {
        return Some("mail.inbox");
    }
    const KINDS: [(&str, &str, &str); 5] = [
        ("\\sent", "sent", "mail.folder.sent"),
        ("\\drafts", "drafts", "mail.folder.drafts"),
        ("\\trash", "trash", "mail.folder.trash"),
        ("\\junk", "junk", "mail.folder.junk"),
        ("\\archive", "archive", "mail.folder.archive"),
    ];
    let marked = KINDS
        .iter()
        .find(|(flag, _, _)| folder.flags.iter().any(|f| f == flag));
    let named = || {
        KINDS
            .iter()
            .find(|(_, name, _)| folder.name.eq_ignore_ascii_case(name))
    };
    marked.or_else(named).map(|(_, _, key)| *key)
}

/// The title of a folder's screen.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn folder_title(
    storage: &dyn Storage,
    words: &mut dyn Words,
    account: u8,
    raw: &str,
) -> Result<String, Fail> {
    let found = store::read_folders(storage, &store::folders_file(account)?)?
        .into_iter()
        .find(|f| f.raw == raw);
    Ok(match found {
        Some(folder) => match special_name(&folder) {
            Some(key) => words.tr(key)?,
            None => folder.name,
        },
        None if raw.eq_ignore_ascii_case("INBOX") => words.tr("mail.inbox")?,
        None => copy(raw)?,
    })
}

/// The lines of the folders of every account: the check, each folder with its
/// unseen mails, then writing and the settings. With no account, the one line
/// that says to set one up.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn boxes(
    storage: &dyn Storage,
    words: &mut dyn Words,
    accounts: &[Account],
) -> Result<Vec<Line>, Fail> {
    let mut lines = Vec::new();
    if accounts.is_empty() {
        let (title, help) = (words.tr("mail.set-up")?, words.tr("mail.set-up.help")?);
        push(
            &mut lines,
            line(&title, &help, "\u{203a}", Target::Settings)?,
        )?;
        return Ok(lines);
    }
    push(
        &mut lines,
        line(&words.tr("mail.check")?, "", "", Target::Check)?,
    )?;
    for account in accounts {
        let mut folders = store::read_folders(storage, &store::folders_file(account.number)?)?;
        if folders.is_empty() {
            push(&mut folders, Folder::new("INBOX", Vec::new())?)?;
        }
        for folder in folders.into_iter().filter(Folder::selectable) {
            let title = match special_name(&folder) {
                Some(key) => words.tr(key)?,
                None => copy(&folder.name)?,
            };
            let unseen = Counts::read(storage, &store::status_file(account.number, &folder.raw)?)
                .map_or(0, |c| c.unseen);
            let value = if unseen > 0 {
                text::format(format_args!("{unseen}"))?
            } else {
                String::new()
            };
            let note = if accounts.len() > 1 {
                account.address.as_str()
            } else {
                ""
            };
            let target = Target::Folder {
                account: account.number,
                raw: folder.raw,
            };
            push(&mut lines, line(&title, note, &value, target)?)?;
        }
    }
    for (key, target) in [
        ("mail.write", Target::Write),
        ("mail.settings", Target::Settings),
    ] {
        push(&mut lines, line(&words.tr(key)?, "", "\u{203a}", target)?)?;
    }
    Ok(lines)
}

/// A mail as a line: the sender with the marks, the subject, the time.
fn message(words: &mut dyn Words, row: &Row) -> Result<Line, Fail> {
    let mut title = String::new();
    if !row.seen() {
        title.try_reserve(4).map_err(|_| Error::NoMemory)?;
        title.push_str("\u{2022} ");
    }
    if row.flagged() {
        title.try_reserve(2).map_err(|_| Error::NoMemory)?;
        title.push_str("* ");
    }
    super::mem::push_str(&mut title, &row.from)?;
    let subject = if row.subject.is_empty() {
        words.tr("mail.no-subject")?
    } else {
        copy(&row.subject)?
    };
    let value = words.when(row.date)?;
    Ok(Line {
        title,
        note: subject,
        value,
        target: Target::Message { uid: row.uid },
    })
}

/// The lines of a folder: the check, its mails newest first, then writing.
///
/// # Errors
/// [`Fail::NoMemory`].
pub fn folder(
    storage: &dyn Storage,
    words: &mut dyn Words,
    account: u8,
    raw: &str,
) -> Result<Vec<Line>, Fail> {
    let mut rows = store::read_index(storage, &store::index_file(account, raw)?)?;
    // The shared sorter: the std sort keeps a 32-row scratch on the stack, a frame the X4 refuses.
    paperpaw_sdk::sort::sort_by_key(&mut rows, |r| Reverse(r.uid));
    let mut lines = Vec::new();
    push(
        &mut lines,
        line(&words.tr("mail.check")?, "", "", Target::Check)?,
    )?;
    for row in &rows {
        push(&mut lines, message(words, row)?)?;
    }
    push(
        &mut lines,
        line(&words.tr("mail.write")?, "", "\u{203a}", Target::Write)?,
    )?;
    Ok(lines)
}

/// What a check of `number` does on the server for the list `mode`: the
/// folders and the inbox for *Mail*, one folder for a folder of that account.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn tasks(mode: &Mode, number: u8, count: u32, now: i64) -> Result<Vec<Task>, Fail> {
    let sync = |raw: &str| -> Result<Task, Fail> {
        Ok(Task::Sync {
            folder: copy(raw)?,
            count,
            index: store::index_file(number, raw)?,
            status: store::status_file(number, raw)?,
            now,
        })
    };
    let mut tasks = Vec::new();
    match mode {
        Mode::Boxes => {
            push(
                &mut tasks,
                Task::Folders {
                    out: store::folders_file(number)?,
                },
            )?;
            push(&mut tasks, sync("INBOX")?)?;
        }
        Mode::Folder { account, raw } if *account == number => push(&mut tasks, sync(raw)?)?,
        Mode::Folder { .. } => {}
    }
    Ok(tasks)
}

/// When a mail came, as the list shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// The mail has no date.
    Never,
    /// Today: the time of day.
    Today {
        /// 0 to 23.
        hour: u8,
        /// 0 to 59.
        minute: u8,
    },
    /// Another day.
    Day {
        /// 1 to 12.
        month: u8,
        /// 1 to 31.
        day: u8,
    },
}

/// When `date` (Unix seconds) is, for a reader whose zone is `offset` seconds
/// ahead of UTC, at `now` (where the clock is known).
#[must_use]
pub fn when_of(date: i64, now: Option<i64>, offset: i64) -> When {
    if date <= 0 {
        return When::Never;
    }
    let local = date.saturating_add(offset);
    let (day, second) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    let today = now.map(|now| now.saturating_add(offset).div_euclid(86_400));
    if today == Some(day) {
        return When::Today {
            hour: u8::try_from(second / 3600).unwrap_or(0),
            minute: u8::try_from(second % 3600 / 60).unwrap_or(0),
        };
    }
    let (_, month, day) = paperpaw_sdk::time::civil_from_days(day);
    When::Day { month, day }
}

/// A hash in two lines of groups of eight digits, to compare.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn grouped(hex: &str) -> Result<String, Error> {
    let mut out = super::mem::string_with(hex.len() + 8)?;
    for (i, chunk) in hex.as_bytes().chunks(8).enumerate() {
        if i == 4 {
            super::mem::push_char(&mut out, '\n')?;
        } else if i > 0 {
            super::mem::push_char(&mut out, ' ')?;
        }
        super::mem::push_str(&mut out, core::str::from_utf8(chunk).unwrap_or(""))?;
    }
    Ok(out)
}
