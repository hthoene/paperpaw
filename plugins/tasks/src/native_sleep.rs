//! The sleep screen: the tasks that are due, in groups (overdue, today, this
//! week), each a box to tick in the mind and its day, with how many are open
//! and how old the server's copy is at the foot. Only the file is read; one
//! bit is enough (ink bars, rules, no grays).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{Error, Host, callbacks::DrawFrame, formatting::Arg};
use paperpaw_ui_kit::page::{Face, Look, Page, Words};
use paperpaw_ui_kit::widget::Align;

use crate::model::{Store, Task, View};

/// The groups, and the days of the last: today and the six after it.
const WEEK: i32 = 6;
/// Where the rows start, and where the last may end (design pixels).
const FIRST_AT: u32 = 108;
const END: u32 = 736;
const ROW: u32 = 48;
const BAR: u32 = 42;

/// The group of a task due on `due` (`None`: none) as seen on `today`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Overdue,
    Today,
    Week,
}

fn group_of(due: Option<i32>, today: i32) -> Option<Group> {
    match due {
        Some(day) if day < today => Some(Group::Overdue),
        Some(day) if day == today => Some(Group::Today),
        Some(day) if day <= today.saturating_add(WEEK) => Some(Group::Week),
        _ => None,
    }
}

/// Draws the sleep screen.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub fn draw(host: &Host<'_>, frame: &mut DrawFrame<'_>, store: &Store) -> Result<(), Error> {
    let look = Look::open(host)?;
    let mut page = look.page(frame)?;
    let mut words = Words::new(host)?;
    let today = words.now()?.map_or(0, |(day, _)| day);
    let open = store.count_open(View::Open, today);
    let side = if today > 0 {
        words.date(today, false)?
    } else {
        String::new()
    };
    let title = words.copy("tasks.sleep.title")?;
    page.header(&title, &side)?;
    let order = store.order(View::Open, today)?;
    let mut y = FIRST_AT;
    let (mut shown, mut total) = (0usize, 0usize);
    for group in [Group::Overdue, Group::Today, Group::Week] {
        let tasks: Vec<&Task> = order
            .iter()
            .filter_map(|at| store.tasks.get(usize::from(*at)))
            .filter(|task| group_of(task.due, today) == Some(group))
            .collect();
        total += tasks.len();
        if !tasks.is_empty() && y + BAR + ROW <= END {
            shown += section(&mut page, &mut words, (group, &tasks), &mut y)?;
        }
    }
    if total == 0 {
        let none = words.copy("tasks.sleep.none")?;
        page.put(
            page.rect(0, 300, 0, 50),
            Face::quiet(30),
            Align::Center,
            &none,
        )?;
    } else if shown < open {
        let left = i64::try_from(open - shown).unwrap_or(0);
        let more = words.format(
            "tasks.sleep.more",
            &[Arg::Int {
                name: "count",
                value: left,
            }],
        )?;
        page.put(
            page.rect(0, y.min(END), 0, 28),
            Face::quiet(17),
            Align::Left,
            &more,
        )?;
    }
    let count = Arg::Int {
        name: "count",
        value: i64::try_from(open).unwrap_or(0),
    };
    let foot = words.stand(
        ("tasks.sleep.stand", "tasks.sleep.never"),
        store.last_sync,
        &[count],
    )?;
    page.footer(&foot)?;
    page.show(frame)
}

/// One group: its bar and as many of its tasks as fit from `y` down; `y` moves
/// below them. Returns how many were drawn.
fn section(
    page: &mut Page<'_, '_>,
    words: &mut Words<'_>,
    (group, tasks): (Group, &[&Task]),
    y: &mut u32,
) -> Result<usize, Error> {
    let key = match group {
        Group::Overdue => "tasks.sleep.overdue",
        Group::Today => "tasks.sleep.today",
        Group::Week => "tasks.sleep.week",
    };
    let count = i64::try_from(tasks.len()).unwrap_or(0);
    let head = words.format(
        key,
        &[Arg::Int {
            name: "count",
            value: count,
        }],
    )?;
    page.fill(page.rect(0, *y, 0, 30))?;
    let paper = page.paper;
    page.put_in(
        page.rect(10, *y + 2, 0, 26),
        Face::bold(19),
        Align::Left,
        &head,
        Some(paper),
    )?;
    *y += BAR;
    let mut drawn = 0;
    for task in tasks {
        if *y + ROW > END {
            break;
        }
        page.frame(page.rect(0, *y + 4, 22, 22), 4, 2)?;
        let when = match (group, task.due) {
            (Group::Overdue, Some(day)) => words.date(day, false)?,
            (Group::Week, Some(day)) => words.weekday(day, true)?,
            _ => String::new(),
        };
        let reserve = if when.is_empty() {
            0
        } else {
            page.measure(Face::new(18), &when)? as u16 + page.u(16)
        };
        let mut row = page.rect(38, *y, 0, 32);
        row.width = row.width.saturating_sub(reserve);
        let face = if group == Group::Overdue {
            Face::bold(23)
        } else {
            Face::new(23)
        };
        page.put(row, face, Align::Left, task.title())?;
        page.put(
            page.rect(0, *y + 2, 0, 30),
            Face::new(18),
            Align::Right,
            &when,
        )?;
        *y += ROW;
        drawn += 1;
    }
    *y += 8;
    Ok(drawn)
}
