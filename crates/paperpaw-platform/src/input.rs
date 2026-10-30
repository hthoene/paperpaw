//! Raw, device-neutral input events.
//!
//! Shells report *what happened physically* (a named button went down, a
//! finger or pen touched the screen), never *what it means*. The kernel
//! recognizes long presses and gestures and resolves them to actions
//! through user-configurable key maps, so input works the same way on every
//! device and can be customized completely.

use crate::geometry::Point;

/// Standard button names. Shells use these where a device has an equivalent
/// button; additional, device-specific buttons may use any other name made of
/// lowercase ASCII letters, digits and `-`.
pub mod buttons {
    pub const PAGE_FORWARD: &str = "page-forward";
    pub const PAGE_BACK: &str = "page-back";
    pub const UP: &str = "up";
    pub const DOWN: &str = "down";
    pub const LEFT: &str = "left";
    pub const RIGHT: &str = "right";
    pub const CONFIRM: &str = "confirm";
    pub const BACK: &str = "back";
    pub const MENU: &str = "menu";
    pub const HOME: &str = "home";
    pub const POWER: &str = "power";
}

/// Physical state change of a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ButtonState {
    Pressed,
    Released,
}

/// Phase of a pointer (finger, pen, mouse) interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    /// The interaction was aborted (e.g. palm rejection).
    Cancel,
}

/// What touches the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerTool {
    Finger,
    Pen,
    Eraser,
    Mouse,
}

/// A single pointer sample in logical screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PointerEvent {
    /// Distinguishes simultaneous pointers (multi-touch).
    pub id: u8,
    pub phase: PointerPhase,
    pub point: Point,
    pub tool: PointerTool,
    /// Pressure from 0 to 1000; 1000 for devices without pressure sensing.
    pub pressure: u16,
}

impl PointerEvent {
    /// Made of all its fields; the `with_` methods change one.
    pub const fn new(
        id: u8,
        phase: PointerPhase,
        point: Point,
        tool: PointerTool,
        pressure: u16,
    ) -> Self {
        Self {
            id,
            phase,
            point,
            tool,
            pressure,
        }
    }

    #[must_use]
    pub const fn with_id(mut self, id: u8) -> Self {
        self.id = id;
        self
    }

    #[must_use]
    pub const fn with_phase(mut self, phase: PointerPhase) -> Self {
        self.phase = phase;
        self
    }

    #[must_use]
    pub const fn with_point(mut self, point: Point) -> Self {
        self.point = point;
        self
    }

    #[must_use]
    pub const fn with_tool(mut self, tool: PointerTool) -> Self {
        self.tool = tool;
        self
    }

    #[must_use]
    pub const fn with_pressure(mut self, pressure: u16) -> Self {
        self.pressure = pressure;
        self
    }
}

/// An input event reported by a shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputEvent {
    Button {
        name: &'static str,
        state: ButtonState,
    },
    Pointer(PointerEvent),
}

/// Input hardware a device offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct InputCaps {
    /// Physical buttons or a keyboard are available.
    pub buttons: bool,
    /// A touch screen is available.
    pub touch: bool,
    /// A pen digitizer is available.
    pub pen: bool,
    /// The buttons it has (see [`buttons`]), for screens that list them;
    /// empty where not known.
    pub keys: &'static [&'static str],
}

impl InputCaps {
    /// Made of all its fields; the `with_` methods change one.
    pub const fn new(buttons: bool, touch: bool, pen: bool, keys: &'static [&'static str]) -> Self {
        Self {
            buttons,
            touch,
            pen,
            keys,
        }
    }

    #[must_use]
    pub const fn with_buttons(mut self, buttons: bool) -> Self {
        self.buttons = buttons;
        self
    }

    #[must_use]
    pub const fn with_touch(mut self, touch: bool) -> Self {
        self.touch = touch;
        self
    }

    #[must_use]
    pub const fn with_pen(mut self, pen: bool) -> Self {
        self.pen = pen;
        self
    }

    #[must_use]
    pub const fn with_keys(mut self, keys: &'static [&'static str]) -> Self {
        self.keys = keys;
        self
    }
}

/// A point in time in milliseconds on a monotonic clock with an arbitrary
/// origin (e.g. boot). Only differences are meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Instant(pub u64);

impl Instant {
    pub const fn millis(self) -> u64 {
        self.0
    }

    pub const fn add_millis(self, millis: u64) -> Instant {
        Instant(self.0.saturating_add(millis))
    }

    /// Milliseconds since `earlier`, or 0 if `earlier` is later.
    pub const fn since(self, earlier: Instant) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}
