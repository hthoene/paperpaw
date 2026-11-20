//! The sleep screen: the entities the reader chose as tiles, each with its
//! name and the state last read, a switch that is on as an ink tile, and when
//! the states were read at the foot. Only the kept file is read; the server is
//! not asked. One bit is enough: ink on paper, outlines.
use alloc::string::String;
use paperpaw_sdk::native::{Error, Host, callbacks::DrawFrame};
use paperpaw_ui_kit::page::{Face, Look, Page, Words};
use paperpaw_ui_kit::widget::Align;

use crate::model::{Entity, Kind, Reading, Store};

/// The tiles: two columns, three rows (design pixels).
const COLUMNS: usize = 2;
const ROWS: usize = 3;
const TOP: u32 = 112;
const HEIGHT: u32 = 150;
const GAP: u32 = 16;
/// The sizes a value is tried at, the largest that fits first.
const SIZES: [u16; 4] = [48, 40, 32, 26];

/// How an entity's state reads as text.
fn state_text(words: &mut Words<'_>, entity: &Entity) -> Result<String, Error> {
    Ok(match entity.reading()? {
        Reading::Text(text) => text,
        Reading::Word(key) => words.copy(key)?,
    })
}

/// Draws the sleep screen.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub fn draw(host: &Host<'_>, frame: &mut DrawFrame<'_>, store: &Store) -> Result<(), Error> {
    let look = Look::open(host)?;
    let mut page = look.page(frame)?;
    let mut words = Words::new(host)?;
    let title = words.copy("smart-home.title")?;
    page.header(&title, "")?;
    if store.entities.is_empty() {
        let none = words.copy("smart-home.sleep.empty")?;
        page.wrap(
            page.rect(0, 280, 0, 300),
            Face::quiet(26),
            Align::Center,
            (&none, 6),
        )?;
    }
    let shown = COLUMNS * ROWS;
    for (n, entity) in store.entities.iter().take(shown).enumerate() {
        tile(&mut page, &mut words, entity, (n % COLUMNS, n / COLUMNS))?;
    }
    if store.entities.len() > shown {
        let more = i64::try_from(store.entities.len() - shown).unwrap_or(0);
        let text = words.format(
            "smart-home.sleep.more",
            &[paperpaw_sdk::native::formatting::Arg::Int {
                name: "count",
                value: more,
            }],
        )?;
        let y = TOP + ROWS as u32 * (HEIGHT + GAP) + 2;
        page.put(page.rect(0, y, 0, 28), Face::quiet(17), Align::Left, &text)?;
    }
    let args = [];
    let foot = words.stand(
        ("smart-home.sleep.stand", "smart-home.sleep.never"),
        store.synced,
        &args,
    )?;
    page.footer(&foot)?;
    page.show(frame)
}

/// One tile at (`column`, `row`).
fn tile(
    page: &mut Page<'_, '_>,
    words: &mut Words<'_>,
    entity: &Entity,
    (column, row): (usize, usize),
) -> Result<(), Error> {
    let wide = (410 - GAP) / COLUMNS as u32;
    let x = column as u32 * (wide + GAP);
    let y = TOP + row as u32 * (HEIGHT + GAP);
    let on = entity.kind() == Kind::Switch && entity.is_on();
    let outer = page.rect(x, y, wide, HEIGHT);
    if on {
        page.fill(outer)?;
    } else {
        page.frame(outer, 10, 3)?;
    }
    let paper = page.paper;
    let ink = on.then_some(paper);
    page.put_in(
        page.rect(x + 16, y + 14, wide - 32, 28),
        Face::new(19),
        Align::Left,
        entity.title(),
        ink,
    )?;
    let value = state_text(words, entity)?;
    let room = u32::from(page.u(wide - 32));
    let size = SIZES
        .iter()
        .copied()
        .find(|size| {
            page.measure(Face::bold(*size), &value)
                .is_ok_and(|w| w as u32 <= room)
        })
        .unwrap_or(SIZES[SIZES.len() - 1]);
    page.put_in(
        page.rect(x + 16, y + 56, wide - 32, 70),
        Face::bold(size),
        Align::Left,
        &value,
        ink,
    )
}
