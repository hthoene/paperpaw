//! Typed UIX declarations share the bounded, latched declaration transaction.
use super::{Declarations, DeclareError, records_wire as wire};
use crate::native::{
    Error,
    ui::{HintInput, navigation::full_id},
};

macro_rules! declaration {
    ($name:ident,$row:ident,$variant:ident,$doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug)]
        pub struct $name<'a>(wire::$row<'a>);
        impl<'a> $name<'a> {
            fn checked(row: wire::$row<'a>) -> Result<Self, Error> {
                wire::record_framed_len_of(&row).map_err(Error::from_status)?;
                validate(wire::Record::$variant(row))?;
                Ok(Self(row))
            }
            fn record(self) -> wire::$row<'a> {
                self.0
            }
        }
    };
}
declaration!(
    InputDecl,
    InputSpec,
    InputSpec,
    "A declared plugin input or gesture, with no recognizer callback."
);
declaration!(
    HelpHintDecl,
    HelpHint,
    HelpHint,
    "A help hint for an own or foreign screen."
);
declaration!(
    WidgetDecl,
    WidgetSpec,
    WidgetSpec,
    "A plugin-owned widget and its callback token."
);
declaration!(
    WidgetPlaceDecl,
    WidgetPlace,
    WidgetPlace,
    "A place whose settings list widgets."
);
declaration!(
    StatusItemDecl,
    StatusItem,
    StatusItem,
    "A plugin-owned status value and its callback token."
);
declaration!(
    StatusLineDecl,
    StatusLine,
    StatusLine,
    "A plugin-owned drawn status line."
);
declaration!(
    StatusPlaceDecl,
    StatusPlace,
    StatusPlace,
    "Settings listing the left, center and right status items."
);

/// A declared input's kind; its host-stamped full id includes the plugin owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputKind {
    /// An input named input.plugin.name.
    Input,
    /// A gesture named gesture.plugin.name.
    Gesture,
}
/// How a widget place arranges its instances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WidgetLayout {
    /// A grid.
    Grid,
    /// A vertical stack.
    Stack,
}
impl<'a> InputDecl<'a> {
    /// Declare a relative name and title key.
    ///
    /// # Errors
    /// Invalid for name syntax; NoSpace above field caps.
    pub fn new(name: &'a str, title: &'a str, kind: InputKind) -> Result<Self, Error> {
        let kind = match kind {
            InputKind::Input => 0,
            InputKind::Gesture => 1,
        };
        Self::checked(wire::InputSpec {
            name,
            title,
            kind,
            need: 0,
            capability: 0,
            flags: 0,
        })
    }
    /// Offer the input only when the device meets this requirement.
    ///
    /// # Errors
    /// Invalid for an empty Has capability set.
    pub fn need(mut self, need: crate::MenuNeed) -> Result<Self, Error> {
        let (need, capability) = super::menus::need_bits(need).map_err(|_| Error::Invalid)?;
        self.0.need = need;
        self.0.capability = capability;
        self.0.flags |= 1;
        Self::checked(self.0)
    }
}
impl<'a> HelpHintDecl<'a> {
    /// Declare a text key for a full screen id, with no rectangular zone.
    ///
    /// # Errors
    /// Invalid for syntax or empty text; NoSpace above field caps.
    pub fn new(screen: &'a str, text: &'a str, input: HintInput) -> Result<Self, Error> {
        let input = match input {
            HintInput::Any => 0,
            HintInput::Touch => 1,
            HintInput::Buttons => 2,
        };
        Self::checked(wire::HelpHint {
            screen,
            input,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            text,
        })
    }
    /// Restrict the hint to a zone in thousandths of the screen.
    ///
    /// # Errors
    /// Invalid for a zone outside the screen.
    pub fn zone(mut self, zone: paperpaw_platform::Rect) -> Result<Self, Error> {
        self.0.x = u32::from(zone.x);
        self.0.y = u32::from(zone.y);
        self.0.width = u32::from(zone.width);
        self.0.height = u32::from(zone.height);
        Self::checked(self.0)
    }
}
impl<'a> WidgetDecl<'a> {
    /// Declare a widget with no live or private flags and automatic cell size.
    ///
    /// # Errors
    /// Invalid for empty names or keys; NoSpace above field caps.
    pub fn new(
        name: &'a str,
        title: &'a str,
        category: &'a str,
        token: u32,
    ) -> Result<Self, Error> {
        Self::checked(wire::WidgetSpec {
            name,
            title,
            category,
            previews: "",
            cells: 0,
            token,
            flags: 0,
        })
    }
    /// Refresh this widget on each minute tick as well as on events.
    #[must_use]
    pub const fn live(mut self) -> Self {
        self.0.flags |= 1;
        self
    }
    /// Keep private content hidden from other plugins while a guard is active.
    #[must_use]
    pub const fn private(mut self) -> Self {
        self.0.flags |= 2;
        self
    }
    /// Declare grid columns and rows; zero requests the place's natural size.
    #[must_use]
    pub const fn cells(mut self, columns: u8, rows: u8) -> Self {
        self.0.cells = columns as u32 | (rows as u32) << 8;
        self
    }
    /// Preview this full settings group id.
    ///
    /// # Errors
    /// Invalid for malformed ids; NoSpace above its field cap.
    pub fn previews(mut self, group: &'a str) -> Result<Self, Error> {
        self.0.previews = group;
        Self::checked(self.0)
    }
}
impl<'a> WidgetPlaceDecl<'a> {
    /// Declare a place using newline-separated setting ids, at most eight.
    ///
    /// # Errors
    /// Invalid for key syntax; NoSpace above list or field caps.
    pub fn new(
        name: &'a str,
        title: &'a str,
        list_keys: &'a str,
        layout: WidgetLayout,
    ) -> Result<Self, Error> {
        let layout = match layout {
            WidgetLayout::Grid => 0,
            WidgetLayout::Stack => 1,
        };
        Self::checked(wire::WidgetPlace {
            name,
            title,
            list_keys,
            layout,
            shown_if_key: "",
            shown_if_value: "",
        })
    }
    /// Show this place while a setting matches its configuration text.
    ///
    /// # Errors
    /// Invalid for malformed or unmatched condition fields; NoSpace above caps.
    pub fn shown_if(mut self, key: &'a str, value: &'a str) -> Result<Self, Error> {
        self.0.shown_if_key = key;
        self.0.shown_if_value = value;
        Self::checked(self.0)
    }
}
impl<'a> StatusItemDecl<'a> {
    /// Declare a status value with default priority and width.
    ///
    /// # Errors
    /// Invalid for syntax or empty title; NoSpace above field caps.
    pub fn new(name: &'a str, title: &'a str, token: u32) -> Result<Self, Error> {
        Self::checked(wire::StatusItem {
            name,
            title,
            tap: "",
            priority: 0,
            min_width: 0,
            token,
            flags: 0,
        })
    }
    /// Run this relative action name on a tap.
    ///
    /// # Errors
    /// Invalid for malformed action names; NoSpace above its field cap.
    pub fn tap(mut self, action: &'a str) -> Result<Self, Error> {
        self.0.tap = action;
        Self::checked(self.0)
    }
    /// Higher-priority items stay visible longer.
    #[must_use]
    pub const fn priority(mut self, priority: u8) -> Self {
        self.0.priority = priority as u32;
        self
    }
    /// Minimum width in thousandths of the text size.
    #[must_use]
    pub const fn min_width(mut self, width: u32) -> Self {
        self.0.min_width = width;
        self
    }
    /// Mark its value as private while another plugin guards the device.
    #[must_use]
    pub const fn private(mut self) -> Self {
        self.0.flags |= 2;
        self
    }
}
impl<'a> StatusLineDecl<'a> {
    /// Declare a drawn status line; height is in thousandths of the text size.
    ///
    /// # Errors
    /// Invalid for syntax or empty title; NoSpace above field caps.
    pub fn new(name: &'a str, title: &'a str, height: u32, token: u32) -> Result<Self, Error> {
        Self::checked(wire::StatusLine {
            name,
            title,
            height,
            token,
            flags: 0,
        })
    }
    /// Hide this private line from other plugins while a guard is active.
    #[must_use]
    pub const fn private(mut self) -> Self {
        self.0.flags |= 2;
        self
    }
}
impl<'a> StatusPlaceDecl<'a> {
    /// Declare the settings listing the three parts of a status line.
    ///
    /// # Errors
    /// Invalid for syntax or empty title; NoSpace above field caps.
    pub fn new(
        name: &'a str,
        title: &'a str,
        left: &'a str,
        center: &'a str,
        right: &'a str,
    ) -> Result<Self, Error> {
        Self::checked(wire::StatusPlace {
            name,
            title,
            left,
            center,
            right,
        })
    }
}

macro_rules! submit {
    ($method:ident,$ty:ident,$doc:literal) => {
        #[doc=$doc]
        ///
        /// # Errors
        /// Returns encoding, allocation or latched sink refusals.
        pub fn $method(&mut self, value: $ty<'_>) -> Result<&mut Self, DeclareError> {
            self.accept_record(Ok(value.record()))
        }
    };
}
impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    submit!(input, InputDecl, "Declare a plugin input or gesture.");
    submit!(help_hint, HelpHintDecl, "Declare a help hint for a screen.");
    submit!(widget, WidgetDecl, "Declare a widget and its token.");
    submit!(
        widget_place,
        WidgetPlaceDecl,
        "Declare where widgets are listed."
    );
    submit!(
        status_item,
        StatusItemDecl,
        "Declare a status value and its token."
    );
    submit!(
        status_line,
        StatusLineDecl,
        "Declare a drawn status line and its token."
    );
    submit!(
        status_place,
        StatusPlaceDecl,
        "Declare settings listing a status line's items."
    );
}

fn relative(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || value.split('.').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn filled(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        Err(Error::Invalid)
    } else {
        Ok(())
    }
}
fn optional_id(value: &str) -> Result<(), Error> {
    if value.is_empty() {
        Ok(())
    } else {
        full_id(value)
    }
}
/// The field rules of a declaration; matching the enum links no record codec.
fn validate(record: wire::Record<'_>) -> Result<(), Error> {
    match record {
        wire::Record::InputSpec(r) => {
            relative(r.name)?;
            filled(r.title)
        }
        wire::Record::HelpHint(r) => {
            full_id(r.screen)?;
            filled(r.text)?;
            if r.x.checked_add(r.width).is_none_or(|n| n > 1000)
                || r.y.checked_add(r.height).is_none_or(|n| n > 1000)
            {
                return Err(Error::Invalid);
            }
            Ok(())
        }
        wire::Record::WidgetSpec(r) => {
            relative(r.name)?;
            filled(r.title)?;
            filled(r.category)?;
            optional_id(r.previews)
        }
        wire::Record::WidgetPlace(r) => {
            relative(r.name)?;
            filled(r.title)?;
            if !r.list_keys.is_empty() {
                if r.list_keys.split('\n').count() > wire::WIDGET_PLACE_LIST_KEYS_MAX_ITEMS {
                    return Err(Error::NoSpace);
                }
                for key in r.list_keys.split('\n') {
                    full_id(key)?;
                }
            }
            if r.shown_if_key.is_empty() != r.shown_if_value.is_empty() {
                return Err(Error::Invalid);
            }
            optional_id(r.shown_if_key)
        }
        wire::Record::StatusItem(r) => {
            relative(r.name)?;
            filled(r.title)?;
            if !r.tap.is_empty() {
                relative(r.tap)?;
            }
            Ok(())
        }
        wire::Record::StatusLine(r) => {
            relative(r.name)?;
            filled(r.title)
        }
        wire::Record::StatusPlace(r) => {
            relative(r.name)?;
            filled(r.title)?;
            optional_id(r.left)?;
            optional_id(r.center)?;
            optional_id(r.right)
        }
        _ => Err(Error::Invalid),
    }
}
