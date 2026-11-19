//! What the launcher decides without a host: where the focus goes from a key,
//! what a tap hit, how a bar's value follows a key or a finger, how many
//! cards a room holds and the pages of the library. Nothing here allocates.
use paperpaw_sdk::{Point, Rect};

/// What a part of a screen is, to tell what choosing it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The card `n` of the home screen (the menu's row `n`).
    Card(usize),
    /// The icon `n` of the dock.
    Dock(usize),
    /// The round switch (or button) `n` of the panel.
    Toggle(usize),
    /// The bar `n` of the panel.
    Bar(usize),
    /// The book `n` of the library's page.
    Book(usize),
    /// The row `n` of the customising page.
    Edit(usize),
    /// The arrow to the page before or after in the library.
    Page(bool),
}

/// A part with the room it was drawn in, for the focus and for taps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zone {
    pub target: Target,
    pub rect: Rect,
}

/// A direction the focus is asked to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
    /// The next part in the order they were drawn in.
    Next,
    /// The one before.
    Previous,
}

impl Dir {
    /// The direction a navigation action names.
    #[must_use]
    pub fn of(action: &str) -> Option<Self> {
        Some(match action {
            "core.left" => Self::Left,
            "core.right" => Self::Right,
            "core.up" => Self::Up,
            "core.down" => Self::Down,
            "core.next" => Self::Next,
            "core.previous" => Self::Previous,
            _ => return None,
        })
    }
}

fn centre(rect: Rect) -> (i32, i32) {
    (
        i32::from(rect.x) + i32::from(rect.width) / 2,
        i32::from(rect.y) + i32::from(rect.height) / 2,
    )
}

/// The part the focus goes to from part `from` in `dir`. Left and right (and
/// next and previous) walk the parts in the order drawn, round the ends, so two
/// buttons reach every part; up and down go to the nearest part that lies that
/// way (across counts double), else, at the edge, round the ends too. `None`
/// for no parts.
#[must_use]
pub fn step(zones: &[Zone], from: usize, dir: Dir) -> Option<usize> {
    let count = zones.len();
    if count == 0 {
        return None;
    }
    let from = from.min(count - 1);
    let forward = (from + 1) % count;
    let back = (from + count - 1) % count;
    let (ax, ay) = centre(zones[from].rect);
    let along = |index: usize| -> Option<i32> {
        let (bx, by) = centre(zones[index].rect);
        let (main, cross) = match dir {
            Dir::Down => (by - ay, bx - ax),
            Dir::Up => (ay - by, bx - ax),
            _ => return None,
        };
        // Under a quarter of the part's own size is the same row.
        let size = i32::from(zones[from].rect.height);
        (main > size / 4).then(|| main + 2 * cross.abs())
    };
    match dir {
        Dir::Next | Dir::Right => Some(forward),
        Dir::Previous | Dir::Left => Some(back),
        Dir::Down | Dir::Up => (0..count)
            .filter(|index| *index != from)
            .filter_map(|index| along(index).map(|score| (score, index)))
            .min()
            .map(|(_, index)| index)
            .or(Some(if dir == Dir::Down { forward } else { back })),
    }
}

/// The part under `point`, the one drawn last where several are.
#[must_use]
pub fn hit(zones: &[Zone], point: Point) -> Option<usize> {
    zones.iter().rposition(|zone| {
        let (x, y) = (point.x, point.y);
        x >= i32::from(zone.rect.x)
            && y >= i32::from(zone.rect.y)
            && x < i32::from(zone.rect.x) + i32::from(zone.rect.width)
            && y < i32::from(zone.rect.y) + i32::from(zone.rect.height)
    })
}

/// How full a bar is, in thousandths, for `value` in `min..=max`.
#[must_use]
pub fn permille(value: i64, min: i64, max: i64) -> u16 {
    if max <= min {
        return 0;
    }
    let done = (value.clamp(min, max) - min).saturating_mul(1000) / (max - min);
    u16::try_from(done).unwrap_or(1000)
}

/// `value` after a key: one `step` more (`up`) or less, kept in `min..=max`.
#[must_use]
pub fn stepped(value: i64, (min, max, step): (i64, i64, i64), up: bool) -> i64 {
    let step = step.max(1);
    if up {
        value.saturating_add(step).min(max)
    } else {
        value.saturating_sub(step).max(min)
    }
}

/// The value a tap at `x` on a bar from `left` and `width` wide sets: the
/// nearest multiple of `step` from `min`.
#[must_use]
pub fn from_tap(x: i32, (left, width): (i32, i32), (min, max, step): (i64, i64, i64)) -> i64 {
    if width <= 0 || max <= min {
        return min;
    }
    let step = step.max(1);
    let at = i64::from((x - left).clamp(0, width));
    let raw = min + (max - min) * at / i64::from(width);
    let rounded = min + ((raw - min) + step / 2) / step * step;
    rounded.clamp(min, max)
}

/// The room a card needs at least, in thousandths of the height of the screen's
/// part, to be a target a finger and a reader can use: how many of `wanted`
/// cards of `min` pixels fit in `room` pixels with `gap` between them.
#[must_use]
pub fn cards_that_fit(room: i32, min: i32, gap: i32, wanted: usize) -> usize {
    let mut count = 0;
    let mut used = 0;
    while count < wanted {
        let next = used + min + if count > 0 { gap } else { 0 };
        if next > room {
            break;
        }
        used = next;
        count += 1;
    }
    count
}

/// The number of pages `books` make at `per` to a page (at least one).
#[must_use]
pub fn pages(books: usize, per: usize) -> usize {
    books.div_ceil(per.max(1)).max(1)
}

/// The percent shown for a progress: never over 100.
#[must_use]
pub fn percent(progress: u8) -> u8 {
    progress.min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(target: Target, x: u16, y: u16, w: u16, h: u16) -> Zone {
        Zone {
            target,
            rect: Rect::new(x, y, w, h),
        }
    }

    /// Two cards in a column, a dock of three below.
    fn home() -> [Zone; 5] {
        [
            zone(Target::Card(0), 20, 100, 440, 120),
            zone(Target::Card(1), 20, 240, 440, 80),
            zone(Target::Dock(0), 60, 700, 60, 60),
            zone(Target::Dock(1), 210, 700, 60, 60),
            zone(Target::Dock(2), 360, 700, 60, 60),
        ]
    }

    #[test]
    fn down_goes_to_the_next_card_then_to_the_dock_below_it() {
        let zones = home();
        assert_eq!(step(&zones, 0, Dir::Down), Some(1));
        // The dock's middle icon is under the cards' middle.
        assert_eq!(step(&zones, 1, Dir::Down), Some(3));
    }

    #[test]
    fn up_returns_along_the_same_way() {
        let zones = home();
        assert_eq!(step(&zones, 3, Dir::Up), Some(1));
        assert_eq!(step(&zones, 1, Dir::Up), Some(0));
    }

    #[test]
    fn right_and_left_walk_the_parts_in_the_order_drawn() {
        let zones = home();
        assert_eq!(step(&zones, 2, Dir::Right), Some(3));
        assert_eq!(step(&zones, 3, Dir::Left), Some(2));
    }

    #[test]
    fn a_key_at_the_edge_goes_round_instead_of_nowhere() {
        let zones = home();
        // Nothing below the dock: the first part.
        assert_eq!(step(&zones, 4, Dir::Down), Some(0));
        assert_eq!(step(&zones, 0, Dir::Up), Some(4));
        // Nothing right of the last icon, nor left of the first.
        assert_eq!(step(&zones, 4, Dir::Right), Some(0));
        assert_eq!(step(&zones, 2, Dir::Left), Some(1));
        assert_eq!(step(&zones, 1, Dir::Right), Some(2));
    }

    #[test]
    fn next_and_previous_follow_the_order_drawn() {
        let zones = home();
        assert_eq!(step(&zones, 1, Dir::Next), Some(2));
        assert_eq!(step(&zones, 0, Dir::Previous), Some(4));
        assert_eq!(step(&[], 0, Dir::Down), None);
    }

    #[test]
    fn a_row_of_switches_is_walked_sideways_and_down_goes_to_the_bar() {
        let zones = [
            zone(Target::Toggle(0), 20, 100, 90, 90),
            zone(Target::Toggle(1), 130, 100, 90, 90),
            zone(Target::Toggle(2), 240, 100, 90, 90),
            zone(Target::Toggle(3), 350, 100, 90, 90),
            zone(Target::Toggle(4), 20, 230, 90, 90),
            zone(Target::Bar(0), 20, 400, 440, 70),
        ];
        assert_eq!(step(&zones, 0, Dir::Right), Some(1));
        assert_eq!(step(&zones, 3, Dir::Down), Some(5));
        assert_eq!(step(&zones, 0, Dir::Down), Some(4));
    }

    #[test]
    fn taps_hit_the_part_drawn_last() {
        let zones = [
            zone(Target::Card(0), 0, 0, 100, 100),
            zone(Target::Toggle(0), 50, 50, 100, 100),
        ];
        assert_eq!(hit(&zones, Point { x: 70, y: 70 }), Some(1));
        assert_eq!(hit(&zones, Point { x: 10, y: 10 }), Some(0));
        assert_eq!(hit(&zones, Point { x: 200, y: 10 }), None);
        // The far edge is outside.
        assert_eq!(hit(&zones, Point { x: 100, y: 10 }), None);
    }

    #[test]
    fn a_bar_fills_by_its_range_and_follows_keys_to_its_ends() {
        assert_eq!(permille(40, 0, 100), 400);
        assert_eq!(permille(3, 0, 10), 300);
        assert_eq!(permille(150, 0, 100), 1000);
        assert_eq!(permille(5, 5, 5), 0);
        assert_eq!(stepped(95, (0, 100, 5), true), 100);
        assert_eq!(stepped(100, (0, 100, 5), true), 100);
        assert_eq!(stepped(5, (0, 100, 10), false), 0);
        assert_eq!(stepped(10, (8, 32, 0), false), 9);
    }

    #[test]
    fn a_tap_on_a_bar_sets_the_value_at_the_finger() {
        let range = (0, 100, 5);
        assert_eq!(from_tap(100, (100, 200), range), 0);
        assert_eq!(from_tap(200, (100, 200), range), 50);
        assert_eq!(from_tap(300, (100, 200), range), 100);
        assert_eq!(from_tap(50, (100, 200), range), 0);
        assert_eq!(from_tap(999, (100, 200), range), 100);
        // 37 % is not a multiple of 5.
        assert_eq!(from_tap(174, (100, 200), range), 35);
        assert_eq!(from_tap(5, (0, 0), range), 0);
    }

    #[test]
    fn cards_are_dropped_when_the_room_is_short() {
        assert_eq!(cards_that_fit(500, 100, 10, 3), 3);
        assert_eq!(cards_that_fit(215, 100, 10, 3), 2);
        assert_eq!(cards_that_fit(99, 100, 10, 3), 0);
        assert_eq!(cards_that_fit(1000, 100, 10, 0), 0);
    }

    #[test]
    fn pages_round_up_and_there_is_always_one() {
        assert_eq!(pages(0, 4), 1);
        assert_eq!(pages(4, 4), 1);
        assert_eq!(pages(5, 4), 2);
        assert_eq!(pages(9, 0), 9);
        assert_eq!(percent(250), 100);
    }
}
