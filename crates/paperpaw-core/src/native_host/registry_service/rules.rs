//! The two rules several lists evaluate where they are read: a device need
//! (`menu.need`, `setup.step.need`) and a setting condition (`setup.step`
//! `applies_key`, `widget.place` `shown_if`), `registry.md` section 6.
use paperpaw_sdk::{Capabilities, Capability, MenuNeed, Settings};

/// The capabilities `menu.need` 12 (`has`) names, by bit (`screens-input.md`
/// section 1); a bit past this list is not known, so not met.
const CAPABILITIES: [Capability; 17] = [
    Capability::AUDIO,
    Capability::WARMTH,
    Capability::CHARGE_LIMIT,
    Capability::KEYBOARD,
    Capability::HOT_CODE,
    Capability::PAGE_KEYS,
    Capability::BUTTONS,
    Capability::USB_LINK,
    Capability::RENDER_HOOK,
    Capability::RENDER_REGIONS,
    Capability::LOOK_EXTRAS,
    Capability::USB_DRIVE,
    Capability::CHORDS,
    Capability::PAGE_TURNER,
    Capability::HOTSPOT,
    Capability::PEN_TILT,
    Capability::PEN_HOVER,
];

/// `menu.need` 12.
const HAS: u32 = 12;

/// Whether a device that can do `caps` meets need `need` with `capability`.
/// A need this host does not know is not met, never an error.
pub(super) fn need_met(need: u32, capability: u64, caps: Capabilities) -> bool {
    let known = match need {
        0 => MenuNeed::Speech,
        1 => MenuNeed::Sleep,
        2 => MenuNeed::Rotation,
        3 => MenuNeed::NoTouch,
        4 => MenuNeed::Touch,
        5 => MenuNeed::ButtonRow,
        6 => MenuNeed::Colors,
        7 => MenuNeed::Wifi,
        8 => MenuNeed::Firmware,
        9 => MenuNeed::BlackAndWhite,
        10 => MenuNeed::Frontlight,
        11 => MenuNeed::Bluetooth,
        HAS => return has(capability, caps),
        _ => return false,
    };
    known.met(caps)
}

/// Every bit of `capability` is a capability the device has (and at least
/// one is named).
fn has(capability: u64, caps: Capabilities) -> bool {
    capability != 0
        && capability >> CAPABILITIES.len() == 0
        && CAPABILITIES
            .iter()
            .enumerate()
            .all(|(bit, each)| capability >> bit & 1 == 0 || caps.has(*each))
}

/// Whether setting `key` has the value `value`, read as that setting reads it
/// (`on`, `true`, ...); an empty key always holds, and so does one naming no
/// setting (as `SettingExtra::shown_if`: a setting not there hides nothing).
pub(super) fn value_holds(settings: &Settings, key: &str, value: &str) -> bool {
    if key.is_empty() {
        return true;
    }
    let Some(spec) = settings.spec(key) else {
        return true;
    };
    spec.kind
        .parse(value)
        .is_some_and(|value| settings.get(key) == Some(&value))
}
