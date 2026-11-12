//! Layered key maps: raw input → action id.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_platform::Point;
use paperpaw_sdk::builtin::GLOBAL_KEYMAP_LAYER;
use paperpaw_sdk::collections::VecMap;
use paperpaw_sdk::{Direction, Id};

/// Section prefix of key map layers in config files.
const SECTION_PREFIX: &str = "keymap.";
/// Section prefix of a layer's own tap zones.
const ZONES_PREFIX: &str = "zones.";
/// Value that removes a binding.
const UNBOUND: &str = "none";

/// One of nine tap zones (a 3×3 grid over the screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Zone {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Zone {
    /// The zone on the other side, left for right.
    pub(crate) fn mirrored(self) -> Zone {
        match self {
            Zone::TopLeft => Zone::TopRight,
            Zone::TopRight => Zone::TopLeft,
            Zone::Left => Zone::Right,
            Zone::Right => Zone::Left,
            Zone::BottomLeft => Zone::BottomRight,
            Zone::BottomRight => Zone::BottomLeft,
            other => other,
        }
    }

    const ALL: [Zone; 9] = [
        Zone::TopLeft,
        Zone::Top,
        Zone::TopRight,
        Zone::Left,
        Zone::Center,
        Zone::Right,
        Zone::BottomLeft,
        Zone::Bottom,
        Zone::BottomRight,
    ];

    fn id(self) -> &'static str {
        match self {
            Zone::TopLeft => "top-left",
            Zone::Top => "top",
            Zone::TopRight => "top-right",
            Zone::Left => "left",
            Zone::Center => "center",
            Zone::Right => "right",
            Zone::BottomLeft => "bottom-left",
            Zone::Bottom => "bottom",
            Zone::BottomRight => "bottom-right",
        }
    }

    fn from_id(id: &str) -> Option<Zone> {
        Zone::ALL.into_iter().find(|zone| zone.id() == id)
    }

    /// The zone containing `point` on a `width`×`height` screen.
    pub(crate) fn of(point: Point, width: u16, height: u16) -> Zone {
        let third = |value: i32, size: u16| {
            let size = i64::from(size.max(1));
            (i64::from(value).clamp(0, size - 1) * 3 / size) as usize
        };
        Zone::ALL[third(point.y, height) * 3 + third(point.x, width)]
    }
}

/// An input as written in key maps.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum InputKey {
    /// `button.<name>`
    Button(Cow<'static, str>),
    /// `long.<name>`
    LongButton(Cow<'static, str>),
    /// `tap.<zone>`
    Tap(Zone),
    /// `long-tap.<zone>`
    LongTap(Zone),
    /// `swipe.<direction>`
    Swipe(Direction),
    /// `edge.<side>`: a swipe in from the edge `side` (`top`, `bottom`,
    /// `left`, `right`), before the screen sees it: for going home,
    /// opening the quick panel, going back.
    Edge(Direction),
    /// `pinch.out`, `pinch.in`: two fingers moved apart or together.
    Pinch(bool),
    /// `double.<name>`: a button pressed twice.
    DoubleButton(Cow<'static, str>),
    /// `double-tap.<zone>`
    DoubleTap(Zone),
    /// `tap2`: two fingers tapped together.
    Tap2,
    /// `chord.<button>_<button>`: two buttons pressed together, their
    /// names in order (`chord.back_power` for `chord.power_back`).
    /// Reserved (`Capability::CHORDS`): read and kept, raised by none yet.
    Chord(String),
    /// `tap.<name>`, `long-tap.<name>` of a layer's own zone
    /// (`[zones.<layer>]`): whether it is long, its name.
    ZoneTap(bool, String),
    /// `input.<id>`: an input a plugin added.
    Named(Id),
    /// `gesture.<id>`: a gesture a plugin's recognizer made.
    Gesture(Id),
    /// `gestures.policy`: how a screen takes gestures, bound to one of
    /// [`POLICIES`] (see [`Policy`]).
    Policy,
}

/// The side an edge swipe starts from, as written in key maps.
pub(crate) fn side_id(side: Direction) -> &'static str {
    match side {
        Direction::Up => "top",
        Direction::Down => "bottom",
        Direction::Left => "left",
        _ => "right",
    }
}

/// How a screen takes gestures (`gestures.policy` in its key map layer):
/// a tool sets its default, the user may change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Policy {
    /// Swipes in from the edges and the key map's gestures apply.
    Standard,
    /// The screen's own: gestures go to it, swipes in from an edge need a
    /// second one to apply (no going home by accident while playing).
    Own,
    /// No gestures but the screen's own; buttons still work.
    Off,
}

/// The values of `gestures.policy`, in the order of [`Policy`].
pub(crate) const POLICIES: [&str; 3] = [
    "core.gestures.standard",
    "core.gestures.own",
    "core.gestures.off",
];

impl Policy {
    pub(crate) fn of(id: Option<&Id>) -> Policy {
        match id.map(Id::as_str) {
            Some(p) if p == POLICIES[1] => Policy::Own,
            Some(p) if p == POLICIES[2] => Policy::Off,
            _ => Policy::Standard,
        }
    }
}

impl InputKey {
    /// Its name as written in key maps, as [`InputKey::parse`] reads it.
    pub(crate) fn name(&self) -> String {
        let mut name = String::new();
        // A `String` never refuses.
        let _ = self.write_name(&mut name);
        name
    }

    /// [`name`](Self::name) written into `out`, which can refuse (a buffer
    /// too small).
    pub(crate) fn write_name(&self, out: &mut dyn core::fmt::Write) -> core::fmt::Result {
        match self {
            InputKey::Button(name) => write!(out, "button.{name}"),
            InputKey::LongButton(name) => write!(out, "long.{name}"),
            InputKey::DoubleButton(name) => write!(out, "double.{name}"),
            InputKey::Tap(zone) => write!(out, "tap.{}", zone.id()),
            InputKey::LongTap(zone) => write!(out, "long-tap.{}", zone.id()),
            InputKey::DoubleTap(zone) => write!(out, "double-tap.{}", zone.id()),
            InputKey::Swipe(direction) => write!(out, "swipe.{}", direction.id()),
            InputKey::Edge(side) => write!(out, "edge.{}", side_id(*side)),
            InputKey::Pinch(out_) => out.write_str(if *out_ { "pinch.out" } else { "pinch.in" }),
            InputKey::Tap2 => out.write_str("tap2"),
            InputKey::Chord(buttons) => write!(out, "chord.{buttons}"),
            InputKey::ZoneTap(false, name) => write!(out, "tap.{name}"),
            InputKey::ZoneTap(true, name) => write!(out, "long-tap.{name}"),
            InputKey::Named(id) => write!(out, "input.{id}"),
            InputKey::Gesture(id) => write!(out, "gesture.{id}"),
            InputKey::Policy => out.write_str("gestures.policy"),
        }
    }

    pub(crate) fn parse(key: &str) -> Option<InputKey> {
        Self::parse_in(key, |name| Cow::Owned(name.to_string()))
    }

    /// [`Self::parse`] of a key kept for the whole run (a built-in key map's):
    /// its button names are borrowed, not copied (#248).
    fn parse_kept(key: &'static str) -> Option<InputKey> {
        Self::parse_in(key, Cow::Borrowed)
    }

    /// `keep` makes a button's name the key keeps.
    fn parse_in<'k>(key: &'k str, keep: impl Fn(&'k str) -> Cow<'static, str>) -> Option<InputKey> {
        if key == "tap2" {
            return Some(InputKey::Tap2);
        }
        let (kind, name) = key.split_once('.')?;
        // A Bluetooth remote's keys have a dot in their names (`hid.right`).
        let button = |name: &'k str| button_name_ok(name).then(|| keep(name));
        match kind {
            "button" => button(name).map(InputKey::Button),
            "long" => button(name).map(InputKey::LongButton),
            "double" => button(name).map(InputKey::DoubleButton),
            "tap" => Zone::from_id(name)
                .map(InputKey::Tap)
                .or_else(|| zone_name(name).map(|name| InputKey::ZoneTap(false, name))),
            "long-tap" => Zone::from_id(name)
                .map(InputKey::LongTap)
                .or_else(|| zone_name(name).map(|name| InputKey::ZoneTap(true, name))),
            "double-tap" => Zone::from_id(name).map(InputKey::DoubleTap),
            "input" => Id::parse(name).ok().map(InputKey::Named),
            "gesture" => Id::parse(name).ok().map(InputKey::Gesture),
            "swipe" => [
                Direction::Left,
                Direction::Right,
                Direction::Up,
                Direction::Down,
            ]
            .into_iter()
            .find(|d| d.id() == name)
            .map(InputKey::Swipe),
            "edge" => [
                Direction::Left,
                Direction::Right,
                Direction::Up,
                Direction::Down,
            ]
            .into_iter()
            .find(|d| side_id(*d) == name)
            .map(InputKey::Edge),
            "gestures" if name == "policy" => Some(InputKey::Policy),
            "pinch" if name == "out" => Some(InputKey::Pinch(true)),
            "pinch" if name == "in" => Some(InputKey::Pinch(false)),
            "chord" => {
                let (a, b) = name.split_once('_')?;
                let (a, b) = (button(a)?, button(b)?);
                let (a, b) = if b < a { (b, a) } else { (a, b) };
                (a != b).then(|| InputKey::Chord(format!("{a}_{b}")))
            }
            _ => None,
        }
    }
}

/// A button's name: lower case letters, digits, `-` and `.` inside (a Bluetooth
/// remote's keys have a dot in their names).
fn button_name_ok(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}

/// Whether `key` is an input a native plugin may bind: what
/// [`InputKey::parse`] reads, except a layer's own zones (the key map sources
/// name those). Allocates nothing, so the install can ask it where every
/// allocation is fallible.
fn native_input_ok(key: &str) -> bool {
    if key == "tap2" {
        return true;
    }
    let Some((kind, name)) = key.split_once('.') else {
        return false;
    };
    let direction = |name: &str, id: fn(Direction) -> &'static str| {
        [
            Direction::Left,
            Direction::Right,
            Direction::Up,
            Direction::Down,
        ]
        .into_iter()
        .any(|d| id(d) == name)
    };
    match kind {
        "button" | "long" | "double" => button_name_ok(name),
        "tap" | "long-tap" | "double-tap" => Zone::from_id(name).is_some(),
        "input" | "gesture" => crate::native_host::declarations::full_id(name).is_ok(),
        "swipe" => direction(name, Direction::id),
        "edge" => direction(name, side_id),
        "gestures" => name == "policy",
        "pinch" => name == "out" || name == "in",
        "chord" => name
            .split_once('_')
            .is_some_and(|(a, b)| a != b && button_name_ok(a) && button_name_ok(b)),
        _ => false,
    }
}

/// Whether a `key.bind` of a native plugin names a layer, an input and an
/// action the key map can hold (see [`Keymap::bind_native`]). Allocates
/// nothing. The escape (held Power, 8 s) has no input name: the host checks the
/// raw hold before the key map, so nothing here can take it.
pub(crate) fn native_binding_ok(layer: &str, input: &str, action: &str) -> bool {
    use crate::native_host::declarations::full_id;
    (layer == GLOBAL_KEYMAP_LAYER || full_id(layer).is_ok())
        && native_input_ok(input)
        && (action.is_empty() || full_id(action).is_ok())
}

/// What came of a native plugin's binding (`Keymap::bind_native`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Native {
    /// Bound, or outranked by a stronger source (no loss either way).
    Bound,
    /// Left out: it would take the last input to the main menu the device has.
    Dropped,
    /// A part that is no layer, input or action; nothing is bound.
    Invalid,
}

/// The inputs a device has: its buttons (all, where it lists none) and a touch
/// screen's inputs. Plugin-raised inputs and a Bluetooth remote's keys are no
/// part of the device, so nothing counts on them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeviceInputs {
    buttons: bool,
    touch: bool,
    keys: &'static [&'static str],
}

impl DeviceInputs {
    pub(crate) fn new(caps: &paperpaw_platform::InputCaps) -> Self {
        Self {
            buttons: caps.buttons,
            touch: caps.touch,
            keys: caps.keys,
        }
    }

    /// Everything a device may have (for tests that do not care).
    #[cfg(test)]
    pub(crate) const ALL: Self = Self {
        buttons: true,
        touch: true,
        keys: &[],
    };

    fn button(&self, name: &str) -> bool {
        self.buttons && (self.keys.is_empty() || self.keys.contains(&name))
    }

    fn has(&self, input: &InputKey) -> bool {
        match input {
            InputKey::Button(name) | InputKey::LongButton(name) | InputKey::DoubleButton(name) => {
                self.button(name)
            }
            InputKey::Chord(buttons) => buttons.split('_').all(|name| self.button(name)),
            InputKey::Tap(_)
            | InputKey::LongTap(_)
            | InputKey::DoubleTap(_)
            | InputKey::ZoneTap(..)
            | InputKey::Swipe(_)
            | InputKey::Edge(_)
            | InputKey::Pinch(_)
            | InputKey::Tap2 => self.touch,
            InputKey::Named(_) | InputKey::Gesture(_) | InputKey::Policy => false,
        }
    }
}

/// A name of a layer's own zone: lower case letters, digits, `-` and `.`.
fn zone_name(name: &str) -> Option<String> {
    let valid = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
    valid.then(|| name.to_string())
}

/// Whether `key` names an input, e.g. `button.menu` or `tap.center`.
pub(crate) fn is_input(key: &str) -> bool {
    InputKey::parse(key).is_some()
}

impl paperpaw_sdk::keys::KeyBindings for Keymap {
    fn bound(&self, layers: &[&str], input: &str) -> Option<Id> {
        self.resolve(layers, &InputKey::parse(input)?).cloned()
    }

    fn bindings(
        &self,
        layer: &str,
    ) -> Vec<(String, Option<Id>, paperpaw_sdk::keys::BindingOrigin)> {
        let Some(bindings) = self.layers.get(layer) else {
            return Vec::new();
        };
        bindings
            .iter()
            .map(|(key, binding)| {
                (
                    key.name(),
                    binding.action.clone(),
                    binding_origin(binding.origin),
                )
            })
            .collect()
    }

    fn each_layer(
        &self,
        visit: &mut dyn FnMut(&str) -> core::ops::ControlFlow<()>,
    ) -> core::ops::ControlFlow<()> {
        for (layer, _) in self.layers.iter() {
            visit(layer)?;
        }
        core::ops::ControlFlow::Continue(())
    }

    /// Nothing is allocated: each name is written on the stack. A name over
    /// 64 bytes cannot be a registry row and is left out, as every walk
    /// leaves out a row over its caps.
    fn each_binding(
        &self,
        layer: &str,
        visit: &mut paperpaw_sdk::keys::BindingVisit<'_>,
    ) -> core::ops::ControlFlow<()> {
        let Some(bindings) = self.layers.get(layer) else {
            return core::ops::ControlFlow::Continue(());
        };
        for (key, binding) in bindings.iter() {
            let mut name = StackName {
                bytes: [0; NAME_CAP],
                len: 0,
            };
            if key.write_name(&mut name).is_err() {
                continue;
            }
            let Ok(name) = core::str::from_utf8(&name.bytes[..name.len]) else {
                continue;
            };
            visit(
                name,
                binding.action.as_ref(),
                binding_origin(binding.origin),
            )?;
        }
        core::ops::ControlFlow::Continue(())
    }
}

/// The longest input name a registry row carries (`reg_binding.input`).
const NAME_CAP: usize = 64;

/// An input's name on the stack: a name over [`NAME_CAP`] bytes is refused.
struct StackName {
    bytes: [u8; NAME_CAP],
    len: usize,
}

impl core::fmt::Write for StackName {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(core::fmt::Error)?;
        let room = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        room.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn binding_origin(origin: Origin) -> paperpaw_sdk::keys::BindingOrigin {
    use paperpaw_sdk::keys::BindingOrigin;
    match origin {
        Origin::Plugin => BindingOrigin::Plugin,
        Origin::Device => BindingOrigin::Device,
        Origin::Preset => BindingOrigin::Preset,
        Origin::User => BindingOrigin::User,
    }
}

/// Where a binding comes from. Later sources override earlier ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Origin {
    Plugin,
    Device,
    /// The preset the user selected.
    Preset,
    User,
}

#[derive(Debug, Clone)]
struct Binding {
    /// `None` means explicitly unbound.
    action: Option<Id>,
    origin: Origin,
}

/// A problem in a key map source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    pub line: u32,
    pub message: Cow<'static, str>,
}

#[derive(Debug, Default)]
pub(crate) struct Keymap {
    /// By layer; a built-in key map's layer names are borrowed (#248).
    layers: VecMap<Cow<'static, str>, VecMap<InputKey, Binding>>,
    /// The layers' own tap zones (`[zones.<layer>]`): the layer, the name
    /// and where it is, in percent of the screen (left, top, width,
    /// height).
    zones: Vec<(String, String, [u8; 4])>,
}

impl Keymap {
    /// Releases unused capacity once all sources are loaded.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.layers.shrink_to_fit();
        for layer in self.layers.values_mut() {
            layer.shrink_to_fit();
        }
        self.zones.shrink_to_fit();
    }

    /// The name of the layers' own zone that holds the point `x`, `y` (in
    /// percent of the screen), the most specific layer first.
    pub(crate) fn zone_at(&self, layers: &[&str], x: u8, y: u8) -> Option<&str> {
        layers.iter().find_map(|layer| {
            self.zones
                .iter()
                .find(|(of, _, [left, top, width, height])| {
                    of == layer
                        && (*left..left.saturating_add(*width)).contains(&x)
                        && (*top..top.saturating_add(*height)).contains(&y)
                })
                .map(|(_, name, _)| name.as_str())
        })
    }

    /// A layer's own zone `name` at `place` (`<left> <top> <width>
    /// <height>` in percent of the screen); whether it is one.
    fn add_zone(&mut self, layer: &str, name: &str, place: &str) -> bool {
        let numbers: Vec<u8> = place
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .filter(|n| *n <= 100)
            .collect();
        let (Some(name), [left, top, width, height]) = (zone_name(name), numbers.as_slice()) else {
            return false;
        };
        self.zones
            .retain(|(of, other, _)| !(of == layer && *other == name));
        self.zones
            .push((layer.to_string(), name, [*left, *top, *width, *height]));
        true
    }

    /// Loads bindings from `source`.
    ///
    /// With `strict`, sections other than `[keymap.*]` are problems;
    /// otherwise (user configuration files) they are skipped.
    pub(crate) fn load(&mut self, source: &str, origin: Origin, strict: bool) -> Vec<Problem> {
        self.load_in(source, origin, strict, |_| None)
    }

    /// [`Self::load`] of a preset's, a plugin's or the device's key map: where
    /// the source is `Cow::Borrowed` (kept for the whole run), its layers,
    /// button names and action ids are borrowed, not copied (#248); an owned
    /// source is copied as by [`Self::load`].
    #[allow(
        clippy::ptr_arg,
        reason = "whether it is borrowed for the whole run is what is asked"
    )]
    pub(crate) fn load_kept(
        &mut self,
        source: &Cow<'static, str>,
        origin: Origin,
        strict: bool,
    ) -> Vec<Problem> {
        match source {
            Cow::Borrowed(source) => self.load_in(source, origin, strict, Some),
            Cow::Owned(source) => self.load(source, origin, strict),
        }
    }

    /// `kept` gives a text of the source as kept for the whole run, where it is.
    fn load_in<'a>(
        &mut self,
        source: &'a str,
        origin: Origin,
        strict: bool,
        kept: impl Fn(&'a str) -> Option<&'static str>,
    ) -> Vec<Problem> {
        let mut problems = Vec::new();
        let problem = |problems: &mut Vec<Problem>, line, message: &'static str| {
            problems.push(Problem {
                line,
                message: Cow::Borrowed(message),
            });
        };
        for entry in paperpaw_config::parse(source) {
            let entry = match entry {
                Ok(entry) => entry,
                // Non-strict sources are whole config files whose syntax
                // errors are reported by their main reader.
                Err(_) if !strict => continue,
                Err(error) => {
                    problems.push(Problem {
                        line: error.line,
                        message: Cow::Owned(error.to_string()),
                    });
                    continue;
                }
            };
            if let Some(layer) = entry.section.strip_prefix(ZONES_PREFIX) {
                let value = entry.value.as_str();
                if !self.add_zone(layer, entry.key, &value) {
                    problem(
                        &mut problems,
                        entry.line,
                        "expected <name> = <left> <top> <width> <height>, in percent",
                    );
                }
                continue;
            }
            let Some(layer) = entry.section.strip_prefix(SECTION_PREFIX) else {
                if strict {
                    problem(
                        &mut problems,
                        entry.line,
                        "expected a [keymap.<layer>] section",
                    );
                }
                continue;
            };
            if layer != GLOBAL_KEYMAP_LAYER && Id::parse(layer).is_err() {
                problem(&mut problems, entry.line, "invalid key map layer");
                continue;
            }
            let input = match kept(entry.key) {
                Some(key) => InputKey::parse_kept(key),
                None => InputKey::parse(entry.key),
            };
            let Some(input) = input else {
                problem(
                    &mut problems,
                    entry.line,
                    "expected button.<name>, long.<name>, double.<name>, tap.<zone>, long-tap.<zone>, double-tap.<zone>, tap2, swipe.<dir>, edge.<side>, pinch.<out|in>, input.<id>, gesture.<id> or gestures.policy",
                );
                continue;
            };
            // A layer's own zone is named before it is bound: else a
            // zone mistyped would bind nothing, unnoticed.
            if let InputKey::ZoneTap(_, name) = &input
                && !self
                    .zones
                    .iter()
                    .any(|(of, zone, _)| of == layer && zone == name)
            {
                problem(
                    &mut problems,
                    entry.line,
                    "unknown zone: name it in [zones.<layer>] first",
                );
                continue;
            }
            let value = entry.value.as_str();
            let kept_value = match &value {
                Cow::Borrowed(value) => kept(value),
                Cow::Owned(_) => None,
            };
            let parsed = match kept_value {
                Some(value) => Id::parse_static(value),
                None => Id::parse(&value),
            };
            let action = if value == UNBOUND {
                None
            } else if let Ok(id) = parsed {
                Some(id)
            } else {
                problem(&mut problems, entry.line, "invalid action id");
                continue;
            };
            let layer = kept(layer).map_or_else(|| Cow::Owned(String::from(layer)), Cow::Borrowed);
            let bindings = self.layers.get_or_insert_with(layer, VecMap::new);
            let overrides = bindings.get(&input).is_none_or(|b| origin >= b.origin);
            if overrides {
                bindings.insert(input, Binding { action, origin });
            }
        }
        problems
    }

    /// One binding a native plugin declared (`key.bind`): the layer `global` or
    /// a screen or slot id, an input as a key map names it, the action (empty:
    /// explicitly unbound). A layer's own zones are the key map sources', so a
    /// native plugin binds only the standard zones. `has` is what the device
    /// offers: the global layer keeps an input to the main menu among them.
    pub(crate) fn bind_native(
        &mut self,
        layer: &str,
        input: &str,
        action: &str,
        has: &DeviceInputs,
    ) -> Native {
        if layer != GLOBAL_KEYMAP_LAYER && Id::parse(layer).is_err() {
            return Native::Invalid;
        }
        let input = match InputKey::parse(input) {
            Some(InputKey::ZoneTap(..)) | None => return Native::Invalid,
            Some(input) => input,
        };
        let action = if action.is_empty() {
            None
        } else if let Ok(id) = Id::parse(action) {
            Some(id)
        } else {
            return Native::Invalid;
        };
        let origin = Origin::Plugin;
        // A binding a stronger source (the preset) already holds stays; it is
        // no loss either.
        let wins = self
            .layers
            .get(layer)
            .and_then(|bindings| bindings.get(&input))
            .is_none_or(|b| origin >= b.origin);
        if !wins {
            return Native::Bound;
        }
        if layer == GLOBAL_KEYMAP_LAYER && self.would_lose_the_menu(&input, action.as_ref(), has) {
            return Native::Dropped;
        }
        self.layers
            .get_or_insert_with(Cow::Owned(String::from(layer)), VecMap::new)
            .insert(input, Binding { action, origin });
        Native::Bound
    }

    /// Whether binding `action` (none: unbound) to `input` in the global layer
    /// would leave that layer without an input to the main menu (`core.menu`)
    /// that the device has (touch zones are none on a device without a touch
    /// screen). Only a binding that replaces the last such input does. The
    /// host-fixed escape is the host's, not the key map's (`registry.md`
    /// section 4.2).
    fn would_lose_the_menu(
        &self,
        input: &InputKey,
        action: Option<&Id>,
        has: &DeviceInputs,
    ) -> bool {
        const MENU: &str = "core.menu";
        if action.is_some_and(|id| id.as_str() == MENU) {
            return false;
        }
        let Some(global) = self.layers.get(GLOBAL_KEYMAP_LAYER) else {
            return false;
        };
        let opens_menu = |binding: &Binding| {
            binding
                .action
                .as_ref()
                .is_some_and(|id| id.as_str() == MENU)
        };
        // An input the device lacks reaches nothing: replacing it loses none.
        has.has(input)
            && global.get(input).is_some_and(opens_menu)
            && !global
                .iter()
                .any(|(key, binding)| key != input && has.has(key) && opens_menu(binding))
    }

    /// Resolves `input` through `layers`, most specific first. An explicit
    /// `none` binding stops the search.
    pub(crate) fn resolve(&self, layers: &[&str], input: &InputKey) -> Option<&Id> {
        layers
            .iter()
            .find_map(|layer| self.layers.get(*layer)?.get(input))
            .and_then(|binding| binding.action.as_ref())
    }

    /// As [`Self::resolve`], with the place in `layers` of the layer that
    /// bound it.
    pub(crate) fn resolve_layer(&self, layers: &[&str], input: &InputKey) -> Option<(usize, &Id)> {
        // An explicit `none` binding stops the search, as in `resolve`.
        let (at, binding) = layers
            .iter()
            .enumerate()
            .find_map(|(at, layer)| Some((at, self.layers.get(*layer)?.get(input)?)))?;
        Some((at, binding.action.as_ref()?))
    }

    /// What a tap (`long`: a long tap) in a layer's own zone `name` is
    /// bound to through `layers`, as [`Self::resolve`], making no key.
    pub(crate) fn resolve_zone(&self, layers: &[&str], long: bool, name: &str) -> Option<&Id> {
        layers
            .iter()
            .find_map(|layer| {
                self.layers
                    .get(*layer)?
                    .iter()
                    .find_map(|(key, binding)| match key {
                        InputKey::ZoneTap(l, n) if *l == long && n == name => Some(binding),
                        _ => None,
                    })
            })
            .and_then(|binding| binding.action.as_ref())
    }

    /// All bound action ids, for validation.
    pub(crate) fn actions(&self) -> impl Iterator<Item = &Id> {
        self.layers
            .values()
            .flat_map(|layer| layer.values())
            .filter_map(|binding| binding.action.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(name: &'static str) -> InputKey {
        InputKey::Button(Cow::Borrowed(name))
    }

    /// What a native plugin may bind is exactly what the parser reads, less a
    /// layer's own zones; the check allocates nothing, the parser does.
    #[test]
    fn the_native_input_check_agrees_with_the_parser() {
        for key in [
            "long.back",
            "button.page-forward",
            "double.menu",
            "button.hid.right",
            "button.",
            "button..x",
            "long.Back",
            "tap.center",
            "long-tap.top-left",
            "double-tap.bottom",
            "tap.mine",
            "long-tap.mine",
            "double-tap.mine",
            "tap2",
            "swipe.up",
            "swipe.sideways",
            "edge.top",
            "edge.up",
            "pinch.out",
            "pinch.in",
            "pinch.diagonal",
            "input.my.chord",
            "input.My",
            "gesture.my.shape",
            "gestures.policy",
            "gestures.other",
            "chord.menu_up",
            "chord.menu_menu",
            "chord.menu",
            "nonsense",
            "factory.nonsense",
            "",
            ".",
        ] {
            let parsed = !matches!(InputKey::parse(key), None | Some(InputKey::ZoneTap(..)));
            assert_eq!(native_input_ok(key), parsed, "{key:?}");
        }
    }

    #[test]
    fn a_native_binding_is_checked_without_the_key_map() {
        assert!(native_binding_ok(
            "keyboard.screen",
            "long.back",
            "keyboard.done"
        ));
        assert!(native_binding_ok("global", "swipe.up", ""));
        // Power and its chords are ordinary inputs (the escape has no name).
        for input in [
            "button.power",
            "double.power",
            "long.power",
            "chord.back_power",
        ] {
            assert!(native_binding_ok("global", input, "a.b"), "{input}");
        }
        assert!(!native_binding_ok("Not A Layer", "long.back", "a.b"));
        assert!(!native_binding_ok("a.b", "long.back", "Not An Action"));
        assert!(!native_binding_ok("a.b", "tap.mine", "a.b"));
        let mut keymap = Keymap::default();
        let all = &DeviceInputs::ALL;
        let bind = |keymap: &mut Keymap, layer, input, action| {
            keymap.bind_native(layer, input, action, all)
        };
        assert_eq!(
            bind(&mut keymap, "keyboard.screen", "long.back", "keyboard.done"),
            Native::Bound
        );
        assert_eq!(
            bind(&mut keymap, "keyboard.screen", "tap.mine", "keyboard.done"),
            Native::Invalid
        );
    }

    fn global(keymap: &Keymap, input: &str) -> Option<String> {
        keymap
            .resolve(&["global"], &InputKey::parse(input).unwrap())
            .map(|id| id.as_str().to_string())
    }

    /// The global layer keeps an input to the main menu: a native binding that
    /// would replace or unbind the last one is dropped, one that leaves another
    /// is bound, and other layers are free.
    #[test]
    fn a_global_binding_never_removes_the_last_input_to_the_menu() {
        let all = &DeviceInputs::ALL;
        let mut keymap = Keymap::default();
        keymap.load(
            "[keymap.global]\nbutton.menu = core.menu\n[keymap.reader]\nbutton.menu = core.menu\n",
            Origin::Plugin,
            true,
        );
        // The only input to the menu: not replaced, not unbound.
        assert_eq!(
            keymap.bind_native("global", "button.menu", "core.home", all),
            Native::Dropped
        );
        assert_eq!(
            keymap.bind_native("global", "button.menu", "", all),
            Native::Dropped
        );
        assert_eq!(global(&keymap, "button.menu").as_deref(), Some("core.menu"));
        // Rebinding it to the menu again, another input, and another layer.
        for (layer, input, action) in [
            ("global", "button.menu", "core.menu"),
            ("global", "button.home", "core.home"),
            ("reader", "button.menu", "core.home"),
            ("reader", "button.menu", ""),
        ] {
            assert_eq!(
                keymap.bind_native(layer, input, action, all),
                Native::Bound,
                "{layer} {input}"
            );
        }
        // With a second input to the menu, the first may go.
        assert_eq!(
            keymap.bind_native("global", "tap.center", "core.menu", all),
            Native::Bound
        );
        assert_eq!(
            keymap.bind_native("global", "button.menu", "core.home", all),
            Native::Bound
        );
        assert_eq!(global(&keymap, "button.menu").as_deref(), Some("core.home"));
        // But not the second, which is the last now.
        assert_eq!(
            keymap.bind_native("global", "tap.center", "", all),
            Native::Dropped
        );
        assert_eq!(global(&keymap, "tap.center").as_deref(), Some("core.menu"));
    }

    /// Only inputs the device has count: on a device without a touch screen the
    /// touch zones that open the menu are no way to it, so the button may not
    /// go; one that lists its buttons counts only those.
    #[test]
    fn only_the_inputs_the_device_has_count_as_a_way_to_the_menu() {
        const BUTTONS: &[&str] = &["back", "confirm", "menu"];
        let device = |touch, keys| DeviceInputs {
            buttons: true,
            touch,
            keys,
        };
        let source = "[keymap.global]\nbutton.menu = core.menu\ntap.center = core.menu\n\
                      button.page = core.menu\n";
        let make = || {
            let mut keymap = Keymap::default();
            keymap.load(source, Origin::Plugin, true);
            keymap
        };
        // Without touch, the zone and the button the device lacks reach nothing.
        let buttons_only = device(false, BUTTONS);
        let mut keymap = make();
        assert_eq!(
            keymap.bind_native("global", "button.menu", "core.home", &buttons_only),
            Native::Dropped
        );
        assert_eq!(global(&keymap, "button.menu").as_deref(), Some("core.menu"));
        // With touch, the zone is a way: the button may go.
        let mut keymap = make();
        assert_eq!(
            keymap.bind_native("global", "button.menu", "core.home", &device(true, BUTTONS)),
            Native::Bound
        );
        // Taking away an input the device lacks loses nothing.
        let mut keymap = make();
        assert_eq!(
            keymap.bind_native("global", "tap.center", "", &buttons_only),
            Native::Bound
        );
        assert_eq!(
            keymap.bind_native("global", "button.page", "core.home", &buttons_only),
            Native::Bound
        );
        // And the last button is still guarded once those are gone.
        assert_eq!(
            keymap.bind_native("global", "button.menu", "", &buttons_only),
            Native::Dropped
        );
        // A device that lists no buttons has them all.
        let mut keymap = make();
        assert_eq!(
            keymap.bind_native("global", "button.menu", "core.home", &device(false, &[])),
            Native::Bound
        );
    }

    /// A layer's own zones are read from `[zones.<layer>]` and found by
    /// where a point is; taps in them are bound as `tap.<name>`.
    #[test]
    fn layers_have_zones_of_their_own() {
        let mut keymap = Keymap::default();
        let problems = keymap.load(
            "[zones.reader]\ncorner = 80 0 20 15\nbad = 1 2\n[keymap.reader]\ntap.corner = core.menu\ndouble.menu = core.home\ntap2 = core.back\ninput.my.chord = core.home\n",
            Origin::Plugin,
            true,
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(keymap.zone_at(&["reader", "global"], 90, 5), Some("corner"));
        assert_eq!(keymap.zone_at(&["reader"], 50, 5), None);
        assert_eq!(keymap.zone_at(&["global"], 90, 5), None);
        let tap = InputKey::ZoneTap(false, String::from("corner"));
        assert_eq!(
            keymap.resolve(&["reader"], &tap).map(Id::as_str),
            Some("core.menu")
        );
        for input in ["double.menu", "tap2", "input.my.chord"] {
            let key = InputKey::parse(input).unwrap();
            assert!(keymap.resolve(&["reader"], &key).is_some(), "{input}");
        }
    }

    /// A built-in key map is kept by reference: its layers, buttons and
    /// actions are not copied (#248); a source made at run time is.
    #[test]
    fn a_built_in_key_map_is_borrowed_and_a_made_one_copied() {
        const SOURCE: &str =
            "[keymap.core.reader]\nbutton.next = core.page-forward\nlong.next = core.menu\n";
        let mut kept = Keymap::default();
        assert!(
            kept.load_kept(&Cow::Borrowed(SOURCE), Origin::Preset, true)
                .is_empty()
        );
        let mut made = Keymap::default();
        let owned = Cow::Owned(String::from(SOURCE));
        assert!(made.load_kept(&owned, Origin::Preset, true).is_empty());
        for (keymap, borrowed) in [(&kept, true), (&made, false)] {
            let (layer, bindings) = keymap.layers.iter().next().unwrap();
            assert_eq!(matches!(layer, Cow::Borrowed(_)), borrowed);
            for (input, binding) in bindings.iter() {
                let (InputKey::Button(name) | InputKey::LongButton(name)) = input else {
                    panic!("{input:?}");
                };
                assert_eq!(matches!(name, Cow::Borrowed(_)), borrowed);
                let action = binding.action.as_ref().unwrap();
                assert_eq!(action.kept().is_some(), borrowed, "{action}");
            }
            let next = InputKey::parse("button.next").unwrap();
            assert_eq!(
                keymap.resolve(&["core.reader"], &next).map(Id::as_str),
                Some("core.page-forward")
            );
        }
        // An invalid id stays a problem either way.
        let mut keymap = Keymap::default();
        let bad = Cow::Borrowed("[keymap.global]\nbutton.next = Not An Id\n");
        assert_eq!(keymap.load_kept(&bad, Origin::Device, true).len(), 1);
    }

    #[test]
    fn zones_split_the_screen_in_thirds() {
        assert_eq!(Zone::of(Point::new(0, 0), 300, 300), Zone::TopLeft);
        assert_eq!(Zone::of(Point::new(150, 150), 300, 300), Zone::Center);
        assert_eq!(Zone::of(Point::new(299, 100), 300, 300), Zone::Right);
        assert_eq!(Zone::of(Point::new(500, 500), 300, 300), Zone::BottomRight);
    }

    #[test]
    fn layers_and_origins() {
        let mut keymap = Keymap::default();
        let problems = keymap.load(
            "[keymap.global]\nbutton.confirm = core.confirm\ntap.center = core.menu\n\
             [keymap.core.reader]\nbutton.confirm = example.bookmark\n",
            Origin::Plugin,
            true,
        );
        assert!(problems.is_empty(), "{problems:?}");

        let global = ["global"];
        let reader = ["core.reader", "global"];
        let confirm = button("confirm");
        assert_eq!(
            keymap.resolve(&global, &confirm).map(Id::as_str),
            Some("core.confirm")
        );
        assert_eq!(
            keymap.resolve(&reader, &confirm).map(Id::as_str),
            Some("example.bookmark")
        );
        assert_eq!(
            keymap
                .resolve(&reader, &InputKey::Tap(Zone::Center))
                .map(Id::as_str),
            Some("core.menu")
        );

        // A plugin cannot override the user, but the user overrides everything.
        keymap.load(
            "[keymap.global]\nbutton.confirm = user.thing\n",
            Origin::User,
            false,
        );
        keymap.load(
            "[keymap.global]\nbutton.confirm = other.thing\n",
            Origin::Plugin,
            true,
        );
        assert_eq!(
            keymap.resolve(&global, &confirm).map(Id::as_str),
            Some("user.thing")
        );
    }

    #[test]
    fn none_unbinds() {
        let mut keymap = Keymap::default();
        keymap.load(
            "[keymap.global]\ntap.center = core.menu\n",
            Origin::Plugin,
            true,
        );
        keymap.load("[keymap.global]\ntap.center = none\n", Origin::User, false);
        assert_eq!(
            keymap.resolve(&["global"], &InputKey::Tap(Zone::Center)),
            None
        );
    }

    #[test]
    fn reports_problems() {
        let mut keymap = Keymap::default();
        let problems = keymap.load(
            "[other]\na = b\n[keymap.global]\nbutton.X = core.menu\ntap.middle = core.menu\n\
             button.ok = Not An Id\n[keymap.Bad Layer]\nbutton.ok = core.menu\n",
            Origin::Plugin,
            true,
        );
        let lines: Vec<u32> = problems.iter().map(|p| p.line).collect();
        // A layer of words is a section, but no layer: its entries are refused.
        assert_eq!(lines, [2, 4, 5, 6, 8]);
    }

    /// The walks of the registry visit what the lists hold, in layer order,
    /// and leave out a name over 64 bytes (it cannot be a registry row).
    #[test]
    fn the_walks_match_the_lists_and_leave_out_a_long_name() {
        use core::ops::ControlFlow;
        use paperpaw_sdk::keys::KeyBindings;
        let long = "x".repeat(70);
        let mut map = Keymap::default();
        let source = format!(
            "[keymap.b]\nbutton.menu = demo.one\n[keymap.a]\nswipe.left = none\nbutton.{long} = demo.two\n"
        );
        let problems = map.load(&source, Origin::User, false);
        assert!(problems.is_empty(), "{problems:?}");
        let mut layers = Vec::new();
        let end = map.each_layer(&mut |layer| {
            layers.push(layer.to_string());
            ControlFlow::Continue(())
        });
        assert_eq!(
            (layers.as_slice(), end),
            (&["a".into(), "b".into()][..], ControlFlow::Continue(()))
        );
        let mut walked = Vec::new();
        let end = map.each_binding("a", &mut |input, action, _| {
            walked.push((input.to_string(), action.map(|id| id.as_str().to_string())));
            ControlFlow::Continue(())
        });
        assert!(end.is_continue());
        // The list has both; the walk has the one that can be a row.
        assert_eq!(map.bindings("a").len(), 2);
        assert_eq!(walked, [("swipe.left".to_string(), None)]);
        // `Break` ends the walk and is the result.
        let mut seen = 0;
        let end = map.each_layer(&mut |_| {
            seen += 1;
            ControlFlow::Break(())
        });
        assert_eq!((seen, end), (1, ControlFlow::Break(())));
        assert!(
            map.each_binding("none", &mut |_, _, _| ControlFlow::Break(()))
                .is_continue()
        );
    }

    /// Every input's name reads back as the same input.
    #[test]
    fn names_read_back() {
        for name in [
            "button.menu",
            "long.confirm",
            "double.back",
            "tap.center",
            "long-tap.top-left",
            "double-tap.bottom",
            "swipe.left",
            "edge.top",
            "pinch.out",
            "pinch.in",
            "tap2",
            "tap.corner",
            "long-tap.corner",
            "input.my.chord",
            "gesture.my.flick",
            "gestures.policy",
            "chord.back_power",
            "chord.hid.left_hid.right",
        ] {
            assert_eq!(InputKey::parse(name).unwrap().name(), name);
        }
    }

    /// Chords name their buttons in order, two different ones (reserved:
    /// read and kept, raised by none yet).
    #[test]
    fn chords_are_read_in_order() {
        let chord = InputKey::parse("chord.power_back").unwrap();
        assert_eq!(chord.name(), "chord.back_power");
        for bad in [
            "chord.power",
            "chord.power_power",
            "chord._back",
            "chord.a_b_c",
            "chord.A_b",
        ] {
            assert_eq!(InputKey::parse(bad), None, "{bad}");
        }
        let mut keymap = Keymap::default();
        let problems = keymap.load(
            "[keymap.global]\nchord.power_back = core.home\n",
            Origin::Plugin,
            true,
        );
        assert!(problems.is_empty(), "{problems:?}");
    }

    /// No later question about the bindings is known yet: each answers
    /// `None`.
    #[test]
    fn later_questions_get_no_answer() {
        use paperpaw_sdk::keys::KeyBindings;
        let keymap = Keymap::default();
        let key = paperpaw_sdk::Id::new_static("core.chords");
        assert_eq!(keymap.extended(&key, &paperpaw_sdk::Params::new()), None);
    }
}
