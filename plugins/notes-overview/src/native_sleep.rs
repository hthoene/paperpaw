//! The sleep screen: the note pinned in *All notes*, large and plain, with its
//! book under it and when it was pinned at the foot. Only one small file is
//! read; the library is not asked. One bit is enough: ink on paper.
use paperpaw_sdk::native::storage::{self, Root};
use paperpaw_sdk::native::{Error, Host, callbacks::DrawFrame};
use paperpaw_ui_kit::page::{Face, Look, Words};
use paperpaw_ui_kit::widget::Align;

use crate::pin::{FILE, MOST, Pinned};

/// The pinned note, if one is pinned and readable.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn pinned(host: &Host<'_>) -> Result<Option<Pinned>, Error> {
    let mut bytes = alloc::vec::Vec::new();
    bytes
        .try_reserve_exact(MOST + 256)
        .map_err(|_| Error::NoMemory)?;
    bytes.resize(MOST + 256, 0);
    let Ok(read) = storage::read_at(host, Root::Data, FILE, 0, &mut bytes) else {
        return Ok(None);
    };
    match bytes.get(..read).map(core::str::from_utf8) {
        Some(Ok(text)) => Pinned::decode(text),
        _ => Ok(None),
    }
}

/// The size the text is set in: the shorter it is the larger.
fn size_for(text: &str) -> u16 {
    match text.chars().count() {
        0..=70 => 40,
        71..=160 => 34,
        161..=320 => 28,
        _ => 24,
    }
}

/// Draws the sleep screen with the note read when it opened.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub fn draw(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    note: Option<&Pinned>,
) -> Result<(), Error> {
    let look = Look::open(host)?;
    let mut page = look.page(frame)?;
    let mut words = Words::new(host)?;
    let unix = note.map_or(0, |note| note.at);
    match note {
        Some(note) => {
            let used = page.wrap(
                page.rect(0, 70, 0, 560),
                Face::new(size_for(&note.text)),
                Align::Left,
                (&note.text, 14),
            )?;
            // The book under the text, not at a place of its own.
            let below = 70 + u32::from(used) * 480 / u32::from(page.area.width.max(1)) + 40;
            page.fill(page.rect(0, below, 60, 3))?;
            page.put(
                page.rect(0, below + 20, 0, 36),
                Face::bold(22),
                Align::Left,
                &note.book,
            )?;
            page.put(
                page.rect(0, below + 58, 0, 28),
                Face::quiet(17),
                Align::Left,
                &note.detail,
            )?;
        }
        None => {
            let none = words.copy("notes-overview.sleep.empty")?;
            page.wrap(
                page.rect(0, 200, 0, 300),
                Face::quiet(26),
                Align::Center,
                (&none, 8),
            )?;
        }
    }
    let foot = words.stand(
        ("notes-overview.sleep.stand", "notes-overview.sleep.never"),
        unix,
        &[],
    )?;
    page.footer(&foot)?;
    page.show(frame)
}
