//! *Reading cat* (`reading-goal.widget`) on interface 7: how high it is, what
//! it draws, what it shows (a number that changes with it, for the minute
//! tick) and what a tap does. The cat's picture needs images (draft 22) and
//! is not drawn yet: its mood is put in words in the picture's place, as
//! interface 6 did where the file was missing; the minutes and the goal show
//! below it as before.
//!
//! Nothing is kept between callbacks here: the figure is the plugin's
//! ([`crate::model::Kept`]), the goal and the thresholds are settings, and the
//! state of the statistics is read by the plugin once a minute and passed in.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        formatting::Arg,
        settings::{self, SettingValue},
        snapshot::capture_snapshot,
        storage::DataStorage,
    },
};
use paperpaw_ui_kit::widget::{Align, Cx, cx::zone_offset, layout::label};
use paperpaw_ui_kit::{Paint, geometry};

use crate::model::{Kept, Mood, Statistics, Today};
use crate::store;

/// The one target of the widget: the whole of it.
pub const TARGET: u32 = 1;
/// The goal (the statistics' setting, `stats.goal.<minutes>` or `.off`).
const GOAL: &str = "stats.goal";
/// The thresholds, in percent.
const CONTENT: &str = "reading-goal.content";
const HAPPY: &str = "reading-goal.happy";
/// What they are where the setting cannot be read.
const CONTENT_DEFAULT: i64 = 50;
const HAPPY_DEFAULT: i64 = 100;
/// The most a goal's name takes, `stats.goal.<minutes>` or `.off`.
const GOAL_BUFFER: usize = 192;

/// The day number of today by the local time, in days since 1970; `None`
/// where the device does not know the time.
///
/// # Errors
/// The host's refusal.
pub fn today(host: &Host<'_>) -> Result<Option<i32>, Error> {
    let Some(unix) = capture_snapshot(host)?.unix_seconds() else {
        return Ok(None);
    };
    let local = unix.saturating_add(i64::from(zone_offset(host, unix)?));
    Ok(i32::try_from(local.div_euclid(86_400)).ok())
}

/// A whole-number setting, or `default` where it cannot be read.
fn number(host: &Host<'_>, key: &str, default: i64) -> i64 {
    let mut bytes = [0u8; 64];
    match settings::read_setting(host, key, &mut bytes) {
        Ok(SettingValue::Int(value)) => value,
        _ => default,
    }
}

/// What is known of today's reading: the figure the statistics told last (in
/// memory, else `today.txt`), the goal and the thresholds as set. A goal that
/// cannot be read is none: the cat goes by the 20 minutes and only the
/// minutes read show.
///
/// # Errors
/// The host's refusal of the clock.
pub fn facts(host: &Host<'_>, kept: &Kept, statistics: Statistics) -> Result<Today, Error> {
    let mut bytes = [0u8; GOAL_BUFFER];
    let goal = match settings::read_setting(host, GOAL, &mut bytes) {
        Ok(SettingValue::Text(goal)) => goal,
        _ => "",
    };
    let told = match kept.told(None) {
        Some(told) => Some(told),
        None => store::read_told(&DataStorage::new(host)),
    };
    Ok(Today::of(
        goal,
        told,
        today(host)?,
        number(host, CONTENT, CONTENT_DEFAULT),
        number(host, HAPPY, HAPPY_DEFAULT),
        statistics,
    ))
}

/// A number that changes when what the widget shows does (never 0, which is
/// "nothing shown"): the minutes, the goal, the mood and the statistics'
/// state. It needs neither font nor look, so the minute tick opens no drawing
/// context.
///
/// # Errors
/// The host's refusal of the clock.
pub fn shows(host: &Host<'_>, kept: &Kept, statistics: Statistics) -> Result<u64, Error> {
    let today = facts(host, kept, statistics)?;
    Ok(mark(&today))
}

/// What the widget shows of `today`, as one number: when it differs from the
/// one noted at the last draw, the widget is out of date.
pub(crate) fn mark(today: &Today) -> u64 {
    let state = match today.statistics {
        Statistics::Counting => 1,
        Statistics::Off => 2,
        Statistics::Missing => 3,
    };
    1 + (u64::from(today.minutes) << 32)
        + (u64::from(today.goal.unwrap_or(0)) << 8)
        + ((today.mood as u64) << 4)
        + state
}

/// The words for the mood, in place of its picture.
fn mood_key(mood: Mood) -> &'static str {
    match mood {
        Mood::Sad => "reading-goal.mood.sad",
        Mood::Meh => "reading-goal.mood.meh",
        Mood::Content => "reading-goal.mood.content",
        Mood::Happy => "reading-goal.mood.happy",
    }
}

/// "12 / 20 min" ("12 min" without a goal), or what the statistics' state
/// says where they do not count.
///
/// # Errors
/// Catalog refusals and [`Error::NoMemory`].
pub fn progress(cx: &mut Cx<'_>, today: &Today) -> Result<String, Error> {
    if today.statistics != Statistics::Counting {
        return cx.tr("reading-goal.uncounted");
    }
    let read = Arg::Int {
        name: "read",
        value: i64::from(today.minutes),
    };
    match today.goal {
        Some(goal) => cx.format(
            "reading-goal.progress",
            &[
                read,
                Arg::Int {
                    name: "goal",
                    value: i64::from(goal),
                },
            ],
        ),
        None => cx.format("reading-goal.read", &[read]),
    }
}

/// The height of the line of minutes below the picture, 0 where it is off.
fn line(cx: &mut Cx<'_>) -> Result<u16, Error> {
    if cx.settings.shows("minutes")? {
        geometry::line_px(&cx.font, cx.style().small)
    } else {
        Ok(0)
    }
}

/// The height at `width`: a picture about as high as half the width, the
/// line of minutes, the frame.
///
/// # Errors
/// Font and settings refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(width / 2)
        .saturating_add(line(cx)?))
}

/// Lays the widget out in `area`: the frame, the mood in words in the
/// picture's place, the minutes below.
///
/// # Errors
/// Font, settings, catalog and drawing refusals; [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect, today: &Today) -> Result<(), Error> {
    // The snapshot does not tell the gray levels: a card is taken to be drawn
    // on a panel with grays.
    let look = cx.shape.paint(out, area, 256)?;
    let inner = look.area;
    let line = line(cx)?;
    let small = cx.style().small;
    let room = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(line),
    );
    let words = cx.tr(mood_key(today.mood))?;
    label(
        out,
        &cx.font,
        (small, Align::Center),
        room,
        &words,
        look.ink,
    )?;
    if line == 0 {
        return Ok(());
    }
    let text = progress(cx, today)?;
    let row = Rect::new(
        inner.x,
        inner.y.saturating_add(inner.height.saturating_sub(line)),
        inner.width,
        line,
    );
    label(out, &cx.font, (small, Align::Center), row, &text, look.ink)
}

/// What the offer of the statistics says, by their state: the labels' keys of
/// its choices, in order.
#[must_use]
pub const fn offer(statistics: Statistics) -> Option<[&'static str; 2]> {
    match statistics {
        Statistics::Counting => None,
        Statistics::Off => Some(["reading-goal.offer.switch-on", "reading-goal.offer.not-now"]),
        Statistics::Missing => Some(["reading-goal.offer.store", "reading-goal.offer.not-now"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today(minutes: u32, goal: Option<u32>, mood: Mood, statistics: Statistics) -> Today {
        Today {
            goal,
            minutes,
            mood,
            statistics,
        }
    }

    #[test]
    fn what_changes_what_is_shown_changes_the_mark_and_nothing_else_does() {
        let base = today(12, Some(20), Mood::Meh, Statistics::Counting);
        assert_ne!(mark(&base), 0);
        assert_eq!(mark(&base), mark(&base));
        for other in [
            today(13, Some(20), Mood::Meh, Statistics::Counting),
            today(12, Some(30), Mood::Meh, Statistics::Counting),
            today(12, None, Mood::Meh, Statistics::Counting),
            today(12, Some(20), Mood::Content, Statistics::Counting),
            today(12, Some(20), Mood::Meh, Statistics::Off),
            today(12, Some(20), Mood::Meh, Statistics::Missing),
        ] {
            assert_ne!(mark(&base), mark(&other), "{other:?}");
        }
    }

    #[test]
    fn the_offer_is_made_only_where_the_statistics_do_not_count() {
        assert_eq!(offer(Statistics::Counting), None);
        assert_eq!(
            offer(Statistics::Off),
            Some(["reading-goal.offer.switch-on", "reading-goal.offer.not-now"])
        );
        assert_eq!(
            offer(Statistics::Missing),
            Some(["reading-goal.offer.store", "reading-goal.offer.not-now"])
        );
    }

    #[test]
    fn every_mood_has_words_in_both_languages() {
        let en = include_str!("../i18n/en.ini");
        let de = include_str!("../i18n/de.ini");
        for mood in [Mood::Sad, Mood::Meh, Mood::Content, Mood::Happy] {
            let key = alloc::format!("{} =", mood_key(mood));
            assert!(en.contains(&key) && de.contains(&key), "{key}");
        }
    }
}
