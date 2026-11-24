//! Buttons' names on the key bar: one that follows the focus, and the four
//! a number is changed with.
use alloc::string::String;
use paperpaw_sdk::native::{Error, Host, commands::CommandSender, ui::ButtonHint};

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

/// The buttons a number is changed with: *Back*, *Select*, left and right.
const EDIT_ACTIONS: [&str; 4] = ["core.back", "core.confirm", "core.left", "core.right"];

/// The key bar while a number is changed with the buttons, as Beta 3's
/// number editor: *Cancel*, *Keep*, `−` and `+` (`core.hint.cancel`,
/// `core.hint.keep`); `on` false gives the buttons their usual names again.
/// A hint the host refuses leaves the bar as it was.
pub fn edit_hints(host: &Host<'_>, on: bool) {
    let mut said = [String::new(), String::new()];
    if on {
        let Ok(mut messages) = Messages::new() else {
            return;
        };
        for (word, key) in said.iter_mut().zip(["core.hint.cancel", "core.hint.keep"]) {
            match messages.copy(host, key) {
                Ok(text) => *word = text,
                Err(_) => return,
            }
        }
    }
    let [cancel, keep] = &said;
    let words = on.then_some((cancel.as_str(), keep.as_str()));
    for hint in edit_bar(words).into_iter().flatten() {
        match CommandSender::new(host, &mut [0; 256]).set_hint(hint) {
            Ok(()) | Err(_) => {}
        }
    }
}

/// The hints [`edit_hints`] sends: `words` (*Cancel*, *Keep*) with `−` and
/// `+` while a number is changed, the buttons' usual names where `None`.
fn edit_bar<'a>(words: Option<(&'a str, &'a str)>) -> [Result<ButtonHint<'a>, Error>; 4] {
    let said = match words {
        Some((cancel, keep)) => [Some(cancel), Some(keep), Some("\u{2212}"), Some("+")],
        None => [None; 4],
    };
    let mut said = said.into_iter();
    EDIT_ACTIONS.map(|action| match said.next().flatten() {
        Some(word) => ButtonHint::words(action, word),
        None => ButtonHint::usual(action),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_changed_names_its_four_buttons() {
        let bar = edit_bar(Some(("Cancel", "Keep"))).map(Result::ok);
        assert_eq!(
            bar,
            [
                ButtonHint::words("core.back", "Cancel").ok(),
                ButtonHint::words("core.confirm", "Keep").ok(),
                ButtonHint::words("core.left", "\u{2212}").ok(),
                ButtonHint::words("core.right", "+").ok(),
            ]
        );
    }

    #[test]
    fn the_buttons_get_their_usual_names_back() {
        let bar = edit_bar(None).map(Result::ok);
        assert_eq!(
            bar,
            EDIT_ACTIONS.map(|action| ButtonHint::usual(action).ok())
        );
        assert!(bar.iter().all(Option::is_some));
    }
}
