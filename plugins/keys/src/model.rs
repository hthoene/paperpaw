//! What the screens of Buttons and gestures decide, without the host: places,
//! the inputs a device has, what a change may take away.
use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::keys::{EDGES, PINCHES, ZONES};

/// The layer of bindings that apply everywhere.
pub const GLOBAL: &str = "global";

/// How a place takes gestures, as written in key maps.
pub const POLICY: &str = "gestures.policy";

/// The buttons a device may have.
pub const BUTTONS: [&str; 10] = [
    "page-forward",
    "page-back",
    "confirm",
    "back",
    "menu",
    "home",
    "left",
    "right",
    "up",
    "down",
];

/// What buttons alone cannot do without, each with the message that a change
/// taking its last button away is refused with.
pub const NEEDED: [(&[&str], &str); 4] = [
    (&["core.confirm"], "keys.needed.confirm"),
    (&["core.back"], "keys.needed.back"),
    (
        &["core.down", "core.next", "core.right", "core.next-page"],
        "keys.needed.forward",
    ),
    (
        &["core.up", "core.previous", "core.left", "core.prev-page"],
        "keys.needed.backward",
    ),
];

/// Where a change applies: the key map layers looked up there, the first one
/// being where changes go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// Translated name, or a screen's name.
    pub title: String,
    /// The layers, most specific first.
    pub layers: Vec<String>,
}

impl Place {
    /// The layer changes go to.
    #[must_use]
    pub fn layer(&self) -> &str {
        self.layers.first().map_or(GLOBAL, String::as_str)
    }

    /// Whether the place is everywhere.
    #[must_use]
    pub fn everywhere(&self) -> bool {
        self.layer() == GLOBAL
    }
}

/// One binding as the registry lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    /// Layer it is in.
    pub layer: String,
    /// Input it is for.
    pub input: String,
    /// The action, empty for nothing.
    pub action: String,
}

/// What `input` is bound to, looking in `layers` in turn: the action, which is
/// empty for nothing; `None` where no layer binds the input.
#[must_use]
pub fn bound<'a>(bindings: &'a [Bound], layers: &[String], input: &str) -> Option<&'a str> {
    layers.iter().find_map(|layer| {
        bindings
            .iter()
            .find(|b| b.layer == *layer && b.input == input)
            .map(|b| b.action.as_str())
    })
}

/// Places of the same name, side by side, told apart by the kind of screen each
/// is (`kind` of its layer), e.g. a tool's list and its choice, both named
/// after the tool; a tool's main screen (`….screen`) keeps the name alone.
/// Those still alike then are numbered. Places must be sorted by name.
pub fn tell_apart(places: &mut [Place], kind: &dyn Fn(&str) -> String) {
    let shared: Vec<bool> = (0..places.len())
        .map(|i| {
            let title = &places[i].title;
            (i > 0 && places[i - 1].title == *title)
                || places.get(i + 1).is_some_and(|next| next.title == *title)
        })
        .collect();
    for (place, shared) in places.iter_mut().zip(shared) {
        if shared && !place.layer().ends_with(".screen") {
            place.title = format!("{} ({})", place.title, kind(place.layer()));
        }
    }
    let mut count = 1;
    let mut previous = String::new();
    for place in places.iter_mut() {
        if place.title == previous {
            count += 1;
            place.title = format!("{} {count}", place.title);
        } else {
            count = 1;
            previous.clone_from(&place.title);
        }
    }
}

/// A group of inputs under a heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Translation key of the heading.
    pub heading: &'static str,
    /// The inputs, in the order key maps list them.
    pub inputs: Vec<String>,
}

/// Whether the device has touch: it makes taps.
#[must_use]
pub fn has_touch(inputs: &[String]) -> bool {
    inputs.iter().any(|i| i.starts_with("tap."))
}

/// The buttons of the device in a fixed order, from its `button.*` inputs.
#[must_use]
pub fn buttons(inputs: &[String]) -> Vec<&'static str> {
    BUTTONS
        .iter()
        .copied()
        .filter(|b| inputs.iter().any(|i| i.strip_prefix("button.") == Some(*b)))
        .collect()
}

/// The inputs of the device in sections, gestures first on touch screens, then
/// buttons, then those plugins bring.
#[must_use]
pub fn sections(inputs: &[String]) -> Vec<Section> {
    let has = |name: &str| inputs.iter().any(|i| i == name);
    let each = |prefix: &str, names: &[&str]| -> Vec<String> {
        names
            .iter()
            .map(|n| format!("{prefix}.{n}"))
            .filter(|i| has(i))
            .collect()
    };
    let touch = has_touch(inputs);
    let mut gestures = Vec::new();
    if touch {
        let swipes = ["left", "right", "up", "down"];
        let mut fingers: Vec<String> = Vec::new();
        if has("tap2") {
            fingers.push(String::from("tap2"));
        }
        fingers.extend(each("pinch", &PINCHES));
        gestures.extend([
            ("keys.section.edges", each("edge", &EDGES)),
            ("keys.section.swipes", each("swipe", &swipes)),
            ("keys.section.taps", each("tap", &ZONES)),
            ("keys.section.held-taps", each("long-tap", &ZONES)),
            ("keys.section.double-taps", each("double-tap", &ZONES)),
            ("keys.section.fingers", fingers),
        ]);
    }
    let have = buttons(inputs);
    let named =
        |prefix: &str| -> Vec<String> { have.iter().map(|b| format!("{prefix}.{b}")).collect() };
    let buttons = [
        ("keys.section.buttons", named("button")),
        ("keys.section.held-buttons", named("long")),
        ("keys.section.double-buttons", named("double")),
    ];
    let (first, then): (Vec<_>, Vec<_>) = if touch {
        (gestures, buttons.into())
    } else {
        (buttons.into(), gestures)
    };
    let own: Vec<String> = inputs
        .iter()
        .filter(|i| i.starts_with("input.") || i.starts_with("gesture."))
        .cloned()
        .collect();
    first
        .into_iter()
        .chain(then)
        .chain([("keys.section.own", own)])
        .filter(|(_, inputs)| !inputs.is_empty())
        .map(|(heading, inputs)| Section { heading, inputs })
        .collect()
}

/// The inputs of buttons this device has: short, held and pressed twice.
#[must_use]
pub fn button_inputs(inputs: &[String]) -> Vec<String> {
    let have = buttons(inputs);
    ["button", "long", "double"]
        .iter()
        .flat_map(|kind| have.iter().map(move |b| format!("{kind}.{b}")))
        .collect()
}

/// Why binding `input` to `action` in `layers` is refused, if it is: on a
/// device with buttons only, when something the buttons reach there now would
/// have no button left. *As standard* (`None`) always may.
#[must_use]
pub fn refused(
    inputs: &[String],
    bindings: &[Bound],
    layers: &[String],
    input: &str,
    action: Option<&str>,
) -> Option<&'static str> {
    let action = action?;
    if has_touch(inputs) || buttons(inputs).is_empty() {
        return None;
    }
    let buttons = button_inputs(inputs);
    let reaches = |need: &[&str], bound_to: &dyn Fn(&str) -> Option<String>| {
        buttons
            .iter()
            .any(|i| bound_to(i).is_some_and(|a| need.contains(&a.as_str())))
    };
    let now = |i: &str| bound(bindings, layers, i).map(String::from);
    let then = |i: &str| {
        if i == input {
            Some(String::from(action))
        } else {
            now(i)
        }
    };
    NEEDED
        .iter()
        .find(|(need, _)| reaches(need, &now) && !reaches(need, &then))
        .map(|(_, message)| *message)
}

/// The index of a tap zone in the 3x3 grid, for its picture.
#[must_use]
pub fn zone_index(input: &str) -> Option<u8> {
    let zone = input
        .strip_prefix("tap.")
        .or_else(|| input.strip_prefix("long-tap."))
        .or_else(|| input.strip_prefix("double-tap."))?;
    ZONES.iter().position(|z| *z == zone).map(|i| i as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| String::from(*s)).collect()
    }

    fn bind(layer: &str, input: &str, action: &str) -> Bound {
        Bound {
            layer: layer.into(),
            input: input.into(),
            action: action.into(),
        }
    }

    fn x4() -> Vec<String> {
        strings(&[
            "button.page-forward",
            "button.page-back",
            "button.confirm",
            "button.back",
            "long.confirm",
        ])
    }

    #[test]
    fn the_first_layer_that_binds_an_input_wins() {
        let bindings = [
            bind(GLOBAL, "button.menu", "core.menu"),
            bind("core.reader", "button.menu", "reader.toc"),
            bind(GLOBAL, "button.home", ""),
        ];
        let layers = strings(&["core.reader", GLOBAL]);
        assert_eq!(bound(&bindings, &layers, "button.menu"), Some("reader.toc"));
        assert_eq!(bound(&bindings, &layers, "button.home"), Some(""));
        assert_eq!(bound(&bindings, &layers, "button.back"), None);
        assert_eq!(
            bound(&bindings, &strings(&[GLOBAL]), "button.menu"),
            Some("core.menu")
        );
    }

    #[test]
    fn sections_follow_the_device_and_skip_what_it_lacks() {
        let sections = sections(&x4());
        let headings: Vec<_> = sections.iter().map(|s| s.heading).collect();
        assert_eq!(
            headings,
            [
                "keys.section.buttons",
                "keys.section.held-buttons",
                "keys.section.double-buttons"
            ]
        );
        assert_eq!(
            sections[0].inputs,
            [
                "button.page-forward",
                "button.page-back",
                "button.confirm",
                "button.back"
            ]
        );
        let mut touch = x4();
        touch.extend(strings(&[
            "tap.center",
            "tap.top",
            "swipe.left",
            "tap2",
            "pinch.in",
            "input.demo.go",
        ]));
        let sections = super::sections(&touch);
        let headings: Vec<_> = sections.iter().map(|s| s.heading).collect();
        assert_eq!(
            headings,
            [
                "keys.section.swipes",
                "keys.section.taps",
                "keys.section.fingers",
                "keys.section.buttons",
                "keys.section.held-buttons",
                "keys.section.double-buttons",
                "keys.section.own"
            ]
        );
        // Zones in grid order, not in the order the host lists them.
        assert_eq!(sections[1].inputs, ["tap.top", "tap.center"]);
        assert_eq!(sections[2].inputs, ["tap2", "pinch.in"]);
    }

    #[test]
    fn buttons_only_devices_keep_a_way_to_choose_go_back_and_move() {
        let inputs = x4();
        let layers = strings(&[GLOBAL]);
        let bindings = [
            bind(GLOBAL, "button.confirm", "core.confirm"),
            bind(GLOBAL, "long.confirm", "core.confirm"),
            bind(GLOBAL, "button.back", "core.back"),
            bind(GLOBAL, "button.page-forward", "core.next-page"),
            bind(GLOBAL, "button.page-back", "core.prev-page"),
        ];
        // One of two buttons for choosing may go.
        assert_eq!(
            refused(
                &inputs,
                &bindings,
                &layers,
                "long.confirm",
                Some("core.menu")
            ),
            None
        );
        // The last for choosing may not.
        let one_left = [
            bind(GLOBAL, "button.confirm", "core.confirm"),
            bind(GLOBAL, "button.back", "core.back"),
            bind(GLOBAL, "button.page-forward", "core.next-page"),
            bind(GLOBAL, "button.page-back", "core.prev-page"),
        ];
        assert_eq!(
            refused(
                &inputs,
                &one_left,
                &layers,
                "button.confirm",
                Some("core.menu")
            ),
            Some("keys.needed.confirm")
        );
        // Back to standard always may.
        assert_eq!(
            refused(&inputs, &bindings, &layers, "button.back", None),
            None
        );
        assert_eq!(
            refused(
                &inputs,
                &bindings,
                &layers,
                "button.back",
                Some("core.menu")
            ),
            Some("keys.needed.back")
        );
        // A touch screen has other ways.
        let mut touch = x4();
        touch.push(String::from("tap.center"));
        assert_eq!(
            refused(&touch, &bindings, &layers, "button.back", Some("core.menu")),
            None
        );
    }

    #[test]
    fn places_of_the_same_name_say_what_kind_of_screen_each_is() {
        let place = |title: &str, layer: &str| Place {
            title: title.into(),
            layers: strings(&[layer]),
        };
        let mut places = [
            place("Add a widget", "home.add-to-list"),
            place("Add a widget", "home.add-widget"),
            place("Battery", "battery.screen"),
            place("Notes", "notes.choice"),
            place("Notes", "notes.other.choice"),
            place("Notes", "notes.list"),
            place("Notes", "notes.screen"),
        ];
        let kind = |layer: &str| match layer.rsplit('.').next().unwrap() {
            "choice" => String::from("choice"),
            other => other.replace('-', " "),
        };
        tell_apart(&mut places, &kind);
        let titles: Vec<&str> = places.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Add a widget (add to list)",
                "Add a widget (add widget)",
                "Battery",
                "Notes (choice)",
                "Notes (choice) 2",
                "Notes (list)",
                "Notes"
            ]
        );
    }

    #[test]
    fn tap_zones_have_a_picture_and_other_inputs_do_not() {
        assert_eq!(zone_index("tap.top-left"), Some(0));
        assert_eq!(zone_index("long-tap.center"), Some(4));
        assert_eq!(zone_index("double-tap.bottom-right"), Some(8));
        assert_eq!(zone_index("tap.nowhere"), None);
        assert_eq!(zone_index("button.menu"), None);
    }
}
