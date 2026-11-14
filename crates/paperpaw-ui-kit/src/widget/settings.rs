//! A widget's settings read at draw time: `<widget>.<key>` through the host's
//! settings slot, which resolves a placement id (`widgets.day@2.style`) and
//! falls back to the setting itself. Nothing is retained between callbacks.
use alloc::string::String;
use core::fmt::Write;
use paperpaw_sdk::native::{
    Error, Host,
    registry::{Registry, RegistryValueSource},
    settings::{self, SettingValue},
};

/// The most bytes of a setting id.
const KEY_MAX: usize = 192;
/// The wire room of a choice, a switch or a number (their values are short).
const SHORT: usize = 192;

/// A setting id built on the stack.
#[derive(Debug)]
pub struct Key {
    bytes: [u8; KEY_MAX],
    len: usize,
}

impl Key {
    /// `<widget>.<key>`.
    ///
    /// # Errors
    /// [`Error::NoSpace`] when the id does not fit.
    pub fn join(widget: &str, key: &str) -> Result<Self, Error> {
        let mut out = Self {
            bytes: [0; KEY_MAX],
            len: 0,
        };
        out.push(widget)?;
        out.push(".")?;
        out.push(key)?;
        Ok(out)
    }

    fn push(&mut self, text: &str) -> Result<(), Error> {
        let end = self.len.checked_add(text.len()).ok_or(Error::NoSpace)?;
        let slot = self.bytes.get_mut(self.len..end).ok_or(Error::NoSpace)?;
        slot.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }

    /// The id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.bytes
            .get(..self.len)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }
}

/// The widget's id without its placement: `widgets.day@2` is `widgets.day`.
#[must_use]
pub fn base(widget: &str) -> &str {
    widget.split('@').next().unwrap_or(widget)
}

/// Reads of one widget's settings in one callback.
#[derive(Debug, Clone, Copy)]
pub struct Settings<'a, 'cb> {
    host: &'a Host<'cb>,
    widget: &'a str,
}

impl<'a, 'cb> Settings<'a, 'cb> {
    /// Reads for `widget` (its id with the placement, as the host names it).
    #[must_use]
    pub const fn new(host: &'a Host<'cb>, widget: &'a str) -> Self {
        Self { host, widget }
    }

    /// The widget these reads are for.
    #[must_use]
    pub const fn widget(&self) -> &'a str {
        self.widget
    }

    /// Runs `f` on the text of `<widget>.<key>`; empty where there is none.
    ///
    /// # Errors
    /// The host's refusal; a value too long for the short room.
    pub fn with_text<R>(&self, key: &str, f: impl FnOnce(&str) -> R) -> Result<R, Error> {
        let id = Key::join(self.widget, key)?;
        text_of(self.host, id.as_str(), f)
    }

    /// Whether the switch `<widget>.<key>` is on.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn flag(&self, key: &str) -> Result<bool, Error> {
        let id = Key::join(self.widget, key)?;
        let mut bytes = [0; SHORT];
        Ok(matches!(
            settings::read_setting(self.host, id.as_str(), &mut bytes)?,
            SettingValue::Bool(true)
        ))
    }

    /// The number `<widget>.<key>`; `default` where there is none.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn int(&self, key: &str, default: i64) -> Result<i64, Error> {
        let id = Key::join(self.widget, key)?;
        let mut bytes = [0; SHORT];
        Ok(
            match settings::read_setting(self.host, id.as_str(), &mut bytes)? {
                SettingValue::Int(value) => value,
                _ => default,
            },
        )
    }

    /// An owned copy of the text `<widget>.<key>`, at most `limit` bytes
    /// (longer text is refused, not cut).
    ///
    /// # Errors
    /// [`Error::NoMemory`] or [`Error::NoSpace`], or the host's refusal.
    pub fn string(&self, key: &str, limit: usize) -> Result<String, Error> {
        let id = Key::join(self.widget, key)?;
        let mut bytes = alloc::vec::Vec::new();
        bytes
            .try_reserve_exact(limit.saturating_add(64))
            .map_err(|_| Error::NoMemory)?;
        bytes.resize(limit.saturating_add(64), 0);
        match settings::read_setting(self.host, id.as_str(), &mut bytes)? {
            SettingValue::Text(text) => crate::text::copy(text),
            _ => Ok(String::new()),
        }
    }

    /// Runs `f` on the text of `<widget>.<key>` as the reader chose it: the
    /// setting for all widgets (`all`) rules while it is neither empty nor
    /// `auto` and the reader did not choose a value for this widget's own.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn chosen<R>(
        &self,
        key: &str,
        all: &str,
        auto: &str,
        f: impl FnOnce(&str) -> R,
    ) -> Result<R, Error> {
        let mut every = [0; SHORT];
        if let SettingValue::Text(value) = settings::read_setting(self.host, all, &mut every)?
            && !value.is_empty()
            && value != auto
            && !self.is_chosen(key)?
        {
            return Ok(f(value));
        }
        self.with_text(key, f)
    }

    /// Whether the reader chose a value for `<widget>.<key>` (it is not at
    /// its default): kept by the user, a profile or a preset, or set by this
    /// widget's placement.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn is_chosen(&self, key: &str) -> Result<bool, Error> {
        let registry = Registry::open(self.host)?;
        let found = self.chosen_in(&registry, key);
        let released = registry.release(self.host);
        let found = found?;
        released?;
        Ok(found)
    }

    fn chosen_in(&self, registry: &Registry, key: &str) -> Result<bool, Error> {
        let mut input = crate::mem::buffer(192)?;
        // The op's `max_reply` (`abi/registry.md` 2.2): the host refuses a
        // smaller reply buffer whatever the reply's size.
        let mut out = crate::mem::buffer(4096)?;
        let id = Key::join(base(self.widget), key)?;
        match registry.setting(self.host, id.as_str(), &mut input, &mut out) {
            Ok(detail) => {
                if detail.setting().source() != RegistryValueSource::Default {
                    return Ok(true);
                }
            }
            Err(Error::NotFound) => return Ok(false),
            Err(error) => return Err(error),
        }
        if self.widget == base(self.widget) {
            return Ok(false);
        }
        self.placed(registry, key)
    }

    /// Whether the placement sets `key` itself.
    fn placed(&self, registry: &Registry, key: &str) -> Result<bool, Error> {
        use paperpaw_sdk::native::registry::{PageCursor, ValuesQuery};
        let id = Key::join(self.widget, key)?;
        let mut input = crate::mem::buffer(192)?;
        let mut out = crate::mem::buffer(4096)?;
        let mut cursor = PageCursor::start();
        loop {
            let page = registry.values(
                self.host,
                ValuesQuery::Placement(self.widget),
                cursor,
                &mut input,
                &mut out,
            )?;
            if page.rows().any(|row| row.key() == id.as_str()) {
                return Ok(true);
            }
            let Some(next) = page.end().next_cursor() else {
                return Ok(false);
            };
            cursor = next;
        }
    }
}

/// Runs `f` on the text of the setting `key`; empty where there is none.
///
/// # Errors
/// The host's refusal.
pub fn text_of<R>(host: &Host<'_>, key: &str, f: impl FnOnce(&str) -> R) -> Result<R, Error> {
    let mut bytes = [0; SHORT];
    Ok(match settings::read_setting(host, key, &mut bytes)? {
        SettingValue::Text(text) => f(text),
        _ => f(""),
    })
}

/// A decimal number as text on the stack.
#[must_use]
pub fn fmt_u32(value: u32, out: &mut [u8; 10]) -> &str {
    let mut end = out.len();
    let mut rest = value;
    loop {
        end -= 1;
        out[end] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    core::str::from_utf8(&out[end..]).unwrap_or("0")
}

/// `write!` into a stack buffer that refuses what does not fit.
#[derive(Debug)]
pub struct Stack<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Default for Stack<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Stack<N> {
    /// An empty buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    /// `args` written into a new buffer; refused where they do not fit.
    ///
    /// # Errors
    /// [`Error::NoSpace`].
    pub fn from_args(args: core::fmt::Arguments<'_>) -> Result<Self, Error> {
        let mut out = Self::new();
        core::fmt::Write::write_fmt(&mut out, args).map_err(|_| Error::NoSpace)?;
        Ok(out)
    }

    /// What was written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.bytes
            .get(..self.len)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }
}

impl<const N: usize> Write for Stack<N> {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(core::fmt::Error)?;
        let slot = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        slot.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_joins_widget_and_setting() {
        let key = Key::join("widgets.day@2", "style").unwrap();
        assert_eq!(key.as_str(), "widgets.day@2.style");
        assert_eq!(base("widgets.day@2"), "widgets.day");
        assert_eq!(base("widgets.day"), "widgets.day");
    }

    #[test]
    fn a_key_that_does_not_fit_is_refused_not_cut() {
        let long = "w".repeat(KEY_MAX);
        assert_eq!(Key::join(&long, "x").unwrap_err(), Error::NoSpace);
        assert!(Key::join(&"w".repeat(KEY_MAX - 3), "x").is_ok());
    }

    #[test]
    fn the_stack_buffer_refuses_what_does_not_fit() {
        let mut out = Stack::<4>::new();
        assert!(write!(out, "{}", 1234).is_ok());
        assert_eq!(out.as_str(), "1234");
        assert!(write!(out, "5").is_err());
        assert_eq!(out.as_str(), "1234");
    }

    #[test]
    fn numbers_are_written_in_decimal() {
        let mut bytes = [0; 10];
        assert_eq!(fmt_u32(0, &mut bytes), "0");
        assert_eq!(fmt_u32(4_294_967_295, &mut bytes), "4294967295");
    }
}
