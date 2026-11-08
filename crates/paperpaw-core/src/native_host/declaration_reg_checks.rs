//! Allocation-free REG declaration checks before catalog admission.
use super::{
    declarations::{full_id, need_value_check, relative},
    registrations::{Record, SetupStep},
};
use crate::abi7::settings_wire::{SettingValueRef, settings_decode};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

/// Registry-free identity checks; resolving screens and slots waits for the
/// catalog commit, where a plugin's complete declaration set is available.
pub(crate) fn validate(plugin: &str, record: &Record<'_>) -> Result<(), i32> {
    match record {
        Record::SetupStep(step) => setup_step(plugin, step),
        Record::SlotSpec(slot) => {
            relative(plugin, slot.name)?;
            nonempty(slot.title)
        }
        Record::SlotOffer(offer) => {
            qualified(offer.slot)?;
            qualified(offer.screen)?;
            let own = offer
                .screen
                .strip_prefix(plugin)
                .and_then(|s| s.strip_prefix('.'));
            match own {
                Some(name) => relative(plugin, name),
                None => Err(ERR_INVALID),
            }
        }
        Record::KeyBind(binding) => {
            if binding.layer != "global" {
                qualified(binding.layer)?;
            }
            qualified(binding.input)?;
            if !binding.action.is_empty() {
                qualified(binding.action)?;
            }
            if binding.when_key.is_empty() != binding.when_value.is_empty() {
                return Err(ERR_INVALID);
            }
            if !binding.when_key.is_empty() {
                qualified(binding.when_key)?;
            }
            Ok(())
        }
        Record::PresetSpec(preset) => {
            relative(plugin, preset.name)?;
            qualified(preset.kind)?;
            nonempty(preset.title)
        }
        Record::SettingDefault(default) => {
            qualified(default.key)?;
            present_value(default.value)
        }
        Record::SettingChoice(choice) => {
            qualified(choice.key)?;
            nonempty(choice.value)?;
            nonempty(choice.title)
        }
        Record::PresetValue(value) => {
            qualified(value.preset)?;
            qualified(value.key)?;
            if value.flags & !1 != 0 {
                return Err(ERR_UNSUPPORTED);
            }
            if value.flags & 1 == 0 {
                present_value(value.value)
            } else if value.value.is_empty() {
                Ok(())
            } else {
                Err(ERR_INVALID)
            }
        }
        _ => Err(ERR_UNSUPPORTED),
    }
}

fn present_value(bytes: &[u8]) -> Result<(), i32> {
    match settings_decode(bytes)? {
        SettingValueRef::Absent => Err(ERR_INVALID),
        _ => Ok(()),
    }
}

fn qualified(id: &str) -> Result<(), i32> {
    full_id(id)?;
    if id.contains('.') {
        Ok(())
    } else {
        Err(ERR_INVALID)
    }
}
fn nonempty(value: &str) -> Result<(), i32> {
    if value.is_empty() {
        Err(ERR_INVALID)
    } else {
        Ok(())
    }
}

/// Local setup syntax only; catalog references and name collisions are checked
/// when construction commits. Unknown device needs remain valid but unmet.
pub(crate) fn setup_step(plugin: &str, step: &SetupStep<'_>) -> Result<(), i32> {
    let SetupStep {
        name,
        title,
        help: _,
        order: _,
        page_kind,
        page,
        applies_key,
        applies_value,
        need,
        capability,
        flags,
    } = step;
    relative(plugin, name)?;
    if title.is_empty() || flags & !3 != 0 {
        return Err(ERR_INVALID);
    }
    if *page_kind > 1 {
        return Err(ERR_UNSUPPORTED);
    }
    full_id(page)?;
    if !page.contains('.') || applies_key.is_empty() != applies_value.is_empty() {
        return Err(ERR_INVALID);
    }
    if !applies_key.is_empty() {
        full_id(applies_key)?;
        if !applies_key.contains('.') {
            return Err(ERR_INVALID);
        }
    }
    if flags & 2 == 0 {
        if *need != 0 || *capability != 0 {
            return Err(ERR_INVALID);
        }
        Ok(())
    } else {
        need_value_check(*need, *capability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_host::{declarations::validate_record, registrations::Record};

    fn step() -> SetupStep<'static> {
        SetupStep {
            name: "first-run",
            title: "First run",
            help: "",
            order: 0,
            page_kind: 0,
            page: "fixture.configure",
            applies_key: "",
            applies_value: "",
            need: 0,
            capability: 0,
            flags: 1,
        }
    }

    #[test]
    fn setup_checks_device_need_presence_without_refusing_future_needs() {
        let base = step();
        assert_eq!(setup_step("fixture", &base), Ok(()));
        for row in [
            SetupStep { need: 1, ..base },
            SetupStep {
                capability: 1,
                ..base
            },
            SetupStep {
                flags: 3,
                need: 12,
                ..base
            },
            SetupStep {
                flags: 3,
                need: 1,
                capability: 1,
                ..base
            },
        ] {
            assert_eq!(setup_step("fixture", &row), Err(ERR_INVALID));
        }
        for row in [
            SetupStep {
                flags: 3,
                need: 1,
                ..base
            },
            SetupStep {
                flags: 3,
                need: 12,
                capability: 1,
                ..base
            },
            SetupStep {
                flags: 3,
                need: u32::MAX,
                capability: 1,
                ..base
            },
        ] {
            assert_eq!(setup_step("fixture", &row), Ok(()));
        }
    }

    #[test]
    fn setup_refuses_bad_names_flags_pages_and_applicability_before_admission() {
        let base = step();
        for row in [
            SetupStep { name: "", ..base },
            SetupStep { title: "", ..base },
            SetupStep { flags: 4, ..base },
            SetupStep {
                page: "relative",
                ..base
            },
            SetupStep {
                applies_key: "fixture.enabled",
                ..base
            },
            SetupStep {
                applies_value: "true",
                ..base
            },
            SetupStep {
                applies_key: "bad id",
                applies_value: "true",
                ..base
            },
        ] {
            assert_eq!(
                validate_record("fixture", &Record::SetupStep(row)),
                Err(ERR_INVALID)
            );
        }
        assert_eq!(
            validate_record("fixture", &Record::SetupStep(base)),
            Err(ERR_UNSUPPORTED)
        );
        assert_eq!(
            setup_step(
                "fixture",
                &SetupStep {
                    page_kind: 2,
                    ..base
                }
            ),
            Err(ERR_UNSUPPORTED)
        );
        assert_eq!(
            setup_step(
                "fixture",
                &SetupStep {
                    page_kind: 1,
                    applies_key: "fixture.enabled",
                    applies_value: "true",
                    ..base
                }
            ),
            Ok(())
        );
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    use crate::native_host::registrations::{KeyBind, PresetSpec, SlotOffer, SlotSpec};

    #[test]
    fn slot_offers_check_ownership_without_resolving_load_order() {
        for screen in ["fixture.reader", "fixture.reader.detail"] {
            assert_eq!(
                validate(
                    "fixture",
                    &Record::SlotOffer(SlotOffer {
                        slot: "core.home",
                        screen,
                        priority: i32::MIN,
                    })
                ),
                Ok(())
            );
        }
        for (slot, screen) in [
            ("slot", "fixture.reader"),
            ("core.home", "fixturex.reader"),
            ("core.home", "other.reader"),
            ("core.home", "fixture."),
        ] {
            assert_eq!(
                validate(
                    "fixture",
                    &Record::SlotOffer(SlotOffer {
                        slot,
                        screen,
                        priority: 0,
                    })
                ),
                Err(ERR_INVALID)
            );
        }
    }

    #[test]
    fn binding_conditions_and_global_nothing_binding_keep_their_meaning() {
        let base = KeyBind {
            layer: "global",
            input: "key.next",
            action: "",
            when_key: "",
            when_value: "",
        };
        assert_eq!(validate("fixture", &Record::KeyBind(base)), Ok(()));
        assert_eq!(
            validate(
                "fixture",
                &Record::KeyBind(KeyBind {
                    layer: "core.home",
                    action: "fixture.next",
                    when_key: "fixture.enabled",
                    when_value: "true",
                    ..base
                })
            ),
            Ok(())
        );
        for row in [
            KeyBind {
                layer: "local",
                ..base
            },
            KeyBind {
                input: "next",
                ..base
            },
            KeyBind {
                action: "bad id",
                ..base
            },
            KeyBind {
                when_key: "fixture.enabled",
                ..base
            },
            KeyBind {
                when_value: "true",
                ..base
            },
        ] {
            assert_eq!(validate("fixture", &Record::KeyBind(row)), Err(ERR_INVALID));
        }
    }

    #[test]
    fn named_slot_and_preset_declarations_require_nonempty_titles() {
        assert_eq!(
            validate(
                "fixture",
                &Record::SlotSpec(SlotSpec {
                    name: "home",
                    title: "Home"
                })
            ),
            Ok(())
        );
        assert_eq!(
            validate(
                "fixture",
                &Record::SlotSpec(SlotSpec {
                    name: "home",
                    title: ""
                })
            ),
            Err(ERR_INVALID)
        );
        let base = PresetSpec {
            name: "night",
            kind: "core.theme",
            title: "Night",
            help: "",
        };
        assert_eq!(validate("fixture", &Record::PresetSpec(base)), Ok(()));
        for row in [
            PresetSpec { name: "", ..base },
            PresetSpec { title: "", ..base },
            PresetSpec {
                kind: "theme",
                ..base
            },
        ] {
            assert_eq!(
                validate("fixture", &Record::PresetSpec(row)),
                Err(ERR_INVALID)
            );
        }
    }
}

#[cfg(test)]
mod value_tests {
    use super::*;
    use crate::{
        abi7::settings_wire::settings_encode,
        native_host::registrations::{PresetValue, SettingChoice, SettingDefault},
    };
    use core::mem::MaybeUninit;

    fn encoded(value: SettingValueRef<'_>) -> [u8; 13] {
        let mut bytes = [MaybeUninit::new(0); 13];
        assert!(settings_encode(value, &mut bytes).is_ok());
        // SAFETY: every byte was initialized before the encoder wrote any prefix.
        bytes.map(|byte| unsafe { byte.assume_init() })
    }

    #[test]
    fn defaults_require_present_canonical_values_without_foreign_policy_checks() {
        let value = encoded(SettingValueRef::Bool(true));
        for key in ["foreign.enabled", "core.grant.fixture.enabled"] {
            assert_eq!(
                validate(
                    "fixture",
                    &Record::SettingDefault(SettingDefault { key, value: &value })
                ),
                Ok(())
            );
        }
        let absent = encoded(SettingValueRef::Absent);
        for value in [&[][..], &[1, 2, 3][..], &absent[..12]] {
            assert_eq!(
                validate(
                    "fixture",
                    &Record::SettingDefault(SettingDefault {
                        key: "foreign.enabled",
                        value
                    })
                ),
                Err(ERR_INVALID)
            );
        }
    }

    #[test]
    fn preset_reset_requires_empty_bytes_and_unknown_flags_remain_unsupported() {
        let reset = PresetValue {
            preset: "fixture.night",
            key: "foreign.enabled",
            value: &[],
            flags: 1,
        };
        assert_eq!(validate("fixture", &Record::PresetValue(reset)), Ok(()));
        let value = encoded(SettingValueRef::Bool(false));
        assert_eq!(
            validate(
                "fixture",
                &Record::PresetValue(PresetValue {
                    value: &value,
                    flags: 0,
                    ..reset
                })
            ),
            Ok(())
        );
        assert_eq!(
            validate(
                "fixture",
                &Record::PresetValue(PresetValue {
                    value: &value,
                    ..reset
                })
            ),
            Err(ERR_INVALID)
        );
        assert_eq!(
            validate(
                "fixture",
                &Record::PresetValue(PresetValue { flags: 0, ..reset })
            ),
            Err(ERR_INVALID)
        );
        assert_eq!(
            validate(
                "fixture",
                &Record::PresetValue(PresetValue { flags: 2, ..reset })
            ),
            Err(ERR_UNSUPPORTED)
        );
    }

    #[test]
    fn choice_extensions_check_only_local_identity_value_and_title() {
        let choice = SettingChoice {
            key: "foreign.color",
            value: "blue",
            title: "Blue",
        };
        assert_eq!(validate("fixture", &Record::SettingChoice(choice)), Ok(()));
        for row in [
            SettingChoice {
                key: "color",
                ..choice
            },
            SettingChoice {
                value: "",
                ..choice
            },
            SettingChoice {
                title: "",
                ..choice
            },
        ] {
            assert_eq!(
                validate("fixture", &Record::SettingChoice(row)),
                Err(ERR_INVALID)
            );
        }
    }
}
