//! The row of button hints while the page is edited: *Back* ends editing, so
//! it is named *Done* there (as in Beta 3); the usual name again afterwards.
//! A refused hint leaves the row as it was.
use paperpaw_sdk::native::{Host, commands::CommandSender, ui::ButtonHint};
use paperpaw_ui_kit::Messages;

/// The word of the *Done* button.
const DONE: &str = "picture-home.edit.done";

/// Brings the label of *Back* in line with `editing`: *Done* while editing,
/// the usual name once it ends. `sent` is what was sent last, kept by the
/// caller: nothing goes out where it is already so. For a callback that may
/// send commands (an input or a show), not a draw.
pub fn sync(host: &Host<'_>, editing: bool, sent: &mut bool) {
    if *sent == editing {
        return;
    }
    *sent = editing;
    let mut scratch = [0; 256];
    let mut commands = CommandSender::new(host, &mut scratch);
    if editing {
        if let Ok(word) = Messages::new().and_then(|mut say| say.copy(host, DONE))
            && let Ok(hint) = ButtonHint::words("core.back", &word)
        {
            let _ = commands.set_hint(hint);
        }
    } else if let Ok(hint) = ButtonHint::usual("core.back") {
        let _ = commands.set_hint(hint);
    }
}
