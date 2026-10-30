//! Recognizes long presses and gestures from raw input.

use alloc::vec::Vec;

use paperpaw_platform::{ButtonState, Instant, Point, PointerEvent, PointerPhase, PointerTool};
use paperpaw_sdk::{Direction, Gesture, GestureKind};

/// Recognition thresholds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Thresholds {
    pub long_press_ms: u64,
    /// Movement in pixels below which a touch still counts as a tap.
    pub tap_slop: u32,
    /// Movement in pixels from which a touch counts as a swipe.
    pub swipe_distance: u32,
}

/// The result of feeding an event or a tick into the recognizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Recognized {
    Button { name: &'static str, long: bool },
    Gesture(Gesture),
}

#[derive(Debug)]
struct HeldButton {
    name: &'static str,
    since: Instant,
    /// Recognized already: as a long press, or on the press itself.
    long_fired: bool,
}

#[derive(Debug)]
struct TrackedPointer {
    id: u8,
    start: Point,
    last: Point,
    since: Instant,
    tool: PointerTool,
    moved: bool,
    long_fired: bool,
    /// Lifted as part of two fingers: no tap or swipe of its own.
    paired: bool,
    /// Taken by the kernel at an edge: followed for its long press only,
    /// what its lift makes is the edge's.
    edge: Option<Edge>,
}

/// Where a finger taken at an edge came down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
    /// At the top or the bottom: held still and alone, a long press (the
    /// first line of a page, the keyboard's bottom row).
    End,
    /// At a side, where a thumb rests holding the device: never a long
    /// press.
    Side,
}

#[derive(Debug, Default)]
pub(crate) struct Recognizer {
    buttons: Vec<HeldButton>,
    pointers: Vec<TrackedPointer>,
}

impl Recognizer {
    /// A button going down or up. With `at_once` (nothing is bound to a
    /// long press of it) it counts as pressed when it goes down, not when
    /// it is let go: a page turns as the button is pressed.
    pub(crate) fn button(
        &mut self,
        name: &'static str,
        state: ButtonState,
        now: Instant,
        at_once: bool,
    ) -> Option<Recognized> {
        let index = self.buttons.iter().position(|b| b.name == name);
        match (state, index) {
            (ButtonState::Pressed, None) => {
                self.buttons.push(HeldButton {
                    name,
                    since: now,
                    long_fired: at_once,
                });
                at_once.then_some(Recognized::Button { name, long: false })
            }
            // Key repeat or a missed release: keep the original press time.
            (ButtonState::Pressed, Some(_)) => None,
            (ButtonState::Released, Some(i)) => {
                let held = self.buttons.swap_remove(i);
                (!held.long_fired).then_some(Recognized::Button { name, long: false })
            }
            (ButtonState::Released, None) => None,
            // States of later releases.
            _ => None,
        }
    }

    /// Stops following button `name`: its press is over, its release
    /// makes nothing.
    pub(crate) fn forget_button(&mut self, name: &str) {
        self.buttons.retain(|b| b.name != name);
    }

    /// Whether button `name` is held: pressed and not let go yet.
    pub(crate) fn holds(&self, name: &str) -> bool {
        self.buttons.iter().any(|b| b.name == name)
    }

    /// Stops following finger `id`, without a gesture: it came down again,
    /// so its touch before is over.
    pub(crate) fn forget_pointer(&mut self, id: u8) {
        self.pointers.retain(|p| p.id != id);
    }

    /// Follows a finger that came down at an edge, so that held still and
    /// alone at the top or the bottom it becomes a long press as anywhere
    /// else. It counts as no finger for two fingers together. Its moves
    /// come through [`Self::pointer`], its lift through
    /// [`Self::lift_edge`].
    pub(crate) fn follow_edge(&mut self, event: &PointerEvent, now: Instant, edge: Edge) {
        self.down(event, now, Some(edge));
    }

    /// The finger at an edge was lifted at `now`: whether its lift makes
    /// nothing more, as it was a long press already or was held still as
    /// long (a thumb resting at a side).
    pub(crate) fn lift_edge(&mut self, id: u8, now: Instant, thresholds: &Thresholds) -> bool {
        let Some(index) = self.pointers.iter().position(|p| p.id == id) else {
            return false;
        };
        let pointer = self.pointers.swap_remove(index);
        pointer.long_fired
            || (!pointer.moved && now.since(pointer.since) >= thresholds.long_press_ms)
    }

    fn down(&mut self, event: &PointerEvent, now: Instant, edge: Option<Edge>) {
        self.forget_pointer(event.id);
        self.pointers.push(TrackedPointer {
            id: event.id,
            start: event.point,
            last: event.point,
            since: now,
            tool: event.tool,
            moved: false,
            long_fired: false,
            paired: false,
            edge,
        });
    }

    /// Tracks a pointer the screen did not capture. Returns a gesture when
    /// the pointer is lifted.
    pub(crate) fn pointer(
        &mut self,
        event: &PointerEvent,
        now: Instant,
        thresholds: &Thresholds,
    ) -> Option<Recognized> {
        let index = self.pointers.iter().position(|p| p.id == event.id);
        match (event.phase, index) {
            (PointerPhase::Down, _) => {
                self.down(event, now, None);
                None
            }
            (PointerPhase::Move, Some(i)) => {
                let pointer = &mut self.pointers[i];
                pointer.last = event.point;
                if distance(pointer.start, event.point) > thresholds.tap_slop {
                    pointer.moved = true;
                }
                None
            }
            (PointerPhase::Up, Some(i)) => {
                let mut pointer = self.pointers.swap_remove(i);
                pointer.last = event.point;
                if distance(pointer.start, event.point) > thresholds.tap_slop {
                    pointer.moved = true;
                }
                if pointer.long_fired || pointer.paired || pointer.edge.is_some() {
                    return None;
                }
                // Two fingers: a tap if neither moved, a pinch if they
                // moved apart or together; the other one then makes no
                // gesture of its own. Not with one at an edge, whose lift
                // is the edge's.
                if let Some(other) = self
                    .pointers
                    .iter_mut()
                    .find(|p| !p.paired && p.edge.is_none())
                {
                    other.paired = true;
                    let before = distance(pointer.start, other.start).max(1);
                    let now = distance(pointer.last, other.last);
                    let kind = if !pointer.moved && !other.moved {
                        GestureKind::TwoFingerTap
                    } else if now * 4 >= before * 5 {
                        GestureKind::Pinch { out: true }
                    } else if now * 5 <= before * 4 {
                        GestureKind::Pinch { out: false }
                    } else {
                        return None;
                    };
                    return Some(Recognized::Gesture(Gesture::new(
                        kind,
                        pointer.start,
                        pointer.last,
                        pointer.tool,
                    )));
                }
                let kind = if !pointer.moved {
                    GestureKind::Tap
                } else if distance(pointer.start, pointer.last) >= thresholds.swipe_distance {
                    GestureKind::Swipe(direction(pointer.start, pointer.last))
                } else {
                    return None;
                };
                Some(Recognized::Gesture(Gesture::new(
                    kind,
                    pointer.start,
                    pointer.last,
                    pointer.tool,
                )))
            }
            (PointerPhase::Cancel, Some(i)) => {
                self.pointers.swap_remove(i);
                None
            }
            (PointerPhase::Move | PointerPhase::Up | PointerPhase::Cancel, None) => None,
            // Phases of later releases.
            _ => None,
        }
    }

    /// Fires long presses that are due at `now`.
    pub(crate) fn tick(&mut self, now: Instant, thresholds: &Thresholds) -> Vec<Recognized> {
        let mut fired = Vec::new();
        let (together, alone) = (self.together(), self.pointers.len() == 1);
        for button in &mut self.buttons {
            if !button.long_fired && now.since(button.since) >= thresholds.long_press_ms {
                button.long_fired = true;
                fired.push(Recognized::Button {
                    name: button.name,
                    long: true,
                });
            }
        }
        for pointer in &mut self.pointers {
            if pointer.waits(together, alone)
                && now.since(pointer.since) >= thresholds.long_press_ms
            {
                pointer.long_fired = true;
                fired.push(Recognized::Gesture(Gesture::new(
                    GestureKind::LongPress,
                    pointer.start,
                    pointer.last,
                    pointer.tool,
                )));
            }
        }
        fired
    }

    /// When the next long press could fire, if any: as [`Self::tick`]
    /// fires them, so that a time asked for is never one already past.
    pub(crate) fn next_deadline(&self, thresholds: &Thresholds) -> Option<Instant> {
        let (together, alone) = (self.together(), self.pointers.len() == 1);
        let buttons = self
            .buttons
            .iter()
            .filter(|b| !b.long_fired)
            .map(|b| b.since);
        let pointers = self
            .pointers
            .iter()
            .filter(|p| p.waits(together, alone))
            .map(|p| p.since);
        buttons
            .chain(pointers)
            .min()
            .map(|since| since.add_millis(thresholds.long_press_ms))
    }

    /// Two fingers down, not counting one at an edge (a thumb resting
    /// there): no long press, maybe a pinch.
    fn together(&self) -> bool {
        self.pointers.iter().filter(|p| p.edge.is_none()).count() > 1
    }
}

impl TrackedPointer {
    /// Whether it may still become a long press: held still, not part of
    /// two fingers before, and not with another (`together`); at an edge,
    /// only at the top or the bottom, the only finger down (`alone`).
    fn waits(&self, together: bool, alone: bool) -> bool {
        let still = !self.long_fired && !self.moved && !self.paired;
        still
            && match self.edge {
                None => !together,
                Some(Edge::End) => alone,
                Some(Edge::Side) => false,
            }
    }
}

pub(crate) fn distance(a: Point, b: Point) -> u32 {
    let dx = i64::from(a.x) - i64::from(b.x);
    let dy = i64::from(a.y) - i64::from(b.y);
    isqrt((dx * dx + dy * dy) as u64) as u32
}

fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

pub(crate) fn direction(from: Point, to: Point) -> Direction {
    let dx = i64::from(to.x) - i64::from(from.x);
    let dy = i64::from(to.y) - i64::from(from.y);
    if dx.abs() >= dy.abs() {
        if dx < 0 {
            Direction::Left
        } else {
            Direction::Right
        }
    } else if dy < 0 {
        Direction::Up
    } else {
        Direction::Down
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Thresholds = Thresholds {
        long_press_ms: 500,
        tap_slop: 10,
        swipe_distance: 50,
    };

    fn pointer(phase: PointerPhase, x: i32, y: i32) -> PointerEvent {
        PointerEvent::new(0, phase, Point::new(x, y), PointerTool::Finger, 1000)
    }

    #[test]
    fn without_a_long_press_at_once() {
        let mut r = Recognizer::default();
        let pressed = Recognized::Button {
            name: "page-forward",
            long: false,
        };
        assert_eq!(
            r.button("page-forward", ButtonState::Pressed, Instant(0), true),
            Some(pressed)
        );
        // Held, it does not fire again; let go, nothing more.
        assert!(r.tick(Instant(800), &T).is_empty());
        assert_eq!(
            r.button("page-forward", ButtonState::Released, Instant(900), true),
            None
        );
    }

    #[test]
    fn short_and_long_button_presses() {
        let mut r = Recognizer::default();
        assert_eq!(
            r.button("confirm", ButtonState::Pressed, Instant(0), false),
            None
        );
        assert_eq!(
            r.button("confirm", ButtonState::Released, Instant(100), false),
            Some(Recognized::Button {
                name: "confirm",
                long: false
            })
        );

        r.button("confirm", ButtonState::Pressed, Instant(1000), false);
        assert_eq!(r.next_deadline(&T), Some(Instant(1500)));
        assert!(r.tick(Instant(1400), &T).is_empty());
        assert_eq!(
            r.tick(Instant(1500), &T),
            [Recognized::Button {
                name: "confirm",
                long: true
            }]
        );
        assert_eq!(r.next_deadline(&T), None);
        // Releasing after a long press produces nothing more.
        assert_eq!(
            r.button("confirm", ButtonState::Released, Instant(1600), false),
            None
        );
    }

    #[test]
    fn tap_swipe_and_long_press() {
        let mut r = Recognizer::default();
        r.pointer(&pointer(PointerPhase::Down, 100, 100), Instant(0), &T);
        let tap = r.pointer(&pointer(PointerPhase::Up, 104, 98), Instant(80), &T);
        assert!(matches!(
            tap,
            Some(Recognized::Gesture(Gesture {
                kind: GestureKind::Tap,
                ..
            }))
        ));

        r.pointer(&pointer(PointerPhase::Down, 300, 100), Instant(0), &T);
        r.pointer(&pointer(PointerPhase::Move, 200, 110), Instant(50), &T);
        let swipe = r.pointer(&pointer(PointerPhase::Up, 150, 112), Instant(90), &T);
        assert!(matches!(
            swipe,
            Some(Recognized::Gesture(Gesture {
                kind: GestureKind::Swipe(Direction::Left),
                ..
            }))
        ));

        r.pointer(&pointer(PointerPhase::Down, 10, 10), Instant(0), &T);
        let fired = r.tick(Instant(600), &T);
        assert!(matches!(
            fired.as_slice(),
            [Recognized::Gesture(Gesture {
                kind: GestureKind::LongPress,
                ..
            })]
        ));
        assert_eq!(
            r.pointer(&pointer(PointerPhase::Up, 10, 10), Instant(700), &T),
            None
        );
    }

    fn finger(id: u8, phase: PointerPhase, x: i32, y: i32) -> PointerEvent {
        pointer(phase, x, y).with_id(id)
    }

    #[test]
    fn two_fingers_pinch() {
        let mut r = Recognizer::default();
        for (out, from, to) in [(false, 40, 10), (true, 10, 40)] {
            r.pointer(
                &finger(1, PointerPhase::Down, 200 - from, 300),
                Instant(0),
                &T,
            );
            r.pointer(
                &finger(2, PointerPhase::Down, 200 + from, 300),
                Instant(10),
                &T,
            );
            // Held long, but two fingers make no long press.
            assert!(r.tick(Instant(600), &T).is_empty());
            r.pointer(
                &finger(1, PointerPhase::Move, 200 - to, 300),
                Instant(700),
                &T,
            );
            r.pointer(
                &finger(2, PointerPhase::Move, 200 + to, 300),
                Instant(700),
                &T,
            );
            let pinch = r.pointer(
                &finger(1, PointerPhase::Up, 200 - to, 300),
                Instant(750),
                &T,
            );
            assert!(matches!(
                pinch,
                Some(Recognized::Gesture(Gesture {
                    kind: GestureKind::Pinch { out: o },
                    ..
                })) if o == out
            ));
            // The second finger makes nothing of its own.
            assert_eq!(
                r.pointer(
                    &finger(2, PointerPhase::Up, 200 + to, 300),
                    Instant(760),
                    &T
                ),
                None
            );
        }
        // Two fingers that did not move: a tap of two.
        r.pointer(&finger(1, PointerPhase::Down, 100, 300), Instant(0), &T);
        r.pointer(&finger(2, PointerPhase::Down, 300, 300), Instant(0), &T);
        assert!(matches!(
            r.pointer(&finger(1, PointerPhase::Up, 102, 300), Instant(80), &T),
            Some(Recognized::Gesture(Gesture {
                kind: GestureKind::TwoFingerTap,
                ..
            }))
        ));
        assert_eq!(
            r.pointer(&finger(2, PointerPhase::Up, 300, 300), Instant(90), &T),
            None
        );
        // Two fingers, one moved a little: nothing.
        r.pointer(&finger(1, PointerPhase::Down, 100, 300), Instant(0), &T);
        r.pointer(&finger(2, PointerPhase::Down, 300, 300), Instant(0), &T);
        r.pointer(&finger(2, PointerPhase::Move, 320, 300), Instant(40), &T);
        assert_eq!(
            r.pointer(&finger(1, PointerPhase::Up, 100, 300), Instant(80), &T),
            None
        );
    }

    #[test]
    fn small_drags_are_ignored() {
        let mut r = Recognizer::default();
        r.pointer(&pointer(PointerPhase::Down, 0, 0), Instant(0), &T);
        assert_eq!(
            r.pointer(&pointer(PointerPhase::Up, 30, 0), Instant(50), &T),
            None
        );
    }

    #[test]
    fn integer_square_root() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(u64::from(u32::MAX)), 65535);
    }
}
