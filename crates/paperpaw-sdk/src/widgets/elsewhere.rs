//! The widgets declared to the kernel that are not registered here: those
//! of native plugins, which the kernel keeps as records, not as
//! [`WidgetSpec`](super::WidgetSpec)s. The kernel's own screens that name
//! widgets (the editor of lists of widgets) ask for them through a lookup
//! the kernel sets; without one they know the registered widgets only.
use super::{Widgets, base};
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

impl Widgets {
    /// Sets how the widgets declared elsewhere are found: `lookup` calls
    /// its second argument with the id, the translation key of the name and
    /// the kind of each widget to add (none that previews settings); of the
    /// widget its first argument names only, where it names one.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_declared_elsewhere(
        &mut self,
        lookup: Box<dyn Fn(Option<&str>, &mut dyn FnMut(&str, &str, &str))>,
    ) {
        self.elsewhere = Elsewhere(Some(lookup));
    }

    /// The name's translation key and the kind of widget `id` (or of the
    /// widget a placement shows), declared elsewhere.
    fn elsewhere(&self, id: &str) -> Option<(String, String)> {
        let lookup = self.elsewhere.0.as_ref()?;
        let mut found = None;
        lookup(Some(base(id)), &mut |_, title, kind| {
            found.get_or_insert_with(|| (String::from(title), String::from(kind)));
        });
        found
    }

    /// The translation key of the name of widget `id`, registered here or
    /// declared elsewhere.
    pub(crate) fn title_of(&self, id: &str) -> Option<Cow<'_, str>> {
        match self.title(id) {
            Some(title) => Some(Cow::Borrowed(title)),
            None => self.elsewhere(id).map(|(title, _)| Cow::Owned(title)),
        }
    }

    /// The kind of widget `id`, registered here or declared elsewhere.
    pub(crate) fn kind_of(&self, id: &str) -> Option<Cow<'_, str>> {
        match self.category(id) {
            Some(kind) => Some(Cow::Borrowed(kind)),
            None => self.elsewhere(id).map(|(_, kind)| Cow::Owned(kind)),
        }
    }

    /// Every widget for screens to show ([`Widgets::iter`]), then those
    /// declared elsewhere: the id and the translation key of the name.
    pub(crate) fn offered(&self) -> Vec<(Id, String)> {
        let mut offered: Vec<(Id, String)> = self
            .iter()
            .map(|(id, title)| (id.clone(), String::from(title)))
            .collect();
        if let Some(lookup) = self.elsewhere.0.as_ref() {
            lookup(None, &mut |id, title, _| {
                if let Ok(id) = Id::parse(id)
                    && !self.has(id.as_str())
                {
                    offered.push((id, String::from(title)));
                }
            });
        }
        offered
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
    fn without_a_lookup_only_the_registered_widgets_are_known() {
        let widgets = widgets();
        assert_eq!(
            widgets.title_of("here.clock").as_deref(),
            Some("here.clock.title")
        );
        assert_eq!(widgets.title_of("home.clock"), None);
        assert_eq!(widgets.kind_of("home.clock"), None);
        let offered = widgets.offered();
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].0.as_str(), "here.clock");
    }

    #[test]
    fn widgets_declared_elsewhere_are_named_and_offered() {
        let mut widgets = widgets();
        widgets.set_declared_elsewhere(Box::new(declared));
        assert_eq!(
            widgets.title_of("home.clock").as_deref(),
            Some("home.widget.clock")
        );
        assert_eq!(
            widgets.kind_of("home.recent").as_deref(),
            Some(categories::READING)
        );
        // A placement is named as its widget.
        assert_eq!(
            widgets.title_of("home.clock@2").as_deref(),
            Some("home.widget.clock")
        );
        assert_eq!(widgets.title_of("home.none"), None);
        // One registered here stays what it is here, and is offered once.
        assert_eq!(
            widgets.title_of("here.clock").as_deref(),
            Some("here.clock.title")
        );
        let offered: Vec<_> = widgets
            .offered()
            .into_iter()
            .map(|(id, title)| (String::from(id.as_str()), title))
            .collect();
        assert_eq!(
            offered,
            [
                (String::from("here.clock"), String::from("here.clock.title")),
                (
                    String::from("home.clock"),
                    String::from("home.widget.clock")
                ),
                (
                    String::from("home.recent"),
                    String::from("home.widget.recent")
                ),
            ]
        );
    }
}
