//! The sleep screen: today's events and then tomorrow's, large and plain, with
//! how old the calendar is at the foot. Only the file is read (what the last
//! sync kept); nothing is fetched. One bit is enough: ink on paper, rules and
//! no grays.
use alloc::string::String;
use paperpaw_sdk::native::{Error, Host, callbacks::DrawFrame, formatting::Arg};
use paperpaw_ui_kit::Messages;
use paperpaw_ui_kit::page::{Face, Look, Page, Words};
use paperpaw_ui_kit::widget::Align;

use crate::model::{Occ, Show, Store};
use crate::native_text::{aged, when_text};
use crate::native_widgets::Env;

/// Where the second block starts (design pixels from the top), and how many
/// events it shows at most.
const TOMORROW_AT: u32 = 520;
const TOMORROW_ROWS: usize = 4;
/// Where the first block's rows start, and the last y one may end at.
const FIRST_AT: u32 = 146;
const FIRST_END: u32 = TOMORROW_AT - 12;

/// Draws the sleep screen.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub fn draw(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    store: &Store,
    env: &Env,
) -> Result<(), Error> {
    let look = Look::open(host)?;
    let mut page = look.page(frame)?;
    let mut words = Words::new(host)?;
    let mut say = Messages::new()?;
    let today = env.today;
    let weekday = words.weekday(today, false)?;
    let (_, month, date) = paperpaw_sdk::time::civil_from_days(i64::from(today));
    let month = words.month(month, false)?;
    let big = words.format(
        "calendar.sleep.date",
        &[
            Arg::Int {
                name: "day",
                value: i64::from(date),
            },
            Arg::Text {
                name: "month",
                value: &month,
            },
        ],
    )?;
    let row = page.rect(0, 34, 0, 28);
    page.put(row, Face::quiet(22), Align::Left, &weekday)?;
    let row = page.rect(0, 58, 0, 62);
    page.put(row, Face::bold(44), Align::Left, &big)?;
    page.rule(128, 3)?;

    let mut first = store.occurrences(today, today, Show::All)?;
    first.retain(|occ| !over(occ, env));
    block(
        &mut page,
        (host, &mut say, store, env),
        &first,
        (FIRST_AT, FIRST_END),
        false,
    )?;
    if first.is_empty() {
        let row = page.rect(0, FIRST_AT, 0, 40);
        let none = words.copy("calendar.widget.today.empty")?;
        page.put(row, Face::quiet(22), Align::Left, &none)?;
    }

    page.rule(TOMORROW_AT, 1)?;
    let label = words.copy("calendar.day.tomorrow")?;
    let row = page.rect(0, TOMORROW_AT + 14, 110, 28);
    page.put(row, Face::quiet(18), Align::Left, &label)?;
    let long = words.date(today + 1, true)?;
    let row = page.rect(110, TOMORROW_AT + 12, 0, 30);
    page.put(row, Face::bold(20), Align::Left, &long)?;
    let next = store.occurrences(today + 1, today + 1, Show::All)?;
    block(
        &mut page,
        (host, &mut say, store, env),
        &next,
        (TOMORROW_AT + 54, 740),
        true,
    )?;
    if next.is_empty() {
        let row = page.rect(0, TOMORROW_AT + 54, 0, 36);
        let none = words.copy("calendar.widget.agenda.empty")?;
        page.put(row, Face::quiet(20), Align::Left, &none)?;
    }

    let synced = store
        .sources
        .iter()
        .map(|source| source.synced)
        .max()
        .unwrap_or(0);
    let foot = words.stand(
        ("calendar.sleep.stand", "calendar.sleep.never"),
        synced,
        &[],
    )?;
    page.footer(&foot)?;
    page.show(frame)
}

/// Whether an occurrence of today is over.
fn over(occ: &Occ, env: &Env) -> bool {
    if occ.day != env.today || occ.whole_day() {
        return false;
    }
    let ends = if occ.end == crate::model::NO_TIME {
        occ.secs
    } else {
        occ.end
    };
    ends < env.secs
}

/// The rows of `occs` from `at` down to `end` (design pixels): the time in
/// bold on the left, the title, and under it the place; `small` for the second
/// block. What does not fit is told as a count.
fn block(
    page: &mut Page<'_, '_>,
    (host, say, store, env): (&Host<'_>, &mut Messages, &Store, &Env),
    occs: &[Occ],
    (at, end): (u32, u32),
    small: bool,
) -> Result<(), Error> {
    let (size, time_w) = if small { (20, 90) } else { (24, 90) };
    let mut y = at;
    for (shown, occ) in occs.iter().enumerate() {
        let Some(entry) = store.entries.get(usize::from(occ.entry)) else {
            continue;
        };
        let place = !small && !entry.place.is_empty();
        let height = if place {
            62
        } else if small {
            40
        } else {
            46
        };
        if y + height > end || (small && shown >= TOMORROW_ROWS) {
            let more = occs.len() - shown;
            let row = page.rect(0, y, 0, 28);
            let text = more_text(host, say, more)?;
            return page.put(row, Face::quiet(17), Align::Left, &text);
        }
        let when = when_text(host, say, env.twelve, occ, false)?;
        let face = if occ.whole_day() {
            Face::quiet(17)
        } else {
            Face::bold(size)
        };
        let row = page.rect(0, y, time_w, 32);
        page.put(row, face, Align::Left, &when)?;
        let title = if entry.mark == crate::model::Mark::Event {
            mem_copy(&entry.title)?
        } else {
            aged(host, say, &entry.title, occ.age)?
        };
        let row = page.rect(time_w, y, 0, 32);
        page.put(row, Face::new(size), Align::Left, &title)?;
        if place {
            let row = page.rect(time_w, y + 30, 0, 24);
            page.put(row, Face::quiet(17), Align::Left, &entry.place)?;
        }
        y += height;
    }
    Ok(())
}

fn mem_copy(text: &str) -> Result<String, Error> {
    paperpaw_ui_kit::text::copy(text)
}

/// "+ 3 more".
fn more_text(host: &Host<'_>, say: &mut Messages, more: usize) -> Result<String, Error> {
    let mut digits = [0u8; 20];
    let count = say.digits(host, paperpaw_ui_kit::text::decimal(more, &mut digits))?;
    say.format(
        host,
        "calendar.sleep.more",
        &[Arg::Text {
            name: "count",
            value: &count,
        }],
    )
}
