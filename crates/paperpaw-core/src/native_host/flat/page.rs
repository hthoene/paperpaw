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
        return Some(events::PageTurn {
            now_ms,
            section: reading.section,
            offset: reading.offset,
            progress_ppm: ppm(reading.progress),
            path: &reading.path,
            title: &reading.title,
            chapter: &reading.chapter,
            page: reading.page,
            pages: reading.pages,
        });
    }
    Some(events::PageTurn {
        now_ms,
        section: u32::from_param(params.get("section")?)?,
        offset: u32::from_param(params.get("offset")?)?,
        progress_ppm: ppm(f32::from_param(params.get("progress")?)?),
        path: params.get("path")?,
        title: params.get("title")?,
        chapter: params.get("chapter")?,
        page: optional(params, "page")?,
        pages: optional(params, "pages")?,
    })
}

fn optional(params: &Params, key: &str) -> Option<Option<u32>> {
    match params.get(key) {
        Some(value) => Some(Some(u32::from_param(value)?)),
        None => Some(None),
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Kernel contract rounds clamped percent; Rust maps NaN to zero"
)]
fn ppm(percent: f32) -> u32 {
    (((percent.clamp(0.0, 100.0) * 10_000.0) + 0.5) as u32).min(1_000_000)
}
