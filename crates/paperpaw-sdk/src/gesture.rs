use paperpaw_platform::{Point, PointerTool};

/// Direction of a swipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub const fn id(self) -> &'static str {
        match self {
            Direction::Left => "left",
            Direction::Right => "right",
            Direction::Up => "up",
            Direction::Down => "down",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GestureKind {
    Tap,
    LongPress,
    Swipe(Direction),
    /// Two fingers moved apart (`out`) or together: in readers the text
    /// larger or smaller.
    Pinch {
        out: bool,
    },
    /// Two taps close together in time and place (the second one; the
    /// first was a [`Tap`](Self::Tap)), to screens that take them
    /// ([`Screen::takes_double_taps`](crate::Screen::takes_double_taps)).
    DoubleTap,
    /// Two fingers tapped together.
    TwoFingerTap,
}

/// A gesture recognized by the kernel from raw pointer samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Gesture {
    pub kind: GestureKind,
    /// Where the gesture started.
    pub start: Point,
    /// Where it ended (equal to `start` for taps).
    pub end: Point,
    pub tool: PointerTool,
}

impl Gesture {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(kind: GestureKind, start: Point, end: Point, tool: PointerTool) -> Self {
        Self {
            kind,
            start,
            end,
            tool,
        }
    }
}
