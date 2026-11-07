//! Screen parameters and navigation through the typed UI service.
use super::{Error, Host, copy};
use alloc::string::String;
use paperpaw_sdk::native::{
    commands::CommandSender,
    ui::{HandoverId, Open, Param, Ui},
};

/// Per-instance route. This owns its parameters after the callback returns.
#[derive(Debug, PartialEq, Eq)]
pub enum ScreenState {
    /// Menu inventory.
    Menus,
    /// Current entries of a menu.
    Entries { menu: String },
    /// Move/remove controls for one entry.
    Entry { menu: String, index: usize },
    /// Candidates to add to a menu.
    Add { menu: String },
}
impl ScreenState {
    /// Decode a screen-opened handover, including reopens with new parameters.
    ///
    /// # Errors
    /// Returns Invalid for a foreign screen, or allocation/host errors.
    /// A caller replaces its old instance state only after this succeeds.
    pub fn read(
        ui: &Ui,
        host: &Host<'_>,
        screen: &str,
        handover: Option<HandoverId>,
        input: &mut [u8],
        output: &mut [u8],
    ) -> Result<Self, Error> {
        if !matches!(
            screen,
            "menu-editor.menus" | "menu-editor.entries" | "menu-editor.entry" | "menu-editor.add"
        ) {
            return Err(Error::Invalid);
        }
        let mut menu = String::new();
        let mut index = None;
        if let Some(handover) = handover {
            for param in ui.params(host, handover, input, output)? {
                let param = param?;
                match param.key() {
                    "menu" => menu = copy(param.value())?,
                    "index" => index = Some(parse_index(param.value())?),
                    _ => {}
                }
            }
        }
        let wants_menu = screen != "menu-editor.menus";
        if (wants_menu && menu.is_empty()) || (screen == "menu-editor.entry") != index.is_some() {
            return Err(Error::Invalid);
        }
        Self::from_parts(screen, menu, index.unwrap_or(0))
    }
    fn from_parts(screen: &str, menu: String, index: usize) -> Result<Self, Error> {
        match screen {
            "menu-editor.menus" => Ok(Self::Menus),
            "menu-editor.entries" => Ok(Self::Entries { menu }),
            "menu-editor.entry" => Ok(Self::Entry { menu, index }),
            "menu-editor.add" => Ok(Self::Add { menu }),
            _ => Err(Error::Invalid),
        }
    }
    /// Stage a new screen with lossless menu/index parameters.
    ///
    /// # Errors
    /// Propagates parameter limits and the host's handover/navigation refusal.
    pub fn open(
        &self,
        ui: &Ui,
        host: &Host<'_>,
        input: &mut [u8],
        output: &mut [u8],
    ) -> Result<(), Error> {
        let (target, menu, index) = match self {
            Self::Menus => {
                return CommandSender::new(host, input).open(Open::screen("menu-editor.menus")?);
            }
            Self::Entries { menu } => ("menu-editor.entries", menu, None),
            Self::Entry { menu, index } => ("menu-editor.entry", menu, Some(*index)),
            Self::Add { menu } => ("menu-editor.add", menu, None),
        };
        let menu = Param::new("menu", menu)?;
        let handover = if let Some(index) = index {
            let mut digits = [0; 20];
            let index = Param::new("index", paperpaw_ui_kit::text::decimal(index, &mut digits))?;
            ui.handover(host, &[menu, index], input, output)?
        } else {
            ui.handover(host, &[menu], input, output)?
        };
        CommandSender::new(host, input).open(Open::screen(target)?.handover(handover))
    }
}

/// A decimal entry index: digits only, no sign, no overflow.
fn parse_index(text: &str) -> Result<usize, Error> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Invalid);
    }
    text.parse().map_err(|_| Error::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routes_preserve_the_menu_and_index() {
        let state =
            ScreenState::from_parts("menu-editor.entry", String::from("core.settings-pages"), 42);
        assert_eq!(
            state,
            Ok(ScreenState::Entry {
                menu: String::from("core.settings-pages"),
                index: 42
            })
        );
        assert_eq!(
            ScreenState::from_parts("other.entry", String::new(), 0),
            Err(Error::Invalid)
        );
    }
    #[test]
    fn indices_roundtrip_without_allocating_text() {
        for value in [0, 1, 9, 10, 99, 100, usize::MAX] {
            let mut buffer = [0; 20];
            assert_eq!(
                paperpaw_ui_kit::text::decimal(value, &mut buffer).parse(),
                Ok(value)
            );
        }
    }
    #[test]
    fn handover_indices_are_digits_that_fit_or_are_refused() {
        assert_eq!(parse_index("0"), Ok(0));
        assert_eq!(parse_index("42"), Ok(42));
        let max = alloc::format!("{}", usize::MAX);
        assert_eq!(parse_index(&max), Ok(usize::MAX));
        let over = alloc::format!("{max}0");
        for bad in ["", "-1", "+1", " 1", "1 ", "1.5", "x", over.as_str(), "٣"] {
            assert_eq!(parse_index(bad), Err(Error::Invalid), "{bad:?}");
        }
    }
}
