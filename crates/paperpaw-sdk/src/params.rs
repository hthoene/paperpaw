use alloc::borrow::Cow;
use alloc::collections::TryReserveError;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::any::Any;
use core::fmt;

/// Named string parameters passed with commands and events, e.g. the path
/// of the book a reader screen should open.
///
/// Parameters are plain strings so they can cross a message boundary
/// unchanged. An event may also carry its data as it is, unwritten (see
/// [`Params::carrying`]): data too large to copy, such as an HTTP
/// answer's body.
#[derive(Clone, Default)]
pub struct Params {
    values: Vec<(Cow<'static, str>, String)>,
    carried: Option<Rc<dyn Any>>,
}

/// As the named strings are, in any order: what is carried is not
/// compared.
impl PartialEq for Params {
    fn eq(&self, other: &Self) -> bool {
        self.values.len() == other.values.len()
            && self
                .values
                .iter()
                .all(|(key, value)| other.get(key) == Some(value))
    }
}

impl Eq for Params {}

impl fmt::Debug for Params {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Params").field(&self.values).finish()
    }
}

impl Params {
    pub const fn new() -> Self {
        Self {
            values: Vec::new(),
            carried: None,
        }
    }

    /// The same, carrying `value` as it is, unwritten: an event's data
    /// that a hearer of its type reads without a copy (see
    /// [`EventData::into_params`](crate::event::EventData::into_params)).
    #[must_use]
    pub fn carrying<T: Any>(mut self, value: T) -> Self {
        self.carried = Some(Rc::new(value));
        self
    }

    /// The value carried, if one of type `T` is.
    pub fn carried<T: Any>(&self) -> Option<&T> {
        self.carried.as_deref()?.downcast_ref()
    }

    /// Adds or replaces a parameter.
    pub fn with(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        self.set(key, value);
        self
    }

    pub fn set(&mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        match self.values.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.values.push((key, value)),
        }
    }

    /// [`Self::set`] that reports a memory shortage instead of aborting: the value is copied
    /// from `value` with the allocation checked. The key is as the caller made it (a
    /// `&'static str` allocates nothing). On `Err` the parameters are as they were, a
    /// replaced value included.
    pub fn try_set(
        &mut self,
        key: impl Into<Cow<'static, str>>,
        value: &str,
    ) -> Result<(), TryReserveError> {
        let key = key.into();
        let copy = try_copy(value)?;
        match self.values.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = copy,
            None => {
                self.values.try_reserve(1)?;
                self.values.push((key, copy));
            }
        }
        Ok(())
    }

    /// A copy made with the allocation checked, as [`Clone`] makes it otherwise. What is
    /// carried is shared, as a clone shares it.
    pub fn try_clone(&self) -> Result<Self, TryReserveError> {
        let mut values = Vec::new();
        values.try_reserve_exact(self.values.len())?;
        for (key, value) in &self.values {
            let key = match key {
                Cow::Borrowed(key) => Cow::Borrowed(*key),
                Cow::Owned(key) => Cow::Owned(try_copy(key)?),
            };
            values.push((key, try_copy(value)?));
        }
        Ok(Self {
            values,
            carried: self.carried.clone(),
        })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(k, v)| (k.as_ref(), v.as_str()))
    }
}

fn try_copy(text: &str) -> Result<String, TryReserveError> {
    let mut copy = String::new();
    copy.try_reserve_exact(text.len())?;
    copy.push_str(text);
    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_and_replace() {
        let params = Params::new().with("path", "a.txt").with("path", "b.txt");
        assert_eq!(params.get("path"), Some("b.txt"));
        assert_eq!(params.get("other"), None);
        assert_eq!(params.iter().count(), 1);
    }

    #[test]
    fn try_set_adds_and_replaces_like_set() {
        let mut params = Params::new();
        params.try_set("path", "a.txt").unwrap();
        params.try_set(String::from("owned"), "x").unwrap();
        params.try_set("path", "b.txt").unwrap();
        assert_eq!(params.get("path"), Some("b.txt"));
        assert_eq!(params.get("owned"), Some("x"));
        assert_eq!(params.iter().count(), 2);
        params.try_set("empty", "").unwrap();
        assert_eq!(params.get("empty"), Some(""));
    }

    #[test]
    fn try_clone_copies_values_and_shares_what_is_carried() {
        let params = Params::new()
            .with("path", "a.txt")
            .with(String::from("owned"), "b")
            .carrying(7_u32);
        let copy = params.try_clone().unwrap();
        assert_eq!(copy, params);
        assert_eq!(copy.carried::<u32>(), Some(&7));
        assert_eq!(Params::new().try_clone().unwrap(), Params::new());
    }
}
