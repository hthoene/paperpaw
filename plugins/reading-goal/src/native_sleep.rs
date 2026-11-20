//! *Today's reading cat* as the sleep screen: the cat of today's mood large on
//! paper (words in its place where there is no picture), under it a headline
//! of the mood, the minutes read and the goal, a bar of the share reached, and
//! at the foot when the screen was made. One bit is enough: ink on paper.
use paperpaw_sdk::{
    Rect,
    native::{
        Error,
        callbacks::DrawFrame,
        formatting::Arg,
        image::{ImageId, ImageStyle},
        snapshot::capture_snapshot,
    },
};
use paperpaw_ui_kit::page::{Face, Page, Words};
use paperpaw_ui_kit::paint;
use paperpaw_ui_kit::widget::{Align, Cx};

use crate::model::{Mood, Statistics, Today};
use crate::native_widget::{mood_key, progress};

/// The size of a `picture` of `size` pixels fitted into `room` (as large as
/// leaves room, its shape kept): both sides 0 for no picture or an empty room.
fn fitted(size: (u32, u32), room: (u16, u16)) -> (u16, u16) {
    let (w, h) = (u64::from(size.0), u64::from(size.1));
    let (room_w, room_h) = (u64::from(room.0), u64::from(room.1));
    if w == 0 || h == 0 || room_w == 0 || room_h == 0 {
        return (0, 0);
    }
    // The wider shape fills the width, the taller one the height.
    let (fit_w, fit_h) = if w * room_h <= h * room_w {
        (w * room_h / h, room_h)
    } else {
        (room_w, h * room_w / w)
    };
    (
        u16::try_from(fit_w.max(1)).unwrap_or(u16::MAX),
        u16::try_from(fit_h.max(1)).unwrap_or(u16::MAX),
    )
}

/// Where the cat is, and where the bar: design pixels on the page.
const CAT_TOP: u32 = 30;
const CAT_HEIGHT: u32 = 420;
const BAR_AT: u32 = 600;
const BAR_HEIGHT: u32 = 28;
const BAR_INSET: u32 = 40;

/// The headline of a mood.
fn headline_key(mood: Mood) -> &'static str {
    match mood {
        Mood::Sad => "reading-goal.sleep.sad",
        Mood::Meh => "reading-goal.sleep.meh",
        Mood::Content => "reading-goal.sleep.content",
        Mood::Happy => "reading-goal.sleep.happy",
    }
}

/// The share of the goal read, in whole percent (at most 100), where there is
/// a goal that counts.
fn share(today: &Today) -> Option<u32> {
    let goal = today.goal.filter(|goal| *goal > 0)?;
    Some((u64::from(today.minutes) * 100 / u64::from(goal)).min(100) as u32)
}

/// Draws the screen into `frame`.
///
/// # Errors
/// Font, settings, catalog and drawing refusals; [`Error::NoMemory`].
pub(crate) fn draw(
    cx: &mut Cx<'_>,
    frame: &mut DrawFrame<'_>,
    today: &Today,
    picture: Option<(ImageId, (u32, u32))>,
) -> Result<(), Error> {
    let mut words = Words::new(cx.host)?;
    let words_for_cat = if picture.is_none() {
        Some(cx.tr(mood_key(today.mood))?)
    } else {
        None
    };
    let progress = progress(cx, today)?;
    let headline = words.copy(headline_key(today.mood))?;
    let mut page = Page::new(&cx.font, cx.style(), frame)?;
    let counting = today.statistics == Statistics::Counting;
    let room = page.rect(0, CAT_TOP, 0, CAT_HEIGHT);
    let cat = fitted(
        picture.map_or((0, 0), |(_, size)| size),
        (room.width, room.height),
    );
    let at = Rect::new(
        room.x.saturating_add(room.width.saturating_sub(cat.0) / 2),
        room.y.saturating_add(room.height.saturating_sub(cat.1) / 2),
        cat.0,
        cat.1,
    );
    if let Some(text) = &words_for_cat {
        page.put(room, Face::new(30), Align::Center, text)?;
    }
    if counting {
        page.put(
            page.rect(0, 470, 0, 56),
            Face::bold(38),
            Align::Center,
            &headline,
        )?;
    }
    page.put(
        page.rect(0, 530, 0, 36),
        Face::new(24),
        Align::Center,
        &progress,
    )?;
    if let (true, Some(percent)) = (counting, share(today)) {
        let outer = page.rect(BAR_INSET, BAR_AT, 410 - 2 * BAR_INSET, BAR_HEIGHT);
        page.frame(outer, 0, 3)?;
        let inner_w = u32::from(outer.width.saturating_sub(6)) * percent / 100;
        page.fill(Rect::new(
            outer.x.saturating_add(3),
            outer.y.saturating_add(3),
            u16::try_from(inner_w).unwrap_or(0),
            outer.height.saturating_sub(6),
        ))?;
        let text = words.format(
            "reading-goal.sleep.percent",
            &[Arg::Int {
                name: "percent",
                value: i64::from(percent),
            }],
        )?;
        page.put(
            page.rect(0, 642, 0, 30),
            Face::quiet(20),
            Align::Center,
            &text,
        )?;
    }
    let unix = capture_snapshot(cx.host)?.unix_seconds().unwrap_or(0);
    let foot = words.stand(
        ("reading-goal.sleep.stand", "reading-goal.sleep.never"),
        unix,
        &[],
    )?;
    page.footer(&foot)?;
    page.show(frame)?;
    if let Some((image, _)) = picture {
        frame
            .draw_canvas()?
            .draw_image(image, paint::frame_rect(at), ImageStyle::new())
            .ok();
    }
    Ok(())
}

#[cfg(test)]
mod tests;
