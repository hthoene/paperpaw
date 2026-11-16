//! Round shapes: circles, rings and arcs of them, and points around a
//! centre (e.g. a clock's hands), in whole numbers only: small devices
//! have no floating-point unit.
//!
//! Angles are whole degrees clockwise from twelve o'clock, as on a clock
//! face: 0 up, 90 right, 180 down, 270 left.

use paperpaw_platform::Point;

/// What [`sin_cos`] scales its values to (1.0).
pub const UNIT: i32 = 10_000;

/// The sine of 0 to 90 degrees, times [`UNIT`].
const SINES: [i16; 91] = [
    0, 175, 349, 523, 698, 872, 1045, 1219, 1392, 1564, 1736, 1908, 2079, 2250, 2419, 2588, 2756,
    2924, 3090, 3256, 3420, 3584, 3746, 3907, 4067, 4226, 4384, 4540, 4695, 4848, 5000, 5150, 5299,
    5446, 5592, 5736, 5878, 6018, 6157, 6293, 6428, 6561, 6691, 6820, 6947, 7071, 7193, 7314, 7431,
    7547, 7660, 7771, 7880, 7986, 8090, 8192, 8290, 8387, 8480, 8572, 8660, 8746, 8829, 8910, 8988,
    9063, 9135, 9205, 9272, 9336, 9397, 9455, 9511, 9563, 9613, 9659, 9703, 9744, 9781, 9816, 9848,
    9877, 9903, 9925, 9945, 9962, 9976, 9986, 9994, 9998, 10_000,
];

/// How far row `row` of a shape `height` rows high with corners of
/// `radius` starts in from its sides (at most half its height and width,
/// as the caller makes sure): the pixels the circle leaves at least half
/// of, at the middle of the row.
pub fn corner_inset(radius: u32, row: u32, height: u32) -> u32 {
    let from_edge = row.min(height - 1 - row);
    if from_edge >= radius {
        return 0;
    }
    // The circle's half width at the middle of the row, in half pixels:
    // the square root of `square`, `dx` rounded down.
    let r = 2 * u64::from(radius);
    let dy = r - 2 * u64::from(from_edge) - 1;
    let square = r * r - dy * dy;
    let dx = square.isqrt();
    // The inset (r - root) / 2 pixels, rounded to the nearest (half a
    // pixel in): where the root is no whole number it lies above `dx`.
    ((r + 1 - dx - u64::from(dx * dx != square)) / 2) as u32
}

/// The sine and cosine of `degrees` (any, taken round the circle), times
/// [`UNIT`].
pub fn sin_cos(degrees: i32) -> (i32, i32) {
    let d = degrees.rem_euclid(360);
    let sin = |d: i32| -> i32 {
        match d {
            0..=90 => i32::from(SINES[d as usize]),
            91..=180 => i32::from(SINES[(180 - d) as usize]),
            181..=270 => -i32::from(SINES[(d - 180) as usize]),
            _ => -i32::from(SINES[(360 - d) as usize]),
        }
    };
    (sin(d), sin((d + 90).rem_euclid(360)))
}

/// The point `radius` pixels from `center` in the direction of `degrees`
/// (clockwise from twelve o'clock), rounded to the nearest pixel (and to
/// the edges of the plane of points, where it would lie past them).
pub fn point_at(center: Point, radius: i32, degrees: i32) -> Point {
    let (sin, cos) = sin_cos(degrees);
    let scale = |v: i32| {
        (i64::from(v) * i64::from(radius) * 2 + i64::from(UNIT)).div_euclid(2 * i64::from(UNIT))
    };
    let clamp = |v: i64| v.clamp(i32::MIN.into(), i32::MAX.into()) as i32;
    Point::new(
        clamp(i64::from(center.x) + scale(sin)),
        clamp(i64::from(center.y) - scale(cos)),
    )
}

/// Whether direction `b` lies less than half a turn clockwise after
/// direction `a` (both given as east and north parts).
fn clockwise_after(a: (i64, i64), b: (i64, i64)) -> bool {
    // With north as the first axis and east as the second, clockwise on a
    // clock face is the positive turn of the plane.
    a.1 * b.0 - a.0 * b.1 > 0
}

/// An arc of a ring: where it starts and how far it goes round.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Arc {
    from: (i64, i64),
    to: (i64, i64),
    sweep: u32,
}

impl Arc {
    /// `sweep` degrees clockwise from `from`; the whole ring from 360.
    pub(crate) fn new(from: i32, sweep: u32) -> Self {
        let direction = |d: i32| {
            let (sin, cos) = sin_cos(d);
            (i64::from(sin), i64::from(cos))
        };
        Self {
            from: direction(from),
            to: direction(from.rem_euclid(360) + sweep.min(360) as i32),
            sweep: sweep.min(360),
        }
    }

    pub(crate) fn full(&self) -> bool {
        self.sweep >= 360
    }

    /// Whether the direction east `x`, north `y` lies on the arc: from
    /// `from` on, up to `to`.
    pub(crate) fn holds(&self, x: i64, y: i64) -> bool {
        if self.full() {
            return true;
        }
        if self.sweep == 0 {
            return false;
        }
        let p = (x, y);
        if self.sweep == 180 {
            // `to` is `from` reversed: the half plane clockwise of `from`
            // and the direction of `from` itself, so that the two halves
            // of a ring share no edge and leave none out.
            let (a, b) = (self.from, p);
            let along = a.1 * b.0 == a.0 * b.1 && a.0 * b.0 + a.1 * b.1 > 0;
            return clockwise_after(a, b) || along;
        }
        let between = |a, b| !clockwise_after(p, a) && clockwise_after(p, b);
        if self.sweep <= 180 {
            between(self.from, self.to)
        } else {
            // The rest of the ring is less than half a turn.
            !(!clockwise_after(p, self.to) && clockwise_after(p, self.from))
        }
    }
}

/// A ring (or a disc, as thick as its radius), or an arc of it, and how
/// much of each pixel around its centre it covers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ring {
    /// Its outer and inner radius, in eighths of a pixel.
    outer: i64,
    inner: i64,
    arc: Arc,
}

/// Half a pixel's diagonal, in eighths of a pixel, rounded up.
const HALF_DIAGONAL: i64 = 6;

impl Ring {
    pub(crate) fn new(radius: u32, thickness: u32, arc: Arc) -> Self {
        Self {
            outer: 8 * i64::from(radius),
            inner: 8 * i64::from(radius.saturating_sub(thickness)),
            arc,
        }
    }

    fn holds(&self, east: i64, north: i64) -> bool {
        let d = east * east + north * north;
        d <= self.outer * self.outer && d > self.inner * self.inner && self.arc.holds(east, north)
    }

    /// The columns of row `y` (pixels below the centre) that the ring may
    /// cover, as `coverage` tells: the others cover nothing. Up to two
    /// stretches `(first, last)`, of pixels right of the centre; two
    /// where the row crosses the ring's hole.
    pub(crate) fn stretches(&self, y: i64) -> [Option<(i64, i64)>; 2] {
        let north = -(8 * y + 4);
        // The columns whose centre is at most `reach` eighths from the
        // middle.
        let columns = |reach: i64| (-((reach + 4).div_euclid(8)), (reach - 4).div_euclid(8));
        let beyond = self.outer + HALF_DIAGONAL + 1;
        let outer = beyond * beyond - north * north;
        if outer < 1 {
            return [None, None];
        }
        let (first, last) = columns((outer - 1).isqrt());
        let within = self.inner - HALF_DIAGONAL + 1;
        let hole = within * within - north * north;
        if within <= 0 || hole < 1 {
            return [(first <= last).then_some((first, last)), None];
        }
        let (hole_first, hole_last) = columns((hole - 1).isqrt());
        if hole_first > hole_last {
            return [(first <= last).then_some((first, last)), None];
        }
        [
            (first < hole_first).then_some((first, (hole_first - 1).min(last))),
            (hole_last < last).then_some(((hole_last + 1).max(first), last)),
        ]
    }

    /// The columns of row `y` that a full ring certainly covers in whole
    /// (`coverage` says 255), as [`stretches`](Self::stretches) gives
    /// them; none where the ring is an arc, whose edges are not round.
    pub(crate) fn solid(&self, y: i64) -> [Option<(i64, i64)>; 2] {
        if !self.arc.full() {
            return [None, None];
        }
        let north = -(8 * y + 4);
        let columns = |reach: i64| (-((reach + 4).div_euclid(8)), (reach - 4).div_euclid(8));
        // `distance + HALF_DIAGONAL <= outer` is `square < (outer -
        // HALF_DIAGONAL + 1)²`, and `distance - HALF_DIAGONAL > inner` is
        // `square >= (inner + HALF_DIAGONAL + 1)²`.
        let within = self.outer - HALF_DIAGONAL + 1;
        let reach = within * within - north * north;
        if within <= 0 || reach < 1 {
            return [None, None];
        }
        let (first, last) = columns((reach - 1).isqrt());
        if self.inner == 0 {
            return [(first <= last).then_some((first, last)), None];
        }
        let beyond = self.inner + HALF_DIAGONAL + 1;
        let hole = beyond * beyond - north * north;
        if hole < 1 {
            return [(first <= last).then_some((first, last)), None];
        }
        let (hole_first, hole_last) = columns((hole - 1).isqrt());
        if hole_first > hole_last {
            return [(first <= last).then_some((first, last)), None];
        }
        [
            (first < hole_first).then_some((first, (hole_first - 1).min(last))),
            (hole_last < last).then_some(((hole_last + 1).max(first), last)),
        ]
    }

    /// How much of the pixel `x`, `y` pixels right of and below the
    /// centre (the corner between four pixels) the ring covers, 0 to 255:
    /// pixels on its edges sampled sixteen times.
    pub(crate) fn coverage(&self, x: i64, y: i64) -> u8 {
        let east = 8 * x + 4;
        let north = -(8 * y + 4);
        let square = east * east + north * north;
        // Far outside or inside the band: told by squares, without the
        // square root (the most of a ring's box, which the root would
        // take a long while to say no to on a device without a
        // floating-point unit). `distance - HALF_DIAGONAL > outer` is
        // `square >= (outer + HALF_DIAGONAL + 1)²`, and `distance +
        // HALF_DIAGONAL <= inner` is `square < (inner - HALF_DIAGONAL +
        // 1)²`.
        let beyond = self.outer + HALF_DIAGONAL + 1;
        let within = self.inner - HALF_DIAGONAL + 1;
        if square >= beyond * beyond || (within > 0 && square < within * within) {
            return 0;
        }
        let distance = square.isqrt();
        if distance - HALF_DIAGONAL > self.outer
            || (self.inner > 0 && distance + HALF_DIAGONAL <= self.inner)
        {
            return 0;
        }
        let radial = distance + HALF_DIAGONAL <= self.outer
            && (self.inner == 0 || distance - HALF_DIAGONAL > self.inner);
        let corners = [(-4, -4), (-4, 4), (4, -4), (4, 4)]
            .iter()
            .filter(|(dx, dy)| self.arc.holds(east + dx, north + dy))
            .count();
        if corners == 0 && !self.arc.full() {
            return 0;
        }
        if radial && (corners == 4 || self.arc.full()) {
            return 255;
        }
        let mut inside = 0u32;
        for sy in [-3, -1, 1, 3] {
            for sx in [-3, -1, 1, 3] {
                if self.holds(east + sx, north + sy) {
                    inside += 1;
                }
            }
        }
        (inside * 255 / 16) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sines_go_round() {
        assert_eq!(sin_cos(0), (0, UNIT));
        assert_eq!(sin_cos(90), (UNIT, 0));
        assert_eq!(sin_cos(180), (0, -UNIT));
        assert_eq!(sin_cos(270), (-UNIT, 0));
        assert_eq!(sin_cos(-90), sin_cos(270));
        assert_eq!(sin_cos(30).0, 5000);
        assert_eq!(sin_cos(420), sin_cos(60));
    }

    #[test]
    fn points_lie_where_a_clock_shows() {
        let c = Point::new(100, 100);
        assert_eq!(point_at(c, 50, 0), Point::new(100, 50));
        assert_eq!(point_at(c, 50, 90), Point::new(150, 100));
        assert_eq!(point_at(c, 50, 180), Point::new(100, 150));
        assert_eq!(point_at(c, 50, 270), Point::new(50, 100));
        // Past the plane's edges: at them.
        let far = Point::new(i32::MAX, i32::MIN);
        assert_eq!(point_at(far, i32::MAX, 45), Point::new(i32::MAX, i32::MIN));
        // 1_518_485_687.5 and a little more to the left: rounded to the nearest.
        assert_eq!(point_at(far, i32::MIN, 45).x, i32::MAX - 1_518_485_688);
        // Any angle, the largest too (127 degrees round the circle).
        assert!(Arc::new(i32::MAX, 90).holds(2, -10));
        assert!(!Arc::new(i32::MAX, 90).holds(-10, -1));
    }

    #[test]
    fn arcs_hold_what_they_sweep() {
        // A quarter from twelve: the upper right only.
        let quarter = Arc::new(0, 90);
        assert!(quarter.holds(10, 10));
        assert!(!quarter.holds(-10, 10));
        assert!(!quarter.holds(10, -10));
        // Three quarters from twelve: all but the upper left.
        let most = Arc::new(0, 270);
        assert!(most.holds(10, 10) && most.holds(10, -10) && most.holds(-10, -10));
        assert!(!most.holds(-10, 10));
        // From three o'clock, half a turn: the lower half.
        let half = Arc::new(90, 180);
        assert!(half.holds(5, -10) && half.holds(-5, -10));
        assert!(!half.holds(5, 10));
        assert!(Arc::new(0, 360).holds(-1, 1));
        assert!(!Arc::new(0, 0).holds(1, 1));
    }

    /// What `coverage` said before it told far pixels by squares: the same
    /// everywhere.
    #[test]
    fn coverage_is_what_the_square_root_says() {
        fn plain(ring: &Ring, x: i64, y: i64) -> u8 {
            let east = 8 * x + 4;
            let north = -(8 * y + 4);
            let distance = (east * east + north * north).isqrt();
            if distance - HALF_DIAGONAL > ring.outer
                || (ring.inner > 0 && distance + HALF_DIAGONAL <= ring.inner)
            {
                return 0;
            }
            let radial = distance + HALF_DIAGONAL <= ring.outer
                && (ring.inner == 0 || distance - HALF_DIAGONAL > ring.inner);
            let corners = [(-4, -4), (-4, 4), (4, -4), (4, 4)]
                .iter()
                .filter(|(dx, dy)| ring.arc.holds(east + dx, north + dy))
                .count();
            if corners == 0 && !ring.arc.full() {
                return 0;
            }
            if radial && (corners == 4 || ring.arc.full()) {
                return 255;
            }
            let mut inside = 0u32;
            for sy in [-3, -1, 1, 3] {
                for sx in [-3, -1, 1, 3] {
                    if ring.holds(east + sx, north + sy) {
                        inside += 1;
                    }
                }
            }
            (inside * 255 / 16) as u8
        }
        for (radius, thickness, from, sweep) in [
            (10, 10, 0, 360),
            (10, 2, 0, 360),
            (37, 5, 0, 120),
            (80, 1, 30, 270),
            (80, 8, 0, 200),
            (3, 1, 0, 360),
            (1, 1, 0, 360),
            (5, 9, 0, 360),
        ] {
            let ring = Ring::new(radius, thickness, Arc::new(from, sweep));
            let r = i64::from(radius);
            for y in -r - 3..r + 3 {
                for x in -r - 3..r + 3 {
                    assert_eq!(
                        ring.coverage(x, y),
                        plain(&ring, x, y),
                        "ring {radius}/{thickness} at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn rings_cover_their_pixels() {
        let disc = Ring::new(10, 10, Arc::new(0, 360));
        assert_eq!(disc.coverage(0, 0), 255);
        assert_eq!(disc.coverage(-10, -10), 0);
        assert_eq!(disc.coverage(20, 0), 0);
        // On the edge: partly.
        let edge = disc.coverage(7, 6);
        assert!(edge > 0 && edge < 255, "{edge}");
        // A ring 2 wide: its middle empty, its band full.
        let ring = Ring::new(10, 2, Arc::new(0, 360));
        assert_eq!(ring.coverage(0, 0), 0);
        assert_eq!(ring.coverage(-1, -9), 255);
        // Half of it, the right side: the left of the band empty.
        let half = Ring::new(10, 2, Arc::new(0, 180));
        assert_eq!(half.coverage(-2, -9), 0);
        assert_eq!(half.coverage(1, 8), 255);
    }

    /// What `solid` gives is covered in whole, in full rings and discs.
    #[test]
    fn solid_stretches_are_covered_in_whole() {
        for radius in [1_u32, 2, 5, 17, 60, 230] {
            for thickness in [0, 1, 3, radius / 2, radius] {
                let ring = Ring::new(radius, thickness, Arc::new(0, 360));
                let r = i64::from(radius);
                for y in -r - 2..r + 2 {
                    for (first, last) in ring.solid(y).into_iter().flatten() {
                        for x in first..=last {
                            assert_eq!(ring.coverage(x, y), 255, "r{radius} t{thickness} {x},{y}");
                        }
                    }
                }
            }
        }
        // Arcs have no solid stretch.
        let arc = Ring::new(30, 5, Arc::new(0, 90));
        assert!((-30..30).all(|y| arc.solid(y) == [None, None]));
    }
}
