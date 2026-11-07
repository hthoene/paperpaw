//! Registry declarations share the existing bounded declaration batches.
use super::menus::{menu_full_id, menu_identifier, need_bits};
use super::{Declarations, DeclareError, Item, Latched, filled, records_wire as wire};
use crate::native::settings::SettingValue;
use core::mem::MaybeUninit;

/// Page opened by a setup step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SetupPage<'a> {
    /// Open a setting's editor.
    Setting(&'a str),
    /// Open a declared screen.
    Screen(&'a str),
}

/// A setup step owned by this plugin. Applicability is host state, never a callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SetupStepDecl<'a> {
    /// Relative identity.
    pub name: &'a str,
    /// Title translation key.
    pub title: &'a str,
    /// Help translation key, empty for none.
    pub help: &'a str,
    /// Lower values appear first.
    pub order: i32,
    /// Page to open.
    pub page: SetupPage<'a>,
    /// Setting whose value must match, empty for no condition.
    pub applies_key: &'a str,
    /// Configuration text to match.
    pub applies_value: &'a str,
    /// Device requirement, absent for all devices.
    pub need: Option<crate::MenuNeed>,
    /// Applies until the plugin pushes its own item state.
    pub applies: bool,
}
impl<'a> SetupStepDecl<'a> {
    /// Declare an applicable step without a condition or device requirement.
    #[must_use]
    pub const fn new(name: &'a str, title: &'a str, page: SetupPage<'a>) -> Self {
        Self {
            name,
            title,
            help: "",
            order: 0,
            page,
            applies_key: "",
            applies_value: "",
            need: None,
            applies: true,
        }
    }
}

/// A named screen slot owned by this plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SlotDecl<'a> {
    /// Relative identity.
    pub name: &'a str,
    /// Title translation key.
    pub title: &'a str,
}
impl<'a> SlotDecl<'a> {
    /// Declare a screen slot.
    #[must_use]
    pub const fn new(name: &'a str, title: &'a str) -> Self {
        Self { name, title }
    }
}

/// Offer a screen of this plugin to any declared slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct SlotOfferDecl<'a> {
    /// Full slot identity.
    pub slot: &'a str,
    /// Full identity of this plugin's screen.
    pub screen: &'a str,
    /// Highest priority becomes the default.
    pub priority: i32,
}
impl<'a> SlotOfferDecl<'a> {
    /// Offer a screen at priority zero.
    #[must_use]
    pub const fn new(slot: &'a str, screen: &'a str) -> Self {
        Self {
            slot,
            screen,
            priority: 0,
        }
    }
}

/// A declared key binding with an optional setting condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct KeyBindingDecl<'a> {
    /// Full screen or slot identity, or global.
    pub layer: &'a str,
    /// Device input identity.
    pub input: &'a str,
    /// Full action identity, empty to bind nothing.
    pub action: &'a str,
    /// Setting whose value must match, empty for no condition.
    pub when_key: &'a str,
    /// Configuration text to match.
    pub when_value: &'a str,
}
impl<'a> KeyBindingDecl<'a> {
    /// Declare a binding without a setting condition.
    #[must_use]
    pub const fn new(layer: &'a str, input: &'a str, action: &'a str) -> Self {
        Self {
            layer,
            input,
            action,
            when_key: "",
            when_value: "",
        }
    }
}

/// A preset owned by this plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PresetDecl<'a> {
    /// Relative identity.
    pub name: &'a str,
    /// Full category identity.
    pub kind: &'a str,
    /// Title translation key.
    pub title: &'a str,
    /// Help translation key, empty for none.
    pub help: &'a str,
}
impl<'a> PresetDecl<'a> {
    /// Declare a preset without help text.
    #[must_use]
    pub const fn new(name: &'a str, kind: &'a str, title: &'a str) -> Self {
        Self {
            name,
            kind,
            title,
            help: "",
        }
    }
}

/// One value of a preset. Absent returns the setting to its default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PresetValueDecl<'a> {
    /// Full preset identity.
    pub preset: &'a str,
    /// Full setting identity.
    pub key: &'a str,
    /// Typed value, or Absent to reset.
    pub value: SettingValue<'a>,
}
impl<'a> PresetValueDecl<'a> {
    /// Declare one preset value.
    #[must_use]
    pub const fn new(preset: &'a str, key: &'a str, value: SettingValue<'a>) -> Self {
        Self { preset, key, value }
    }
}

impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    /// Declare a setup step. The host evaluates all applicability conditions.
    ///
    /// # Errors
    /// Returns field validation, encoding, allocation, or latched sink errors.
    pub fn setup_step(&mut self, step: SetupStepDecl<'_>) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(step.name, wire::SETUP_STEP_NAME_MAX_BYTES, "setup name")?;
            filled(step.title, "setup title")?;
            let (page_kind, page) = match step.page {
                SetupPage::Setting(page) => (0, page),
                SetupPage::Screen(page) => (1, page),
            };
            menu_full_id(page, wire::SETUP_STEP_PAGE_MAX_BYTES, "setup page")?;
            if !step.applies_key.is_empty() {
                menu_full_id(
                    step.applies_key,
                    wire::SETUP_STEP_APPLIES_KEY_MAX_BYTES,
                    "setup condition",
                )?;
            }
            let (need, capability) = step.need.map(need_bits).transpose()?.unwrap_or((0, 0));
            Ok(wire::Record::SetupStep(wire::SetupStep {
                name: step.name,
                title: step.title,
                help: step.help,
                order: step.order,
                page_kind,
                page,
                applies_key: step.applies_key,
                applies_value: step.applies_value,
                need,
                capability,
                flags: u32::from(step.applies) | (u32::from(step.need.is_some()) << 1),
            }))
        })();
        self.accept_registry(record)
    }

    /// Declare a screen slot.
    ///
    /// # Errors
    /// Returns field validation, encoding, allocation, or latched sink errors.
    pub fn slot(&mut self, slot: SlotDecl<'_>) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(slot.name, wire::SLOT_SPEC_NAME_MAX_BYTES, "slot name")?;
            filled(slot.title, "slot title")?;
            Ok(wire::Record::SlotSpec(wire::SlotSpec {
                name: slot.name,
                title: slot.title,
            }))
        })();
        self.accept_registry(record)
    }

    /// Offer a declared screen for a slot. The host resolves ownership and priority.
    ///
    /// # Errors
    /// Returns field validation, encoding, allocation, or latched sink errors.
    pub fn slot_offer(&mut self, offer: SlotOfferDecl<'_>) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_full_id(offer.slot, wire::SLOT_OFFER_SLOT_MAX_BYTES, "offered slot")?;
            menu_full_id(
                offer.screen,
                wire::SLOT_OFFER_SCREEN_MAX_BYTES,
                "offered screen",
            )?;
            Ok(wire::Record::SlotOffer(wire::SlotOffer {
                slot: offer.slot,
                screen: offer.screen,
                priority: offer.priority,
            }))
        })();
        self.accept_registry(record)
    }

    /// Declare a key binding. The host protects its fixed escape input.
    ///
    /// # Errors
    /// Returns field validation, encoding, allocation, or latched sink errors.
    pub fn key_binding(&mut self, binding: KeyBindingDecl<'_>) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            if binding.layer != "global" {
                menu_full_id(
                    binding.layer,
                    wire::KEY_BIND_LAYER_MAX_BYTES,
                    "binding layer",
                )?;
            }
            menu_full_id(
                binding.input,
                wire::KEY_BIND_INPUT_MAX_BYTES,
                "binding input",
            )?;
            if !binding.action.is_empty() {
                menu_full_id(
                    binding.action,
                    wire::KEY_BIND_ACTION_MAX_BYTES,
                    "binding action",
                )?;
            }
            if !binding.when_key.is_empty() {
                menu_full_id(
                    binding.when_key,
                    wire::KEY_BIND_WHEN_KEY_MAX_BYTES,
                    "binding condition",
                )?;
            }
            Ok(wire::Record::KeyBind(wire::KeyBind {
                layer: binding.layer,
                input: binding.input,
                action: binding.action,
                when_key: binding.when_key,
                when_value: binding.when_value,
            }))
        })();
        self.accept_registry(record)
    }

    /// Extend another plugin's default. The host applies extension precedence.
    ///
    /// # Errors
    /// Returns Invalid for an absent default, field validation, encoding or sink errors.
    pub fn setting_default(
        &mut self,
        key: &str,
        value: SettingValue<'_>,
    ) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let mut bytes = [MaybeUninit::uninit(); wire::SETTING_DEFAULT_VALUE_MAX_BYTES];
        let record = default_record(key, value, &mut bytes);
        self.accept_registry(record)
    }

    /// Add a translated choice to another plugin's setting.
    ///
    /// # Errors
    /// Returns field validation, encoding, allocation, or latched sink errors.
    pub fn setting_choice(
        &mut self,
        key: &str,
        value: &str,
        title: &str,
    ) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_full_id(key, wire::SETTING_CHOICE_KEY_MAX_BYTES, "foreign setting")?;
            filled(value, "choice value")?;
            filled(title, "choice title")?;
            Ok(wire::Record::SettingChoice(wire::SettingChoice {
                key,
                value,
                title,
            }))
        })();
        self.accept_registry(record)
    }

    /// Declare a preset and its category.
    ///
    /// # Errors
    /// Returns field validation, encoding, allocation, or latched sink errors.
    pub fn preset(&mut self, preset: PresetDecl<'_>) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(preset.name, wire::PRESET_SPEC_NAME_MAX_BYTES, "preset name")?;
            menu_full_id(preset.kind, wire::PRESET_SPEC_KIND_MAX_BYTES, "preset kind")?;
            filled(preset.title, "preset title")?;
            Ok(wire::Record::PresetSpec(wire::PresetSpec {
                name: preset.name,
                kind: preset.kind,
                title: preset.title,
                help: preset.help,
            }))
        })();
        self.accept_registry(record)
    }

    /// Declare one value of a preset, or reset it to its default with Absent.
    ///
    /// # Errors
    /// Returns field validation, encoding, allocation, or latched sink errors.
    pub fn preset_value(&mut self, value: PresetValueDecl<'_>) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let mut bytes = [MaybeUninit::uninit(); wire::PRESET_VALUE_VALUE_MAX_BYTES];
        let record = preset_record(value, &mut bytes);
        self.accept_registry(record)
    }

    fn accept_registry(
        &mut self,
        record: Result<wire::Record<'_>, DeclareError>,
    ) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        if let Err(error) = record.and_then(|record| self.push(Item::Registry(record))) {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }
}

fn default_record<'a>(
    key: &'a str,
    value: SettingValue<'_>,
    bytes: &'a mut [MaybeUninit<u8>],
) -> Result<wire::Record<'a>, DeclareError> {
    menu_full_id(key, wire::SETTING_DEFAULT_KEY_MAX_BYTES, "foreign setting")?;
    let value = encode_value(value, bytes)?;
    Ok(wire::Record::SettingDefault(wire::SettingDefault {
        key,
        value,
    }))
}

fn preset_record<'a>(
    value: PresetValueDecl<'a>,
    bytes: &'a mut [MaybeUninit<u8>],
) -> Result<wire::Record<'a>, DeclareError> {
    menu_full_id(value.preset, wire::PRESET_VALUE_PRESET_MAX_BYTES, "preset")?;
    menu_full_id(
        value.key,
        wire::PRESET_VALUE_KEY_MAX_BYTES,
        "preset setting",
    )?;
    let reset = matches!(value.value, SettingValue::Absent);
    let encoded = if reset {
        &[]
    } else {
        encode_value(value.value, bytes)?
    };
    Ok(wire::Record::PresetValue(wire::PresetValue {
        preset: value.preset,
        key: value.key,
        value: encoded,
        flags: u32::from(reset),
    }))
}

fn encode_value<'a>(
    value: SettingValue<'_>,
    out: &'a mut [MaybeUninit<u8>],
) -> Result<&'a [u8], DeclareError> {
    use super::settings_value::{SettingValueRef, settings_encode};
    let value = match value {
        SettingValue::Absent => return Err(DeclareError::Default),
        SettingValue::Bool(value) => SettingValueRef::Bool(value),
        SettingValue::Int(value) => SettingValueRef::Int(value),
        SettingValue::Text(value) => SettingValueRef::Text(value),
    };
    let length = settings_encode(value, out).map_err(DeclareError::Encode)?;
    if length > out.len() {
        return Err(DeclareError::Encode(
            paperpaw_native::host_glue::ERR_NO_SPACE,
        ));
    }
    // SAFETY: the generated encoder initialized exactly this prefix; u8 has
    // the same layout as MaybeUninit<u8>, and the output borrow remains live.
    Ok(unsafe { core::slice::from_raw_parts(out.as_ptr().cast(), length) })
}

#[cfg(test)]
mod tests;
