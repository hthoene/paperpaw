//! Borrow page-turn data without constructing an owned Reading.

use crate::native_host::events;
use paperpaw_sdk::{Params, event::ParamValue};

pub(in crate::native_host) fn turn(now_ms: u64, params: &Params) -> Option<events::PageTurn<'_>> {
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
