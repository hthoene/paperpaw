//! *Online ›* in the panel's header: where a screen is offered for the slot
//! `core.lookup-online` (Wikipedia's), a tap on it, or the menu button, closes the
//! panel and opens that screen for the same word and language.
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::{Input, InputGestureKind},
        commands::CommandSender,
        registry::{PageCursor, Registry},
        ui::{Open, Param, Ui},
    },
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

/// The slot of the screens that look a word up online.
pub const SLOT: &str = "core.lookup-online";

/// Whether a screen is offered for [`SLOT`].
///
/// # Errors
/// The host's refusals; `NoMemory`.
pub fn offered(host: &Host<'_>) -> Result<bool, Error> {
    let registry = Registry::open(host)?;
    let found = first_offer(host, &registry);
    let released = registry.release(host);
    let any = found?;
    released?;
    Ok(any)
}

fn first_offer(host: &Host<'_>, registry: &Registry) -> Result<bool, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let page = registry.slot_offers(host, SLOT, PageCursor::start(), &mut input, &mut out)?;
    Ok(!page.is_empty())
}

/// Whether `input` asks to go online: a tap on the header's *Online ›* (at
/// `area`), or the menu button.
#[must_use]
pub fn asks(input: &Input<'_>, area: Option<Rect>) -> bool {
    match *input {
        Input::Action { action, .. } => action == "core.menu",
        Input::Gesture {
            position,
            kind: InputGestureKind::Tap,
            ..
        } => area.is_some_and(|area| area.contains(position)),
        _ => false,
    }
}

/// Closes the panel and opens the screen offered for [`SLOT`] for `word` in
/// `language` (as Beta 3: back, then the slot with the word and the language).
///
/// # Errors
/// The host's refusals; `NoMemory`.
pub fn go(host: &Host<'_>, ui: &Ui, word: &str, language: &str) -> Result<(), Error> {
    let params = [Param::new("word", word)?, Param::new("language", language)?];
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let handover = ui.handover(host, &params, &mut input, &mut out)?;
    let mut scratch = [0; 256];
    let mut commands = CommandSender::new(host, &mut scratch);
    commands.close_screen()?;
    commands.open(Open::slot(SLOT)?.handover(handover))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::Point;

    fn tap(x: i32, y: i32) -> Input<'static> {
        Input::Gesture {
            now_ms: 0,
            position: Point::new(x, y),
            kind: InputGestureKind::Tap,
        }
    }

    #[test]
    fn the_menu_button_and_a_tap_on_the_label_go_online_and_nothing_else_does() {
        let area = Some(Rect::new(300, 200, 60, 40));
        let action = |action| Input::Action { now_ms: 0, action };
        assert!(asks(&action("core.menu"), area));
        assert!(
            asks(&action("core.menu"), None),
            "the button needs no label"
        );
        assert!(!asks(&action("core.confirm"), area));
        assert!(asks(&tap(320, 210), area));
        assert!(
            !asks(&tap(100, 210), area),
            "a tap elsewhere is the panel's"
        );
        assert!(!asks(&tap(320, 210), None));
    }
}
