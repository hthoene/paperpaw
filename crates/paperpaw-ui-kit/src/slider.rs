//! The slider the quick panel (and any other panel) shows on a touch screen:
//! a row for the title with `−`, the value and `+` tight at its right edge,
//! and below it the track across almost the whole width, a fingertip high.
//! This holds its geometry (where the track goes, where a tap or a finger
//! puts the knob, what a drag moves it to), shared by every screen with a
//! slider; drawing a track is [`parts::slider`](crate::parts::slider).
//! Without touch a number keeps its compact row with `−` and `+`.

use paperpaw_sdk::Rect;

/// The two rows of a slider in `area` (two `row`s high): the first, for the
/// title and the buttons, and the track in the second, `margin` from the
/// sides and `pad` from the row's edges, so that it spans almost the width.
#[must_use]
pub fn rows(area: Rect, row: u16, (margin, pad): (u16, u16)) -> (Rect, Rect) {
    let head = Rect::new(area.x, area.y, area.width, row);
    let low = Rect::new(area.x, area.y.saturating_add(row), area.width, row);
    (head, low.inset(margin, pad))
}

/// The radius of a slider's knob on `track`; the knob's centre goes along
/// the track less that at both ends.
#[must_use]
pub fn knob_radius(track: Rect) -> u16 {
    track.height / 3
}

/// The number a tap at pixel `x` of a slider's `track` sets, over `min..=max`
/// in steps of `step` (1 where 0), the nearest step; a tap beyond an end sets
/// that end.
#[must_use]
pub fn value_at(track: Rect, x: i32, (min, max, step): (i64, i64, i64)) -> i64 {
    // In u64 without a product that can overflow: a card plugin declares any
    // range, up to the whole of i64, and 128-bit helpers are not wanted on
    // the X4. The range's `span / run` is shared out first.
    let max = max.max(min);
    let span = max.abs_diff(min);
    let radius = i64::from(knob_radius(track));
    let run = (i64::from(track.width) - 2 * radius).max(1);
    let along = (i64::from(x) - i64::from(track.x) - radius).clamp(0, run);
    let (run, along) = (run as u64, along as u64);
    let (whole, rest) = (span / run, span % run);
    let offset = whole * along + (rest * along + run / 2) / run;
    let step = u64::try_from(step).unwrap_or(1).max(1);
    let offset = (offset.saturating_add(step / 2) / step)
        .saturating_mul(step)
        .min(span);
    min.checked_add_unsigned(offset).unwrap_or(max)
}

/// Where the knob of a slider on `track` is, at `value`: its centre's pixel.
#[must_use]
pub fn knob_x(track: Rect, value: i64, (min, max): (i64, i64)) -> i32 {
    let max = max.max(min);
    let radius = i64::from(knob_radius(track));
    let run = (i64::from(track.width) - 2 * radius).max(0) as u64;
    let span = max.abs_diff(min);
    let offset = value.clamp(min, max).abs_diff(min);
    // A pixel's worth is enough: both are scaled down to 32 bits first, so
    // that `run * offset` (a pixel count under 2^16) cannot overflow.
    let shift = (u64::BITS - span.leading_zeros()).saturating_sub(32);
    let (span, offset) = (span >> shift, offset >> shift);
    let along = (run * offset).checked_div(span).unwrap_or(0);
    let at = i64::from(track.x) + radius + along as i64;
    i32::try_from(at).unwrap_or(0)
}

/// A finger on a slider's track, from coming down to lifting (Beta 3's
/// `SliderDrag`): moved sideways it moves the knob as far from the value it
/// had as the finger from where it came down, in the slider's steps and
/// within its range, so it never jumps to where the finger lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drag {
    /// The host's id of the contact.
    pub contact: u32,
    /// The setting row of the slider.
    pub row: usize,
    track: Rect,
    range: (i64, i64, i64),
    start: i32,
    from: i64,
    /// The value the knob is dragged to.
    pub value: i64,
    moved: bool,
}

impl Drag {
    /// A finger came down at `x` on `track`, the number having `from`.
    #[must_use]
    pub const fn begin(
        (contact, row): (u32, usize),
        track: Rect,
        range: (i64, i64, i64),
        x: i32,
        from: i64,
    ) -> Self {
        Self {
            contact,
            row,
            track,
            range,
            start: x,
            from,
            value: from,
            moved: false,
        }
    }

    /// The track the finger came down on.
    #[must_use]
    pub const fn track(&self) -> Rect {
        self.track
    }

    /// Whether the finger moved further than a tap may wander.
    #[must_use]
    pub const fn moved(&self) -> bool {
        self.moved
    }

    /// The finger is at `x`: the new value when the knob moved to another.
    /// Within the reach of a tap (about 3 mm: three sevenths of the track's
    /// height, a fingertip) nothing moves yet.
    pub fn follow(&mut self, x: i32) -> Option<i64> {
        let slop = (i32::from(self.track.height) * 3 / 7).max(1);
        if !self.moved && (x - self.start).abs() < slop {
            return None;
        }
        self.moved = true;
        let (min, max, _) = self.range;
        let at = knob_x(self.track, self.from, (min, max)).saturating_add(x - self.start);
        let value = value_at(self.track, at, self.range);
        (value != self.value).then(|| {
            self.value = value;
            value
        })
    }
}
