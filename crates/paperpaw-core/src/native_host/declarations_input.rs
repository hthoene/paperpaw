//! Allocation-free syntax checks of the input and help declarations
//! (`docs/abi/ui.md` sections 2 and 7): `input.spec` and `help.hint`.
//! Uniqueness (a plugin's input names, its hints) is the declaration index's.
use super::{
    declarations::{full_id, need_value_check, relative},
    registrations::{HelpHint, InputSpec, Record},
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

/// `input.spec.kind`: an input of the plugin's own, a gesture its recognizer makes.
const KIND_GESTURE: u32 = 1;
/// `input.spec.flags` bit 0: `need` and `capability` mean something.
const HAS_NEED: u32 = 1;
/// `help.hint.input`: any, touch, buttons.
const INPUT_BUTTONS: u32 = 2;
/// A hint's zone is in thousandths of the screen.
const WHOLE: u32 = 1_000;

pub(crate) fn validate(plugin: &str, record: &Record<'_>) -> Result<(), i32> {
    match record {
        Record::InputSpec(spec) => input(plugin, spec),
        Record::HelpHint(hint) => help(hint),
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// An input or gesture of the plugin's own (`ui.md` section 2): a relative
/// name, a title (a catalog key), a kind, and a need only with its flag.
fn input(plugin: &str, spec: &InputSpec<'_>) -> Result<(), i32> {
    // Every field named: a field added to the record must be checked here.
    let InputSpec {
        name,
        title,
        kind,
        need,
        capability,
        flags,
    } = spec;
    relative(plugin, name)?;
    if title.is_empty() || *kind > KIND_GESTURE || flags & !HAS_NEED != 0 {
        return Err(ERR_INVALID);
    }
    if flags & HAS_NEED == 0 {
        if *need != 0 || *capability != 0 {
            return Err(ERR_INVALID);
        }
        Ok(())
    } else {
        need_value_check(*need, *capability)
    }
}

/// A hint for a screen, any plugin's (`ui.md` section 7): its full id, which
/// inputs it is for, a zone that lies on the screen, the catalog key of its text.
fn help(hint: &HelpHint<'_>) -> Result<(), i32> {
    let HelpHint {
        screen,
        input,
        x,
        y,
        width,
        height,
        text,
    } = hint;
    full_id(screen)?;
    if !screen.contains('.') || text.is_empty() || *input > INPUT_BUTTONS {
        return Err(ERR_INVALID);
    }
    // All 0 is no zone. A sum that overflows is over the screen, never clamped.
    let on_screen = |start: u32, size: u32| start.checked_add(size).is_some_and(|end| end <= WHOLE);
    if !on_screen(*x, *width) || !on_screen(*y, *height) {
        return Err(ERR_INVALID);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> InputSpec<'static> {
        InputSpec {
            name: "chord",
            title: "demo.input.chord",
            kind: 0,
            need: 0,
            capability: 0,
            flags: 0,
        }
    }

    fn hint() -> HelpHint<'static> {
        HelpHint {
            screen: "reader.page",
            input: 0,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            text: "demo.help.page",
        }
    }

    #[test]
    fn an_input_and_a_gesture_are_admitted() {
        assert_eq!(input("demo", &spec()), Ok(()));
        assert_eq!(input("demo", &InputSpec { kind: 1, ..spec() }), Ok(()));
        // A need with its flag: the device has the bits.
        let needing = InputSpec {
            flags: HAS_NEED,
            need: 1,
            capability: 0,
            ..spec()
        };
        assert_eq!(input("demo", &needing), need_value_check(1, 0));
    }

    #[test]
    fn a_malformed_input_is_invalid() {
        for bad in [
            InputSpec { name: "", ..spec() },
            InputSpec {
                name: "Not A Name",
                ..spec()
            },
            InputSpec {
                title: "",
                ..spec()
            },
            InputSpec { kind: 2, ..spec() },
            InputSpec {
                flags: 1 << 1,
                ..spec()
            },
            // A need without its flag.
            InputSpec { need: 1, ..spec() },
            InputSpec {
                capability: 1,
                ..spec()
            },
        ] {
            assert_eq!(input("demo", &bad), Err(ERR_INVALID), "{bad:?}");
        }
    }

    #[test]
    fn a_hint_with_and_without_a_zone_is_admitted() {
        assert_eq!(help(&hint()), Ok(()));
        for input in [0, 1, 2] {
            assert_eq!(help(&HelpHint { input, ..hint() }), Ok(()));
        }
        let zone = HelpHint {
            input: 1,
            x: 400,
            y: 900,
            width: 600,
            height: 100,
            ..hint()
        };
        assert_eq!(help(&zone), Ok(()));
    }

    #[test]
    fn a_hint_that_breaks_a_rule_is_invalid() {
        for bad in [
            HelpHint {
                screen: "page",
                ..hint()
            },
            HelpHint {
                screen: "Not A Screen",
                ..hint()
            },
            HelpHint { text: "", ..hint() },
            HelpHint { input: 3, ..hint() },
            // Over the screen, and a sum that overflows.
            HelpHint {
                x: 500,
                width: 501,
                ..hint()
            },
            HelpHint {
                y: 1,
                height: 1_000,
                ..hint()
            },
            HelpHint {
                x: u32::MAX,
                width: 2,
                ..hint()
            },
            HelpHint {
                width: u32::MAX,
                y: 1,
                ..hint()
            },
        ] {
            assert_eq!(help(&bad), Err(ERR_INVALID), "{bad:?}");
        }
    }
}
