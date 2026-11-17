//! How much room a screen has: its class by width ([`ScreenClass`]), for
//! layouts that adapt to phones, tablets and a device on its side.

use crate::event::ParamValue;
use alloc::string::String;

/// How much room a part of the screen has, by its width. The X4 and other
/// small readers are compact in both orientations; a phone on its side
/// and a tablet are expanded. What each means is a rule for layouts, not
/// enforced:
///
/// - compact: one column, as on the X4;
/// - medium: a readable column in the middle (`readable`),
///   grids with more columns;
/// - expanded: also a list and the item chosen beside each other
///   ([`Pane::DETAIL`](crate::Pane::DETAIL)).
///
/// Ordered: `Compact < Medium < Expanded`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ScreenClass {
    Compact,
    Medium,
    Expanded,
}

impl ScreenClass {
    /// Its name, as events carry it: `compact`, `medium` or `expanded`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Medium => "medium",
            Self::Expanded => "expanded",
        }
    }
}

impl ParamValue for ScreenClass {
    fn to_param(&self) -> String {
        String::from(self.name())
    }

    /// A class of a later kernel this one does not know is the widest it
    /// knows (classes come narrowest first), so that an event carrying it
    /// still comes.
    fn from_param(text: &str) -> Option<Self> {
        match text {
            "" => None,
            "compact" => Some(Self::Compact),
            "medium" => Some(Self::Medium),
            _ => Some(Self::Expanded),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_go_by_their_names_in_events() {
        for class in [
            ScreenClass::Compact,
            ScreenClass::Medium,
            ScreenClass::Expanded,
        ] {
            assert_eq!(ScreenClass::from_param(&class.to_param()), Some(class));
        }
        // One of a later kernel: the widest known.
        assert_eq!(ScreenClass::from_param("huge"), Some(ScreenClass::Expanded));
        assert_eq!(ScreenClass::from_param(""), None);
    }
}
