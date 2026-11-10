//! Borrowed typed registry rows. Generated wire types stay private.
use super::{Error, expect, wire};
use crate::native::{
    settings::SettingValue,
    wire::settings_wire::{SettingValueRef, settings_decode},
};

/// A registry category with unknown future values preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistrySettingKind {
    /// Bool category.
    Bool,
    /// Int category.
    Int,
    /// Choice category.
    Choice,
    /// Text category.
    Text,
    /// Language category.
    Language,
    /// List category.
    List,
    /// A category introduced by a newer host.
    Unknown(u32),
}
impl RegistrySettingKind {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::Bool,
            1 => Self::Int,
            2 => Self::Choice,
            3 => Self::Text,
            4 => Self::Language,
            5 => Self::List,
            other => Self::Unknown(other),
        }
    }
}

/// A registry category with unknown future values preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryValueSource {
    /// Default category.
    Default,
    /// Preset category.
    Preset,
    /// Profile category.
    Profile,
    /// User category.
    User,
    /// BookScope category.
    BookScope,
    /// A category introduced by a newer host.
    Unknown(u32),
}
impl RegistryValueSource {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::Default,
            1 => Self::Preset,
            2 => Self::Profile,
            3 => Self::User,
            4 => Self::BookScope,
            other => Self::Unknown(other),
        }
    }
}

/// A registry category with unknown future values preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryPluginKind {
    /// BuiltIn category.
    BuiltIn,
    /// Data category.
    Data,
    /// Code category.
    Code,
    /// A category introduced by a newer host.
    Unknown(u32),
}
impl RegistryPluginKind {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::BuiltIn,
            1 => Self::Data,
            2 => Self::Code,
            other => Self::Unknown(other),
        }
    }
}

/// A registry category with unknown future values preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryPluginState {
    /// Running category.
    Running,
    /// Off category.
    Off,
    /// Failed category.
    Failed,
    /// A category introduced by a newer host.
    Unknown(u32),
}
impl RegistryPluginState {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::Running,
            1 => Self::Off,
            2 => Self::Failed,
            other => Self::Unknown(other),
        }
    }
}

/// A registry category with unknown future values preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryNeedKind {
    /// Feature category.
    Feature,
    /// Service category.
    Service,
    /// Plugin category.
    Plugin,
    /// A category introduced by a newer host.
    Unknown(u32),
}
impl RegistryNeedKind {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::Feature,
            1 => Self::Service,
            2 => Self::Plugin,
            other => Self::Unknown(other),
        }
    }
}

/// A registry category with unknown future values preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryMenuKind {
    /// Action category.
    Action,
    /// Setting category.
    Setting,
    /// Submenu category.
    Submenu,
    /// Section category.
    Section,
    /// A category introduced by a newer host.
    Unknown(u32),
}
impl RegistryMenuKind {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::Action,
            1 => Self::Setting,
            2 => Self::Submenu,
            3 => Self::Section,
            other => Self::Unknown(other),
        }
    }
}

/// A registry category with unknown future values preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryBindingOrigin {
    /// Plugin category.
    Plugin,
    /// Device category.
    Device,
    /// Preset category.
    Preset,
    /// User category.
    User,
    /// A category introduced by a newer host.
    Unknown(u32),
}
impl RegistryBindingOrigin {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::Plugin,
            1 => Self::Device,
            2 => Self::Preset,
            3 => Self::User,
            other => Self::Unknown(other),
        }
    }
}

/// A registry category with unknown future values preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistrySetupPageKind {
    /// Setting category.
    Setting,
    /// Screen category.
    Screen,
    /// A category introduced by a newer host.
    Unknown(u32),
}
impl RegistrySetupPageKind {
    const fn from_wire(value: u32) -> Self {
        match value {
            0 => Self::Setting,
            1 => Self::Screen,
            other => Self::Unknown(other),
        }
    }
}

/// A setting and its current readable value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistrySetting<'a> {
    row: wire::ServiceRegSetting<'a>,
}
impl<'a> RegistrySetting<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegSetting<'_> = expect(record)?;
        if row.key.is_empty() {
            return Err(Error::Invalid);
        }
        validate_value(row.value, row.flags & ((1 << 1) | (1 << 5) | (1 << 7)) != 0)?;
        Ok(Self { row })
    }
    /// Full setting or requirement key, never truncated.
    #[must_use]
    pub fn key(&self) -> &'a str {
        self.row.key
    }
    /// Owning settings group.
    #[must_use]
    pub fn group(&self) -> &'a str {
        self.row.group
    }
    /// Typed row category; future values are preserved.
    #[must_use]
    pub fn kind(&self) -> RegistrySettingKind {
        RegistrySettingKind::from_wire(self.row.kind)
    }
    /// Contract flags; unknown bits are preserved.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.row.flags
    }
    /// Source of the effective value.
    #[must_use]
    pub fn source(&self) -> RegistryValueSource {
        RegistryValueSource::from_wire(self.row.source)
    }
    /// Text translated in the host language.
    #[must_use]
    pub fn title(&self) -> &'a str {
        self.row.title
    }
    /// Readable typed value, or None when withheld, long or reset.
    #[must_use]
    pub fn value(&self) -> Option<SettingValue<'a>> {
        decoded_value(self.row.value)
    }
}

/// Additional metadata for a single setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistrySettingExtra<'a> {
    row: wire::ServiceRegSettingExtra<'a>,
}
impl<'a> RegistrySettingExtra<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegSettingExtra<'_> = expect(record)?;
        Ok(Self { row })
    }
    /// Smallest numeric value.
    #[must_use]
    pub fn min(&self) -> i32 {
        self.row.min
    }
    /// Largest numeric value.
    #[must_use]
    pub fn max(&self) -> i32 {
        self.row.max
    }
    /// Numeric adjustment step.
    #[must_use]
    pub fn step(&self) -> i32 {
        self.row.step
    }
    /// Translated value unit.
    #[must_use]
    pub fn unit(&self) -> &'a str {
        self.row.unit
    }
    /// Section identity.
    #[must_use]
    pub fn section(&self) -> &'a str {
        self.row.section
    }
    /// Followed setting identity.
    #[must_use]
    pub fn follows(&self) -> &'a str {
        self.row.follows
    }
    /// Setting controlling visibility.
    #[must_use]
    pub fn shown_id(&self) -> &'a str {
        self.row.shown_id
    }
    /// Text value needed for visibility.
    #[must_use]
    pub fn shown_value(&self) -> &'a str {
        self.row.shown_value
    }
    /// Editor-kind code from the contract.
    #[must_use]
    pub fn editor_kind(&self) -> u32 {
        self.row.editor_kind
    }
    /// Editor identity, empty when none.
    #[must_use]
    pub fn editor(&self) -> &'a str {
        self.row.editor
    }
    /// Number of choices; enumerate them with the choices operation.
    #[must_use]
    pub fn choices(&self) -> u32 {
        self.row.choices
    }
}

/// One translated choice in declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryChoice<'a> {
    row: wire::ServiceRegChoice<'a>,
}
impl<'a> RegistryChoice<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegChoice<'_> = expect(record)?;
        if row.value.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { row })
    }
    /// Complete choice value, without translation or truncation.
    #[must_use]
    pub fn value(&self) -> &'a str {
        self.row.value
    }
    /// Text translated in the host language.
    #[must_use]
    pub fn title(&self) -> &'a str {
        self.row.title
    }
}

/// A translated settings group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryGroup<'a> {
    row: wire::ServiceRegGroup<'a>,
}
impl<'a> RegistryGroup<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegGroup<'_> = expect(record)?;
        if row.id.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { row })
    }
    /// Full identity, never truncated.
    #[must_use]
    pub fn id(&self) -> &'a str {
        self.row.id
    }
    /// Parent group identity, empty for none.
    #[must_use]
    pub fn parent(&self) -> &'a str {
        self.row.parent
    }
    /// Page identity.
    #[must_use]
    pub fn page(&self) -> &'a str {
        self.row.page
    }
    /// Display order, smallest first.
    #[must_use]
    pub fn order(&self) -> i32 {
        self.row.order
    }
    /// Text translated in the host language.
    #[must_use]
    pub fn title(&self) -> &'a str {
        self.row.title
    }
}

/// One known plugin, including its next-start state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryPlugin<'a> {
    row: wire::ServiceRegPlugin<'a>,
}
impl<'a> RegistryPlugin<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegPlugin<'_> = expect(record)?;
        if row.id.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { row })
    }
    /// Full identity, never truncated.
    #[must_use]
    pub fn id(&self) -> &'a str {
        self.row.id
    }
    /// Translated plugin name.
    #[must_use]
    pub fn name(&self) -> &'a str {
        self.row.name
    }
    /// Installed version text.
    #[must_use]
    pub fn version(&self) -> &'a str {
        self.row.version
    }
    /// Typed row category; future values are preserved.
    #[must_use]
    pub fn kind(&self) -> RegistryPluginKind {
        RegistryPluginKind::from_wire(self.row.kind)
    }
    /// Current plugin running state.
    #[must_use]
    pub fn state(&self) -> RegistryPluginState {
        RegistryPluginState::from_wire(self.row.state)
    }
    /// Contract flags; unknown bits are preserved.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.row.flags
    }
    /// Host plugin-problem code, zero for none.
    #[must_use]
    pub fn problem(&self) -> u32 {
        self.row.problem
    }
    /// Numeric argument for the plugin problem.
    #[must_use]
    pub fn problem_arg(&self) -> u32 {
        self.row.problem_arg
    }
    /// Pending update version, empty when none.
    #[must_use]
    pub fn update(&self) -> &'a str {
        self.row.update
    }
}

/// One feature, service or plugin requirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryNeed<'a> {
    row: wire::ServiceRegNeed<'a>,
}
impl<'a> RegistryNeed<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegNeed<'_> = expect(record)?;
        if row.key.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { row })
    }
    /// Typed row category; future values are preserved.
    #[must_use]
    pub fn kind(&self) -> RegistryNeedKind {
        RegistryNeedKind::from_wire(self.row.kind)
    }
    /// Full setting or requirement key, never truncated.
    #[must_use]
    pub fn key(&self) -> &'a str {
        self.row.key
    }
    /// Installed version text.
    #[must_use]
    pub fn version(&self) -> &'a str {
        self.row.version
    }
    /// Contract flags; unknown bits are preserved.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.row.flags
    }
}

/// An action, screen, input, menu or preset identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryItem<'a> {
    row: wire::ServiceRegItem<'a>,
}
impl<'a> RegistryItem<'a> {
    /// For a menu row from `Registry::menus`, a user definition exists.
    /// Empty definitions and definitions equal to defaults also return true.
    #[must_use]
    pub fn user_defined(&self) -> bool {
        self.row.flags & (1 << 5) != 0
    }

    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegItem<'_> = expect(record)?;
        if row.id.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { row })
    }
    /// Full identity, never truncated.
    #[must_use]
    pub fn id(&self) -> &'a str {
        self.row.id
    }
    /// Owning plugin identity.
    #[must_use]
    pub fn plugin(&self) -> &'a str {
        self.row.plugin
    }
    /// Contract flags; unknown bits are preserved.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.row.flags
    }
    /// Text translated in the host language.
    #[must_use]
    pub fn title(&self) -> &'a str {
        self.row.title
    }
}

/// A slot with its current and default screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistrySlot<'a> {
    row: wire::ServiceRegSlot<'a>,
}
impl<'a> RegistrySlot<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegSlot<'_> = expect(record)?;
        if row.id.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { row })
    }
    /// Full identity, never truncated.
    #[must_use]
    pub fn id(&self) -> &'a str {
        self.row.id
    }
    /// Current screen identity.
    #[must_use]
    pub fn screen(&self) -> &'a str {
        self.row.screen
    }
    /// Default screen identity.
    #[must_use]
    pub fn default(&self) -> &'a str {
        self.row.default
    }
    /// Contract flags; unknown bits are preserved.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.row.flags
    }
    /// Text translated in the host language.
    #[must_use]
    pub fn title(&self) -> &'a str {
        self.row.title
    }
}

/// One evaluated menu row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryMenuRow<'a> {
    row: wire::ServiceRegMenuRow<'a>,
}
impl<'a> RegistryMenuRow<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegMenuRow<'_> = expect(record)?;
        if row.item.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { row })
    }
    /// Menu item identity.
    #[must_use]
    pub fn item(&self) -> &'a str {
        self.row.item
    }
    /// Typed row category; future values are preserved.
    #[must_use]
    pub fn kind(&self) -> RegistryMenuKind {
        RegistryMenuKind::from_wire(self.row.kind)
    }
    /// Contract flags; unknown bits are preserved.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.row.flags
    }
    /// Display order, smallest first.
    #[must_use]
    pub fn order(&self) -> i32 {
        self.row.order
    }
    /// Plugin declaring or hiding this row.
    #[must_use]
    pub fn by(&self) -> &'a str {
        self.row.by
    }
    /// Text translated in the host language.
    #[must_use]
    pub fn title(&self) -> &'a str {
        self.row.title
    }
    /// Icon identity, empty when none.
    #[must_use]
    pub fn icon(&self) -> &'a str {
        self.row.icon
    }
    /// Translated badge text.
    #[must_use]
    pub fn badge(&self) -> &'a str {
        self.row.badge
    }
    /// Translated description text.
    #[must_use]
    pub fn describe(&self) -> &'a str {
        self.row.describe
    }
}

/// A key binding in one layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryBinding<'a> {
    row: wire::ServiceRegBinding<'a>,
}
impl<'a> RegistryBinding<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegBinding<'_> = expect(record)?;
        if row.layer.is_empty() || row.input.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { row })
    }
    /// Binding layer identity.
    #[must_use]
    pub fn layer(&self) -> &'a str {
        self.row.layer
    }
    /// Input identity.
    #[must_use]
    pub fn input(&self) -> &'a str {
        self.row.input
    }
    /// Action identity, empty for bound to nothing.
    #[must_use]
    pub fn action(&self) -> &'a str {
        self.row.action
    }
    /// Source of this binding.
    #[must_use]
    pub fn origin(&self) -> RegistryBindingOrigin {
        RegistryBindingOrigin::from_wire(self.row.origin)
    }
}

/// A setup step with current applicability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistrySetupStep<'a> {
    row: wire::ServiceRegSetupStep<'a>,
}
impl<'a> RegistrySetupStep<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegSetupStep<'_> = expect(record)?;
        if row.id.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { row })
    }
    /// Full identity, never truncated.
    #[must_use]
    pub fn id(&self) -> &'a str {
        self.row.id
    }
    /// Whether the page is a setting or a screen.
    #[must_use]
    pub fn page_kind(&self) -> RegistrySetupPageKind {
        RegistrySetupPageKind::from_wire(self.row.page_kind)
    }
    /// Page identity.
    #[must_use]
    pub fn page(&self) -> &'a str {
        self.row.page
    }
    /// Display order, smallest first.
    #[must_use]
    pub fn order(&self) -> i32 {
        self.row.order
    }
    /// Contract flags; unknown bits are preserved.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.row.flags
    }
    /// Text translated in the host language.
    #[must_use]
    pub fn title(&self) -> &'a str {
        self.row.title
    }
}

/// One preset or placement value, never truncated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryValue<'a> {
    row: wire::ServiceRegValue<'a>,
}
impl<'a> RegistryValue<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegValue<'_> = expect(record)?;
        if row.key.is_empty() {
            return Err(Error::Invalid);
        }
        validate_value(row.value, row.flags & 3 != 0)?;
        Ok(Self { row })
    }
    /// Full setting or requirement key, never truncated.
    #[must_use]
    pub fn key(&self) -> &'a str {
        self.row.key
    }
    /// Readable typed value, or None when withheld, long or reset.
    #[must_use]
    pub fn value(&self) -> Option<SettingValue<'a>> {
        decoded_value(self.row.value)
    }
    /// Contract flags; unknown bits are preserved.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.row.flags
    }
}

/// A discoverable widget and its category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryWidget<'a> {
    row: wire::ServiceRegWidget<'a>,
}
impl<'a> RegistryWidget<'a> {
    pub(super) fn decode(record: wire::RawRecord<'a>) -> Result<Self, Error> {
        let row: wire::ServiceRegWidget<'_> = expect(record)?;
        if row.id.is_empty() {
            return Err(Error::Invalid);
        }
        Ok(Self { row })
    }
    /// Full identity, never truncated.
    #[must_use]
    pub fn id(&self) -> &'a str {
        self.row.id
    }
    /// Owning plugin identity.
    #[must_use]
    pub fn plugin(&self) -> &'a str {
        self.row.plugin
    }
    /// Typed row category; future values are preserved.
    #[must_use]
    pub fn kind(&self) -> &'a str {
        self.row.kind
    }
    /// Icon identity, empty when none.
    #[must_use]
    pub fn icon(&self) -> &'a str {
        self.row.icon
    }
    /// Contract flags; unknown bits are preserved.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.row.flags
    }
    /// Text translated in the host language.
    #[must_use]
    pub fn title(&self) -> &'a str {
        self.row.title
    }
    /// The columns and rows it declared (`widget.spec.cells`), 0 each where
    /// the place decides.
    #[must_use]
    pub fn cells(&self) -> (u8, u8) {
        let [columns, rows, _, _] = self.row.cells.to_le_bytes();
        (columns, rows)
    }
    /// The least columns and rows it is shown in, at least 1 each.
    #[must_use]
    pub fn min_cells(&self) -> (u8, u8) {
        let [_, _, columns, rows] = self.row.cells.to_le_bytes();
        (columns.max(1), rows.max(1))
    }
    /// The settings group it previews (`widget.spec.previews`), empty for none.
    /// What the registry reports: a settings page lists the widgets whose
    /// `previews` is its group (see `Registry::widgets_previewing`).
    #[must_use]
    pub fn previews(&self) -> &'a str {
        self.row.previews
    }
    /// It stays as wide as it is high (`flags` bit 2).
    #[must_use]
    pub fn square(&self) -> bool {
        self.row.flags & 4 != 0
    }
}

fn validate_value(bytes: &[u8], withheld: bool) -> Result<(), Error> {
    if withheld {
        return if bytes.is_empty() {
            Ok(())
        } else {
            Err(Error::Invalid)
        };
    }
    if bytes.is_empty() {
        return Err(Error::Invalid);
    }
    settings_decode(bytes)
        .map(|_| ())
        .map_err(|_| Error::Invalid)
}
fn decoded_value(bytes: &[u8]) -> Option<SettingValue<'_>> {
    let value = settings_decode(bytes).ok()?;
    Some(match value {
        SettingValueRef::Absent => SettingValue::Absent,
        SettingValueRef::Bool(value) => SettingValue::Bool(value),
        SettingValueRef::Int(value) => SettingValue::Int(value),
        SettingValueRef::Text(value) => SettingValue::Text(value),
    })
}
