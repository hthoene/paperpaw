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
        let mut index = 0;
        if let Some(handover) = handover {
            for param in ui.params(host, handover, input, output)? {
                let param = param?;
                match param.key() {
                    "menu" => menu = copy(param.value())?,
                    "index" => index = param.value().parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        Self::from_parts(screen, menu, index)
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
            let index = Param::new("index", decimal(index, &mut digits))?;
            ui.handover(host, &[menu, index], input, output)?
        } else {
            ui.handover(host, &[menu], input, output)?
        };
        CommandSender::new(host, input).open(Open::screen(target)?.handover(handover))
    }
}

// Parameter text only; wire framing belongs exclusively to the SDK.
fn decimal(mut value: usize, digits: &mut [u8; 20]) -> &str {
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    // All bytes above are decimal ASCII.
    core::str::from_utf8(&digits[at..]).unwrap_or("")
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
            assert_eq!(decimal(value, &mut buffer).parse(), Ok(value));
        }
    }
}
