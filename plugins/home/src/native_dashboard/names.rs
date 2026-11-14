//! The names the footer's buttons are told by: they show icons and no
//! words, and a button without a name is told as nothing a reader could say
//! or a test could find (#243).

use alloc::string::String;

use crate::native_focus::{Button, Owner};

/// The message an action is told by where its plugin gave it no name.
pub(super) const UNNAMED: &str = "home.footer.action";

/// The name `owner` is told by, where it is one of the footer's buttons:
/// the pencil by its word, *Add* as "+", an action by its name. `None` for
/// the other targets (a cell is told by the text drawn in it).
pub(super) fn button_name<'a>(
    owner: Owner,
    pencil: Option<&'a str>,
    actions: &'a [(String, String)],
) -> Option<&'a str> {
    match owner {
        Owner::Button(Button::Pencil) => pencil,
        Owner::Button(Button::Plus) => Some("+"),
        Owner::Button(Button::Action(index)) => actions.get(index).map(|(_, name)| name.as_str()),
        _ => None,
    }
}

/// The name an action's button keeps: its own, or `unnamed` (the word for
/// [`UNNAMED`]) where its plugin gave it none.
pub(super) fn action_name<'a>(name: &'a str, unnamed: &'a str) -> &'a str {
    if name.is_empty() { unnamed } else { name }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn add_is_told_as_plus_and_an_action_by_its_name() {
        let actions = vec![("probe.switch".to_string(), "Switch".to_string())];
        let name = |owner| button_name(owner, Some("Edit"), &actions);
        assert_eq!(name(Owner::Button(Button::Plus)), Some("+"));
        assert_eq!(name(Owner::Button(Button::Pencil)), Some("Edit"));
        assert_eq!(name(Owner::Button(Button::Action(0))), Some("Switch"));
        assert_eq!(
            name(Owner::Button(Button::Action(1))),
            None,
            "no such action"
        );
    }

    #[test]
    fn an_action_without_a_name_is_told_by_the_generic_word() {
        assert_eq!(action_name("", UNNAMED), UNNAMED);
        assert_eq!(action_name("Switch", UNNAMED), "Switch");
    }

    #[test]
    fn the_generic_word_is_in_both_catalogs() {
        for catalog in [
            include_str!("../../i18n/en.ini"),
            include_str!("../../i18n/de.ini"),
        ] {
            assert!(
                catalog
                    .lines()
                    .any(|line| line.starts_with("home.footer.action = ")),
                "{UNNAMED} missing"
            );
        }
    }
}
