//! Borrow page-turn data without constructing an owned Reading.

use super::{DispatchDepth, DropReason, Env, FlatHost, FlatInstance, Params, events, invoke};
use paperpaw_sdk::event::ParamValue;

use crate::native_host::registrations::{
    EVENT_PAGE_TURNED_CHAPTER_MAX_BYTES, EVENT_PAGE_TURNED_PATH_MAX_BYTES,
    EVENT_PAGE_TURNED_TITLE_MAX_BYTES,
};

// Count/tag/length, bitmap, now, section/offset/progress, three string lengths,
// and two optional page counters. All varints fit in at most two bytes here.
const FRAMING_AND_SCALARS: usize = 40;
const ENCODE_BYTES: usize = EVENT_PAGE_TURNED_PATH_MAX_BYTES
    + EVENT_PAGE_TURNED_TITLE_MAX_BYTES
    + EVENT_PAGE_TURNED_CHAPTER_MAX_BYTES
    + FRAMING_AND_SCALARS;
const _: () = assert!(ENCODE_BYTES < 1100);

// Keep this buffer off the scalar/lifecycle call frame even in release builds.
#[inline(never)]
pub(super) fn deliver(
    instance: &dyn FlatInstance,
    depth: &DispatchDepth,
    host: &FlatHost,
    index: usize,
    env: &Env<'_>,
    params: &Params,
) -> Result<(), DropReason> {
    let turn = turn(env.now().millis(), params).ok_or(DropReason::Producer)?;
    let mut bytes = [0; ENCODE_BYTES];
    let len = events::encode_page_turned(&turn, &mut bytes).map_err(|_| DropReason::Producer)?;
    invoke(instance, depth, host, index, env, &bytes[..len])
}

fn turn(now_ms: u64, params: &Params) -> Option<events::PageTurn<'_>> {
    if let Some(reading) = params.carried::<paperpaw_sdk::builtin::events::Reading>() {
        let (page, pages) = counters(reading.page, reading.pages);
        return Some(events::PageTurn {
            now_ms,
            section: reading.section,
            offset: reading.offset,
            progress_ppm: ppm(reading.progress),
            path: &reading.path,
            title: &reading.title,
            chapter: &reading.chapter,
            page,
            pages,
        });
    }
    let (page, pages) = counters(optional(params, "page"), optional(params, "pages"));
    Some(events::PageTurn {
        now_ms,
        section: u32::from_param(params.get("section")?)?,
        offset: u32::from_param(params.get("offset")?)?,
        progress_ppm: ppm(f32::from_param(params.get("progress")?)?),
        path: params.get("path")?,
        title: params.get("title")?,
        chapter: params.get("chapter")?,
        page,
        pages,
    })
}

// Recounting can leave stale counters while the location is already current.
// Keep delivering the location, marking unavailable counters as absent together.
fn counters(page: Option<u32>, pages: Option<u32>) -> (Option<u32>, Option<u32>) {
    if page == Some(0)
        || pages == Some(0)
        || matches!((page, pages), (Some(page), Some(pages)) if page > pages)
    {
        (None, None)
    } else {
        (page, pages)
    }
}

fn optional(params: &Params, key: &str) -> Option<u32> {
    params.get(key).and_then(u32::from_param)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Kernel contract rounds clamped percent; Rust maps NaN to zero"
)]
fn ppm(percent: f32) -> u32 {
    (((percent.clamp(0.0, 100.0) * 10_000.0) + 0.5) as u32).min(1_000_000)
}
