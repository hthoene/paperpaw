use super::{Error, wire};

/// A secret name and its owner. Empty owner means the calling plugin.
#[derive(Clone, Copy, Debug)]
pub struct SecretName<'a> {
    pub(super) plugin: &'a str,
    pub(super) name: &'a str,
}
impl<'a> SecretName<'a> {
    /// Address one of the caller's own secrets.
    ///
    /// # Errors
    /// Invalid for an empty name or prohibited controls; NoSpace above 64 bytes.
    /// Tab, LF and CR remain valid SSID characters.
    pub fn new(name: &'a str) -> Result<Self, Error> {
        if name.is_empty() {
            return Err(Error::Invalid);
        }
        label(name, wire::SERVICE_SEC_NAME_NAME_MAX_BYTES)?;
        Ok(Self { plugin: "", name })
    }
    /// Address another owner for editor keep/remove operations, never get.
    /// An empty owner restores the caller's own namespace.
    ///
    /// # Errors
    /// Invalid for a malformed plugin id; NoSpace above 64 bytes.
    pub fn owner(mut self, plugin: &'a str) -> Result<Self, Error> {
        if !plugin.is_empty() {
            plugin_id(plugin)?;
        }
        self.plugin = plugin;
        Ok(self)
    }
    /// The unprefixed secret name, compared byte for byte.
    #[must_use]
    pub const fn name(self) -> &'a str {
        self.name
    }
    /// Explicit owner for editor operations, or empty for the caller.
    #[must_use]
    pub const fn plugin(self) -> &'a str {
        self.plugin
    }
}

/// A borrowed value to save. Debug always redacts the value.
#[derive(Clone, Copy)]
pub struct SecretWrite<'a> {
    pub(super) name: SecretName<'a>,
    pub(super) label: &'a str,
    pub(super) value: &'a str,
    pub(super) flags: u32,
}
impl core::fmt::Debug for SecretWrite<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SecretWrite")
            .field("name", &self.name)
            .field("label", &self.label)
            .field("flags", &self.flags)
            .field("value", &"[redacted]")
            .finish()
    }
}
impl<'a> SecretWrite<'a> {
    /// Save a complete UTF-8 value. Empty is a value; it never removes a secret.
    /// Empty label keeps an existing label. By default only the owner may change
    /// the value; an editor can always remove it.
    ///
    /// # Errors
    /// NoSpace above the generated 1,024-byte value cap.
    pub fn new(name: SecretName<'a>, value: &'a str) -> Result<Self, Error> {
        if value.len() > wire::SERVICE_SEC_KEEP_VALUE_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        Ok(Self {
            name,
            label: "",
            value,
            flags: 0,
        })
    }
    /// Set the display label, or preserve an existing label with empty text.
    ///
    /// # Errors
    /// Invalid for prohibited controls; NoSpace above 64 bytes.
    pub fn label(mut self, text: &'a str) -> Result<Self, Error> {
        label(text, wire::SERVICE_SEC_KEEP_LABEL_MAX_BYTES)?;
        self.label = text;
        Ok(self)
    }
    /// Permit the user to change this value in the settings editor.
    /// An editor replacing another owner's value must preserve that owner's flag.
    #[must_use]
    pub const fn changeable(mut self) -> Self {
        self.flags = 1;
        self
    }
    /// The owner/name being changed.
    #[must_use]
    pub const fn target(self) -> SecretName<'a> {
        self.name
    }
    /// The supplied display label, empty to preserve an existing label.
    #[must_use]
    pub const fn display_label(self) -> &'a str {
        self.label
    }
    /// Explicitly read the borrowed value. Debug does not expose it.
    #[must_use]
    pub const fn value(self) -> &'a str {
        self.value
    }
    /// Whether this request marks the value as user-changeable.
    #[must_use]
    pub const fn is_changeable(self) -> bool {
        self.flags & 1 != 0
    }
}

/// The visible secret rows to list and the row cursor to resume from.
#[derive(Clone, Copy, Debug, Default)]
pub struct SecretList {
    pub(super) flags: u32,
    pub(super) cursor: u32,
}
impl SecretList {
    /// List the caller's own names and labels, beginning with the first row.
    #[must_use]
    pub const fn own() -> Self {
        Self {
            flags: 0,
            cursor: 0,
        }
    }
    /// List all plugins' names and labels. The host requires `uses.secrets`.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            flags: 1,
            cursor: 0,
        }
    }
    /// Resume at a row cursor returned by a previous page.
    #[must_use]
    pub const fn from(mut self, cursor: u32) -> Self {
        self.cursor = cursor;
        self
    }
    /// Whether every owner's rows are requested.
    #[must_use]
    pub const fn includes_all(self) -> bool {
        self.flags & 1 != 0
    }
    /// The row cursor, zero for a first page.
    #[must_use]
    pub const fn cursor(self) -> u32 {
        self.cursor
    }
}

/// A secret value borrowed from caller-owned output, with redacted Debug.
///
/// ```compile_fail
/// use paperpaw_sdk::native::{Host, secrets::{Secrets, SecretValue}};
/// fn escape<'a>(secrets: &Secrets, host: &Host<'_>, input: &mut [u8]) -> SecretValue<'a> {
///     let mut output = [0; 1280];
///     secrets.get(host, "token", input, &mut output).unwrap()
/// }
/// ```
#[derive(Clone, Copy, Debug)]
pub struct SecretValue<'a>(pub(super) wire::ServiceSecValue<'a>);
impl<'a> SecretValue<'a> {
    /// Explicitly access the value returned to its owner.
    #[must_use]
    pub const fn value(self) -> &'a str {
        self.0.value
    }
    /// Label supplied by the owner.
    #[must_use]
    pub const fn label(self) -> &'a str {
        self.0.label
    }
    /// Whether the editor may replace the value.
    #[must_use]
    pub const fn changeable(self) -> bool {
        self.0.flags & 1 != 0
    }
}

pub(super) fn label(text: &str, cap: usize) -> Result<(), Error> {
    if text.len() > cap {
        return Err(Error::NoSpace);
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
pub(super) fn plugin_id(plugin: &str) -> Result<(), Error> {
    if plugin.len() > wire::SERVICE_SEC_NAME_PLUGIN_MAX_BYTES {
        return Err(Error::NoSpace);
    }
    if plugin.is_empty() || plugin.contains('.') || plugin.chars().any(char::is_control) {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// Device randomness borrowed from the caller's output. Debug hides the bytes.
#[derive(Clone, Copy)]
pub struct RandomBytes<'a>(pub(super) wire::ServiceSecBytes<'a>);
impl core::fmt::Debug for RandomBytes<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RandomBytes")
            .field("strong", &self.strong())
            .field("data", &"[redacted]")
            .finish()
    }
}
impl<'a> RandomBytes<'a> {
    /// The exact number of requested random bytes, with no allocation or copying.
    #[must_use]
    pub const fn bytes(self) -> &'a [u8] {
        self.0.data
    }
    /// Whether the device reports a fully random source for this draw.
    #[must_use]
    pub const fn strong(self) -> bool {
        self.0.flags & 1 != 0
    }
}
