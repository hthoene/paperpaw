//! A button's name on the key bar that follows the focus.
use paperpaw_sdk::native::{Host, commands::CommandSender, ui::ButtonHint};

use crate::Messages;

/// Names `action` on the key bar by the message `key` where `named`, and by
/// the button's usual name where not (a menu button that says *Forget* while
/// the focus is on a row that can be forgotten). Sent only where that changes
/// what the bar says, `shown`; a hint the host refuses leaves the bar, and
/// `shown`, as they were.
pub fn name_while(host: &Host<'_>, action: &str, key: &str, named: bool, shown: &mut bool) {
    if named == *shown {
        return;
    }
    let word = if named {
        match Messages::new().and_then(|mut say| say.copy(host, key)) {
            Ok(word) => Some(word),
            Err(_) => return,
        }
    } else {
        None
    };
    let hint = match &word {
        Some(word) => ButtonHint::words(action, word),
        None => ButtonHint::usual(action),
    };
    if let Ok(hint) = hint
        && CommandSender::new(host, &mut [0; 256])
            .set_hint(hint)
            .is_ok()
    {
        *shown = named;
    }
}
