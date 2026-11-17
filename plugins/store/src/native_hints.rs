//! The row of button hints where a button does less, or other, than its usual
//! name says (#235): the source screen's buttons do nothing but Back, the
//! empty list's *Select* tries again. A refused hint leaves the row as it was.
use paperpaw_sdk::native::{Host, commands::CommandSender, ui::ButtonHint};

/// Hides the hints of `actions`: the buttons do nothing here.
pub(crate) fn hide(host: &Host<'_>, actions: &[&str]) {
    for action in actions {
        if let Ok(hint) = ButtonHint::hidden(action) {
            let _ = CommandSender::new(host, &mut [0; 256]).set_hint(hint);
        }
    }
}

/// Labels the button of `action` with `words`, already translated.
pub(crate) fn say(host: &Host<'_>, action: &str, words: &str) {
    if let Ok(hint) = ButtonHint::words(action, words) {
        let _ = CommandSender::new(host, &mut [0; 256]).set_hint(hint);
    }
}

/// Gives the button of `action` its usual name again.
pub(crate) fn usual(host: &Host<'_>, action: &str) {
    if let Ok(hint) = ButtonHint::usual(action) {
        let _ = CommandSender::new(host, &mut [0; 256]).set_hint(hint);
    }
}
