//! What the screens of Buttons and gestures decide, without the host: places,
//! the inputs a device has, what a change may take away.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    keys::{EDGES, PINCHES, ZONES},
    native::Error,
};
use paperpaw_ui_kit::{
    mem::{self, push},
    text::{concat, copy, decimal},
};

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
    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            title: copy(&self.title)?,
            layers: mem::copy_strings(&self.layers)?,
        })
    }

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
///
/// # Errors
/// [`Error::NoMemory`], or what `kind` refuses with; titles named before the
/// refusal stay changed.
pub fn tell_apart(
    places: &mut [Place],
    kind: &mut dyn FnMut(&str) -> Result<String, Error>,
) -> Result<(), Error> {
    let mut shared = Vec::new();
    shared
        .try_reserve_exact(places.len())
        .map_err(|_| Error::NoMemory)?;
    for i in 0..places.len() {
        let title = &places[i].title;
        shared.push(
            (i > 0 && places[i - 1].title == *title)
                || places.get(i + 1).is_some_and(|next| next.title == *title),
        );
    }
    for (place, shared) in places.iter_mut().zip(shared) {
        if shared && !place.layer().ends_with(".screen") {
            let kind = kind(place.layer())?;
            place.title = concat(&[&place.title, " (", &kind, ")"])?;
        }
    }
    let mut count = 1usize;
    let mut previous = String::new();
    for place in places.iter_mut() {
        if place.title == previous {
            count = count.saturating_add(1);
            let mut digits = [0; 20];
            place.title = concat(&[&place.title, " ", decimal(count, &mut digits)])?;
        } else {
            count = 1;
            previous = copy(&place.title)?;
        }
    }
    Ok(())
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
///
/// # Errors
/// [`Error::NoMemory`].
pub fn buttons(inputs: &[String]) -> Result<Vec<&'static str>, Error> {
    let mut found = Vec::new();
    for button in BUTTONS {
        if inputs
            .iter()
            .any(|i| i.strip_prefix("button.") == Some(button))
        {
            push(&mut found, button)?;
        }
    }
    Ok(found)
}

/// `prefix.name` for each of `names` the device has.
fn each(inputs: &[String], prefix: &str, names: &[&str]) -> Result<Vec<String>, Error> {
    let mut found = Vec::new();
    for name in names {
        let input = concat(&[prefix, ".", name])?;
        if inputs.iter().any(|i| *i == input) {
            push(&mut found, input)?;
        }
    }
    Ok(found)
}

/// `prefix.button` for each button the device has.
fn named(have: &[&str], prefix: &str) -> Result<Vec<String>, Error> {
    let mut found = Vec::new();
    for button in have {
        push(&mut found, concat(&[prefix, ".", button])?)?;
    }
    Ok(found)
}

/// The inputs of the device in sections, gestures first on touch screens, then
/// buttons, then those plugins bring.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn sections(inputs: &[String]) -> Result<Vec<Section>, Error> {
    let has = |name: &str| inputs.iter().any(|i| i == name);
    let touch = has_touch(inputs);
    let mut gestures: Vec<(&'static str, Vec<String>)> = Vec::new();
    if touch {
        let swipes = ["left", "right", "up", "down"];
        let mut fingers: Vec<String> = Vec::new();
        if has("tap2") {
            push(&mut fingers, copy("tap2")?)?;
        }
        mem::extend(&mut fingers, each(inputs, "pinch", &PINCHES)?)?;
        for entry in [
            ("keys.section.edges", each(inputs, "edge", &EDGES)?),
            ("keys.section.swipes", each(inputs, "swipe", &swipes)?),
            ("keys.section.taps", each(inputs, "tap", &ZONES)?),
            ("keys.section.held-taps", each(inputs, "long-tap", &ZONES)?),
            (
                "keys.section.double-taps",
                each(inputs, "double-tap", &ZONES)?,
            ),
            ("keys.section.fingers", fingers),
        ] {
            push(&mut gestures, entry)?;
        }
    }
    let have = buttons(inputs)?;
    let mut keys: Vec<(&'static str, Vec<String>)> = Vec::new();
    for entry in [
        ("keys.section.buttons", named(&have, "button")?),
        ("keys.section.held-buttons", named(&have, "long")?),
        ("keys.section.double-buttons", named(&have, "double")?),
    ] {
        push(&mut keys, entry)?;
    }
    let mut own: Vec<String> = Vec::new();
    for input in inputs
        .iter()
        .filter(|i| i.starts_with("input.") || i.starts_with("gesture."))
    {
        push(&mut own, copy(input)?)?;
    }
    let (first, then) = if touch {
        (gestures, keys)
    } else {
        (keys, gestures)
    };
    let mut sections = Vec::new();
    for (heading, inputs) in first
        .into_iter()
        .chain(then)
        .chain([("keys.section.own", own)])
    {
        if !inputs.is_empty() {
            push(&mut sections, Section { heading, inputs })?;
        }
    }
    Ok(sections)
}

/// The inputs of buttons this device has: short, held and pressed twice.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn button_inputs(inputs: &[String]) -> Result<Vec<String>, Error> {
    let have = buttons(inputs)?;
    let mut found = Vec::new();
    for kind in ["button", "long", "double"] {
        mem::extend(&mut found, named(&have, kind)?)?;
    }
    Ok(found)
}

/// Why binding `input` to `action` in `layers` is refused, if it is: on a
/// device with buttons only, when something the buttons reach there now would
/// have no button left. *As standard* (`None`) always may.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn refused(
    inputs: &[String],
    bindings: &[Bound],
    layers: &[String],
    input: &str,
    action: Option<&str>,
) -> Result<Option<&'static str>, Error> {
    let Some(action) = action else {
        return Ok(None);
    };
    if has_touch(inputs) || buttons(inputs)?.is_empty() {
        return Ok(None);
    }
    let buttons = button_inputs(inputs)?;
    let now = |i: &str| bound(bindings, layers, i);
    let then = |i: &str| if i == input { Some(action) } else { now(i) };
    for (need, message) in &NEEDED {
        let reach_now = buttons
            .iter()
            .any(|i| now(i).is_some_and(|a| need.contains(&a)));
        let reach_then = buttons
            .iter()
            .any(|i| then(i).is_some_and(|a| need.contains(&a)));
        if reach_now && !reach_then {
            return Ok(Some(*message));
        }
    }
    Ok(None)
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
        let sections = sections(&x4()).unwrap();
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
        let sections = super::sections(&touch).unwrap();
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
            )
            .unwrap(),
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
            )
            .unwrap(),
            Some("keys.needed.confirm")
        );
        // Back to standard always may.
        assert_eq!(
            refused(&inputs, &bindings, &layers, "button.back", None).unwrap(),
            None
        );
        assert_eq!(
            refused(
                &inputs,
                &bindings,
                &layers,
                "button.back",
                Some("core.menu")
            )
            .unwrap(),
            Some("keys.needed.back")
        );
        // A touch screen has other ways.
        let mut touch = x4();
        touch.push(String::from("tap.center"));
        assert_eq!(
            refused(&touch, &bindings, &layers, "button.back", Some("core.menu")).unwrap(),
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
        let mut kind = |layer: &str| {
            Ok(match layer.rsplit('.').next().unwrap() {
                "choice" => String::from("choice"),
                other => other.replace('-', " "),
            })
        };
        tell_apart(&mut places, &mut kind).unwrap();
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

    #[test]
    fn the_model_survives_every_refused_allocation() {
        let mut touch = x4();
        touch.extend(strings(&[
            "tap.center",
            "tap.top",
            "swipe.left",
            "tap2",
            "pinch.in",
            "input.demo.go",
        ]));
        crate::fail_alloc::sweep(|| sections(&touch).map(|s| s.len()));
        let inputs = x4();
        crate::fail_alloc::sweep(|| button_inputs(&inputs).map(|s| s.len()));
        crate::fail_alloc::sweep(|| buttons(&inputs).map(|s| s.len()));
        let layers = strings(&[GLOBAL]);
        let bindings = [
            bind(GLOBAL, "button.confirm", "core.confirm"),
            bind(GLOBAL, "button.back", "core.back"),
            bind(GLOBAL, "button.page-forward", "core.next-page"),
            bind(GLOBAL, "button.page-back", "core.prev-page"),
        ];
        crate::fail_alloc::sweep(|| {
            refused(
                &inputs,
                &bindings,
                &layers,
                "button.confirm",
                Some("core.menu"),
            )
        });
        let template = [
            Place {
                title: "Notes".into(),
                layers: strings(&["notes.choice"]),
            },
            Place {
                title: "Notes".into(),
                layers: strings(&["notes.list"]),
            },
            Place {
                title: "Notes".into(),
                layers: strings(&["notes.screen"]),
            },
        ];
        crate::fail_alloc::sweep(|| {
            let mut places = [
                template[0].try_clone()?,
                template[1].try_clone()?,
                template[2].try_clone()?,
            ];
            let mut kind = |layer: &str| copy(layer.rsplit('.').next().unwrap_or(layer));
            tell_apart(&mut places, &mut kind)?;
            places[0].try_clone().map(|p| p.title.len())
        });
    }
}
