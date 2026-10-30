//! What the key map binds, as plugins see it, e.g. to show and change
//! the bindings ([`Command::Bind`](crate::Command::Bind)).
//!
//! Inputs are named as in `paperpaw.ini`: `button.<name>`, `long.<name>`
//! (held), `double.<name>` (pressed twice), `tap.<zone>`,
//! `long-tap.<zone>`, `double-tap.<zone>` (zones [`ZONES`], or a layer's
//! own: `[zones.<layer>] <name> = <x> <y> <width> <height>` in percent of
//! the screen), `tap2` (two fingers), `swipe.<direction>` ([`SWIPES`]),
//! `edge.<side>` ([`EDGES`]), `pinch.<out|in>` ([`PINCHES`]), a plugin's
//! `input.<id>` ([`InputSpec`]) and `gesture.<id>` ([`GestureSpec`]).
//! Layers are screen or slot ids, and
//! [`GLOBAL_KEYMAP_LAYER`](crate::builtin::GLOBAL_KEYMAP_LAYER).

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_platform::{InputCaps, Instant, PointerEvent};

use crate::id::Id;
use crate::menus::MenuNeed;

/// The tap zones, a 3 by 3 grid over the screen.
pub const ZONES: [&str; 9] = [
    "top-left",
    "top",
    "top-right",
    "left",
    "center",
    "right",
    "bottom-left",
    "bottom",
    "bottom-right",
];
/// The directions of swipes.
pub const SWIPES: [&str; 4] = ["left", "right", "up", "down"];
/// The edges swipes in begin at.
pub const EDGES: [&str; 4] = ["top", "bottom", "left", "right"];
/// Two fingers moved apart or together.
pub const PINCHES: [&str; 2] = ["out", "in"];
/// How a screen takes gestures (`gestures.policy` in its layer): as
/// usual, its own (swipes in from an edge need a second), none.
pub const POLICIES: [&str; 3] = [
    "core.gestures.standard",
    "core.gestures.own",
    "core.gestures.off",
];

/// Every input a device with `caps` makes, as key maps name them: its
/// buttons pressed, held and pressed twice, and on touch screens taps,
/// long taps and double taps in each zone, two-finger taps, swipes,
/// swipes in from the edges and pinches. Plugins' inputs are in
/// [`Env::inputs`](crate::Env::inputs).
pub fn inputs(caps: &InputCaps) -> Vec<String> {
    let mut all = Vec::new();
    for key in caps.keys {
        for kind in ["button", "long", "double"] {
            all.push(format!("{kind}.{key}"));
        }
    }
    if caps.touch {
        for zone in ZONES {
            for kind in ["tap", "long-tap", "double-tap"] {
                all.push(format!("{kind}.{zone}"));
            }
        }
        all.push(String::from("tap2"));
        all.extend(SWIPES.iter().map(|d| format!("swipe.{d}")));
        all.extend(EDGES.iter().map(|e| format!("edge.{e}")));
        all.extend(PINCHES.iter().map(|p| format!("pinch.{p}")));
    }
    all
}

/// What the reader used last ([`Env::last_input`](crate::Env::last_input)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum InputMode {
    #[default]
    Buttons,
    Touch,
    Pen,
}

impl InputMode {
    /// What counts as used last before anything was: a touch screen is
    /// touched first, so no focus shows before a button is pressed
    /// ([`ui::focus_shown`](crate::ui::focus_shown)); buttons elsewhere.
    pub fn at_start(input: &InputCaps) -> Self {
        if input.touch {
            Self::Touch
        } else {
            Self::Buttons
        }
    }
}

/// An input of one's own (`input.<id>` in key maps), e.g. a chord a
/// plugin recognizes or a switch of a switch-access device: bound like
/// any other, raised with [`Command::Input`](crate::Command::Input).
/// [`Env::inputs`](crate::Env::inputs) lists them with the gestures
/// plugins recognize ([`InputKind::Gesture`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InputSpec {
    pub id: Id,
    /// Translation key of its name, for lists of inputs to bind.
    pub title: Cow<'static, str>,
    /// What the device must have for it.
    pub need: Option<MenuNeed>,
    /// An input of its own, or a gesture a recognizer makes.
    pub kind: InputKind,
}

/// What an input of [`Env::inputs`](crate::Env::inputs) is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum InputKind {
    /// `input.<id>` ([`Registrar::add_input`](crate::Registrar::add_input)).
    #[default]
    Input,
    /// `gesture.<id>` ([`Registrar::add_gesture`](crate::Registrar::add_gesture)).
    Gesture,
}

impl InputSpec {
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id,
            title: title.into(),
            need: None,
            kind: InputKind::Input,
        }
    }

    /// Its name in key maps: `input.<id>` or `gesture.<id>`.
    pub fn key(&self) -> String {
        match self.kind {
            InputKind::Gesture => format!("gesture.{}", self.id),
            _ => format!("input.{}", self.id),
        }
    }

    pub fn need(mut self, need: MenuNeed) -> Self {
        self.need = Some(need);
        self
    }
}

/// What a recognizer of gestures made of the time that went on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Recognition {
    /// Nothing yet.
    Nothing,
    /// Its gesture: `gesture.<id>` runs what it is bound to, and the
    /// fingers it follows are its own from now.
    Recognized,
}

/// What a recognizer does with a finger, told at each of its samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PointerVerdict {
    /// Not for it: it sees no more of this touch.
    Pass,
    /// It follows the finger, and the kernel recognizes it as usual (a
    /// tap, a long press, a swipe) beside it.
    Observe,
    /// The finger is its own: the kernel recognizes nothing of it any
    /// more.
    Capture,
    /// Its gesture: `gesture.<id>` runs what it is bound to, and the
    /// finger is its own (as [`Capture`](Self::Capture)).
    Recognized,
}

/// Recognizes a gesture of one's own from the fingers on the screen (a
/// circle, a zigzag, three fingers), in the screen's coordinates, beside
/// what the kernel recognizes.
///
/// It sees a finger come down where gestures are the key map's: not on a
/// bar, at an edge, on a screen that took the finger or that takes
/// gestures itself (`gestures.policy`), nor while a screen guards the
/// device (unless the recognizer is of the guard's plugin). Then it sees
/// the samples of the fingers it follows ([`PointerVerdict`]); a finger
/// taken away (by an input filter, or as a screen comes up that guards
/// the device) comes as
/// [`PointerPhase::Cancel`](paperpaw_platform::PointerPhase::Cancel), at a
/// point that may not be where it was.
pub trait GestureRecognizer {
    /// A sample of a finger at `now`.
    fn pointer(&mut self, event: &PointerEvent, now: Instant) -> PointerVerdict;

    /// Time went on to `now` (asked with [`next_tick`](Self::next_tick)).
    fn tick(&mut self, now: Instant) -> Recognition {
        let _ = now;
        Recognition::Nothing
    }

    /// When it wants [`tick`](Self::tick), e.g. for a gesture of holding;
    /// asked again after each, at most twenty times a second.
    fn next_tick(&self) -> Option<Instant> {
        None
    }

    /// A call that comes after this release, by its key and parameters:
    /// what PaperPaw asks of recognizers later reaches those built now,
    /// which answer [`Handled::No`](crate::Handled::No) to keys they do
    /// not know.
    fn extended(&mut self, key: &Id, params: &crate::Params) -> crate::Handled {
        let _ = (key, params);
        crate::Handled::No
    }
}

/// Makes a recognizer.
pub type RecognizerFactory = Box<dyn Fn() -> Box<dyn GestureRecognizer>>;

/// A recognizer of gestures a plugin adds (`gesture.<id>` in key maps).
#[non_exhaustive]
pub struct GestureSpec {
    pub id: Id,
    /// Translation key of its name, for lists of inputs to bind.
    pub title: Cow<'static, str>,
    pub factory: RecognizerFactory,
}

impl GestureSpec {
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>, factory: RecognizerFactory) -> Self {
        Self {
            id,
            title: title.into(),
            factory,
        }
    }
}

impl fmt::Debug for GestureSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GestureSpec")
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// The bindings in effect.
pub trait KeyBindings {
    /// The action `input` runs, looked up in `layers` from the most
    /// specific (a screen, its slot) to `global`, as input is; `None` if
    /// it runs none.
    fn bound(&self, layers: &[&str], input: &str) -> Option<Id>;

    /// The bindings of layer `layer` alone: each input as key maps name
    /// it, the action (`None`: bound to nothing on purpose) and where the
    /// binding comes from.
    fn bindings(&self, layer: &str) -> Vec<(String, Option<Id>, BindingOrigin)> {
        let _ = layer;
        Vec::new()
    }
}

/// Where a binding comes from ([`KeyBindings::bindings`]); later ones
/// win.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum BindingOrigin {
    /// A plugin's key map.
    Plugin,
    /// The device's (its shell's).
    Device,
    /// The button layout the user chose.
    Preset,
    /// The user's own.
    User,
}
