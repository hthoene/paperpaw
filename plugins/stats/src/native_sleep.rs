//! The sleep screen: what was read today, large, the last seven days as bars
//! against the daily goal, and the streak. It is drawn from the log of days
//! the plugin keeps (and today's seconds as the recorder counts them); nothing
//! is fetched. One bit is enough: ink bars and outlines, no grays.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::snapshot::capture_snapshot;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::{Error, Host, callbacks::DrawFrame, formatting::Arg};
use paperpaw_ui_kit::page::{Face, Look, Page, Words as Dates};
use paperpaw_ui_kit::widget::{Align, LocalZone};

use crate::files::DataFolder;
use crate::native_screens::{day_in, today_in};
use crate::native_widgets::{goal, weekday_of};
use crate::store::{self, Day};
use crate::texts::{HostWords, Words, duration, seconds_on, weekday_short};
use crate::totals::streaks;

/// The days the bars show, and where they stand (design pixels).
const DAYS: usize = 7;
const BASE: u32 = 590;
const HEIGHT: u32 = 290;
const BAR: u32 = 40;
/// The footer's text and the screen's name.
const FOOT: (&str, &str) = ("stats.sleep.stand", "stats.sleep.never");

/// The seconds of the bar that reaches the top: the most of the week, the goal
/// and a quarter of an hour at least.
fn top_of(seconds: &[u32; DAYS], goal: Option<u64>) -> u32 {
    let goal = goal.map_or(0, |goal| u32::try_from(goal).unwrap_or(u32::MAX));
    seconds
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .max(goal)
        .max(900)
}

/// What the screen shows, read when it opens: today and the seconds read on
/// it as the recorder has them (`None`: not known, then the log's), the log
/// of days and the time zone they are counted in.
#[derive(Debug)]
pub(crate) struct Prepared {
    counted: Option<(i32, u32)>,
    days: Vec<Day>,
    zone: LocalZone,
}

impl Prepared {
    /// Reads the log of days, by the local day (UTC where the zone cannot be
    /// read).
    pub(crate) fn read(host: &Host<'_>, counted: Option<(i32, u32)>) -> Self {
        let zone = capture_snapshot(host)
            .and_then(|snapshot| LocalZone::read(host, &snapshot))
            .unwrap_or(LocalZone::fixed(0));
        let storage = DataStorage::new(host);
        let days = store::days(&DataFolder(&storage), day_in(zone)).unwrap_or_default();
        Self {
            counted,
            days,
            zone,
        }
    }
}

/// Draws the sleep screen from what was read when it opened.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub(crate) fn draw(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    prepared: &Prepared,
) -> Result<(), Error> {
    let Prepared {
        counted,
        days,
        zone,
    } = prepared;
    let counted = *counted;
    let snapshot = capture_snapshot(host)?;
    let today = counted
        .map(|(day, _)| day)
        .or_else(|| today_in(*zone, &snapshot));
    let look = Look::open(host)?;
    let mut page = look.page(frame)?;
    let mut words = Dates::new(host)?;
    let mut say = HostWords::new(host)?;
    let goal = goal(host)?;
    let today = today.unwrap_or(0);
    let read = counted.map_or_else(|| seconds_on(days, today), |(_, seconds)| seconds);

    let label = words.copy("stats.sleep.today")?;
    page.put(
        page.rect(0, 34, 0, 28),
        Face::quiet(20),
        Align::Left,
        &label,
    )?;
    let big = duration(&mut say, u64::from(read))?;
    let size = match big.chars().count() {
        0..=7 => 48,
        8..=9 => 40,
        _ => 32,
    };
    page.put(page.rect(0, 62, 0, 64), Face::bold(size), Align::Left, &big)?;
    if let Some(goal) = goal {
        let line = goal_line(&mut say, u64::from(read), goal)?;
        page.put(
            page.rect(0, 134, 0, 30),
            Face::quiet(20),
            Align::Left,
            &line,
        )?;
    }
    page.rule(184, 1)?;

    let mut seconds = [0u32; DAYS];
    for (n, slot) in seconds.iter_mut().enumerate() {
        let day = today - i32::try_from(DAYS - 1 - n).unwrap_or(0);
        *slot = if day == today {
            read
        } else {
            seconds_on(days, day)
        };
    }
    let week = seconds.iter().map(|s| u64::from(*s)).sum::<u64>();
    let head = words.copy("stats.sleep.week")?;
    page.put(page.rect(0, 200, 0, 28), Face::bold(20), Align::Left, &head)?;
    let total = duration(&mut say, week)?;
    page.put(
        page.rect(0, 200, 0, 28),
        Face::quiet(20),
        Align::Right,
        &total,
    )?;
    if today > 0 {
        bars(&mut page, &mut say, (&seconds, today), goal)?;
    }

    page.rule(646, 1)?;
    let current = streaks(days, today, 1);
    let streak = say.format(
        "stats.streak-note",
        &[Arg::Count(i64::from(current.current))],
    )?;
    page.put(
        page.rect(0, 660, 0, 40),
        Face::bold(26),
        Align::Left,
        &streak,
    )?;
    let best = words.format(
        "stats.sleep.best",
        &[Arg::Count(i64::from(current.longest))],
    )?;
    page.put(
        page.rect(0, 700, 0, 28),
        Face::quiet(18),
        Align::Left,
        &best,
    )?;
    let foot = words.stand((FOOT.0, FOOT.1), snapshot.unix_seconds().unwrap_or(0), &[])?;
    page.footer(&foot)?;
    page.show(frame)
}

/// "Goal 20 min reached" or "13 min to go for the goal".
fn goal_line(say: &mut HostWords<'_, '_>, read: u64, goal: u64) -> Result<String, Error> {
    let time = duration(say, goal.saturating_sub(read).max(60))?;
    let goal_text = duration(say, goal)?;
    let args = [
        Arg::Text {
            name: "time",
            value: &time,
        },
        Arg::Text {
            name: "goal",
            value: &goal_text,
        },
    ];
    say.format(
        if read >= goal {
            "stats.sleep.reached"
        } else {
            "stats.sleep.left"
        },
        &args,
    )
}

/// The seven bars with the minutes above and the weekday below, today's filled,
/// and the goal as a dashed line across.
fn bars(
    page: &mut Page<'_, '_>,
    say: &mut HostWords<'_, '_>,
    (seconds, today): (&[u32; DAYS], i32),
    goal: Option<u64>,
) -> Result<(), Error> {
    let top = top_of(seconds, goal);
    let pitch = (410 - BAR) / (DAYS as u32 - 1);
    let height_of =
        |s: u32| u32::try_from(u64::from(s) * u64::from(HEIGHT) / u64::from(top)).unwrap_or(0);
    for (n, s) in seconds.iter().enumerate() {
        let x = n as u32 * pitch;
        let high = height_of(*s);
        let shown_today = n == DAYS - 1;
        if high == 0 {
            page.fill(page.rect(x, BASE - 2, BAR, 2))?;
        } else if shown_today {
            page.fill(page.rect(x, BASE - high, BAR, high))?;
        } else {
            page.frame(page.rect(x, BASE - high, BAR, high), 0, 2)?;
        }
        if *s >= 60 {
            let mut digits = [0u8; 20];
            let minutes = say.digits(paperpaw_ui_kit::text::decimal(
                (s / 60) as usize,
                &mut digits,
            ))?;
            page.put(
                page.rect(x.saturating_sub(10), BASE - high - 26, BAR + 20, 22),
                Face::new(16),
                Align::Center,
                &minutes,
            )?;
        }
        let day = today - i32::try_from(DAYS - 1 - n).unwrap_or(0);
        let name = weekday_short(say, weekday_of(day))?;
        let face = if shown_today {
            Face::bold(18)
        } else {
            Face::new(18)
        };
        page.put(
            page.rect(x.saturating_sub(10), BASE + 8, BAR + 20, 26),
            face,
            Align::Center,
            &name,
        )?;
    }
    if let Some(goal) = goal {
        let level = height_of(u32::try_from(goal).unwrap_or(u32::MAX));
        let mut x = 0;
        while x < 410 {
            page.fill(page.rect(x, BASE - level, 5, 1))?;
            x += 10;
        }
    }
    Ok(())
}
