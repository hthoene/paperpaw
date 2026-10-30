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
        match self {
            InputKey::Button(name) => format!("button.{name}"),
            InputKey::LongButton(name) => format!("long.{name}"),
            InputKey::DoubleButton(name) => format!("double.{name}"),
            InputKey::Tap(zone) => format!("tap.{}", zone.id()),
            InputKey::LongTap(zone) => format!("long-tap.{}", zone.id()),
            InputKey::DoubleTap(zone) => format!("double-tap.{}", zone.id()),
            InputKey::Swipe(direction) => format!("swipe.{}", direction.id()),
            InputKey::Edge(side) => format!("edge.{}", side_id(*side)),
            InputKey::Pinch(out) => String::from(if *out { "pinch.out" } else { "pinch.in" }),
            InputKey::Tap2 => String::from("tap2"),
            InputKey::ZoneTap(false, name) => format!("tap.{name}"),
            InputKey::ZoneTap(true, name) => format!("long-tap.{name}"),
            InputKey::Named(id) => format!("input.{id}"),
            InputKey::Gesture(id) => format!("gesture.{id}"),
            InputKey::Policy => String::from("gestures.policy"),
        }
    }

    pub(crate) fn parse(key: &str) -> Option<InputKey> {
        if key == "tap2" {
            return Some(InputKey::Tap2);
        }
        let (kind, name) = key.split_once('.')?;
        // A Bluetooth remote's keys have a dot in their names (`hid.right`).
        let button = |name: &str| {
            let valid = !name.is_empty()
                && !name.starts_with('.')
                && !name.ends_with('.')
                && name.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'
                });
            valid.then(|| Cow::Owned(name.to_string()))
        };
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
            _ => None,
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
        use paperpaw_sdk::keys::BindingOrigin;
        let Some(bindings) = self.layers.get(layer) else {
            return Vec::new();
        };
        bindings
            .iter()
            .map(|(key, binding)| {
                let origin = match binding.origin {
                    Origin::Plugin => BindingOrigin::Plugin,
                    Origin::Device => BindingOrigin::Device,
                    Origin::Preset => BindingOrigin::Preset,
                    Origin::User => BindingOrigin::User,
                };
                (key.name(), binding.action.clone(), origin)
            })
            .collect()
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
    layers: VecMap<String, VecMap<InputKey, Binding>>,
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
            let Some(input) = InputKey::parse(entry.key) else {
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
            let action = if value == UNBOUND {
                None
            } else if let Ok(id) = Id::parse(&value) {
                Some(id)
            } else {
                problem(&mut problems, entry.line, "invalid action id");
                continue;
            };
            let bindings = self
                .layers
                .get_or_insert_with(String::from(layer), VecMap::new);
            let overrides = bindings.get(&input).is_none_or(|b| origin >= b.origin);
            if overrides {
                bindings.insert(input, Binding { action, origin });
            }
        }
        problems
    }

    /// Resolves `input` through `layers`, most specific first. An explicit
    /// `none` binding stops the search.
    pub(crate) fn resolve(&self, layers: &[&str], input: &InputKey) -> Option<&Id> {
        layers
            .iter()
            .find_map(|layer| self.layers.get(*layer)?.get(input))
            .and_then(|binding| binding.action.as_ref())
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
        ] {
            assert_eq!(InputKey::parse(name).unwrap().name(), name);
        }
    }
}
