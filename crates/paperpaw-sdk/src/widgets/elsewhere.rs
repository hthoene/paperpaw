//! The widgets declared to the kernel that are not registered here: those
//! of native plugins, which the kernel keeps as records, not as
//! [`WidgetSpec`](super::WidgetSpec)s. The kernel's own screens that name
//! widgets (the editor of lists of widgets) ask for them through a lookup
//! the kernel sets; without one they know the registered widgets only.
use super::base;
use crate::Id;
use alloc::{borrow::Cow, boxed::Box, string::String, vec::Vec};

/// Calls its second argument with the id, the translation key of the name
/// and the kind of each widget to add that is declared elsewhere; of the
/// widget its first argument names only, where it names one.
type Lookup = dyn Fn(Option<&str>, &mut dyn FnMut(&str, &str, &str));

/// The kernel's lookup, where it set one.
#[derive(Default)]
pub(super) struct Elsewhere(Option<Box<Lookup>>);

impl core::fmt::Debug for Elsewhere {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(if self.0.is_some() {
            "Elsewhere(set)"
        } else {
            "Elsewhere"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{Drawn, WidgetSpec, categories};

    fn widgets() -> Widgets {
        let mut widgets = Widgets::new();
        widgets.add(WidgetSpec::drawn(
            Id::new_static("here.clock"),
            "here.clock.title",
            Drawn::new(|_, _| 0, |_, _, _| {}),
        ));
        widgets
    }

    const DECLARED: [(&str, &str, &str); 3] = [
        ("home.clock", "home.widget.clock", categories::TIME),
        ("home.recent", "home.widget.recent", categories::READING),
        ("here.clock", "another.title", categories::TIME),
    ];

    fn declared(only: Option<&str>, each: &mut dyn FnMut(&str, &str, &str)) {
        for (id, title, kind) in DECLARED {
            if only.is_none_or(|only| only == id) {
                each(id, title, kind);
            }
        }
    }

    #[test]
    fn tools_list_the_widgets_registered_and_declared_elsewhere_with_their_kinds() {
        let mut widgets = widgets();
        assert_eq!(widgets.everywhere().len(), 1);
        widgets.set_declared_elsewhere(Box::new(declared));
        let all = widgets.everywhere();
        let all: Vec<(&str, &str, &str)> = all
            .iter()
            .map(|(id, title, kind)| (id.as_str(), title.as_str(), kind.as_str()))
            .collect();
        // The registered one first, as registered; a declaration of its id adds none.
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].0, "here.clock");
        assert_eq!(all[0].1, "here.clock.title");
        assert_eq!(all[1], DECLARED[0]);
        assert_eq!(all[2], DECLARED[1]);
    }
}
