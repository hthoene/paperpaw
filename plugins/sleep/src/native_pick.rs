//! The picker of the picture and the widget over the page, without the host:
//! the rows (the pictures folder, its pictures, the presets that offer a
//! picture, the widgets), which of them is chosen now and what choosing one
//! sets. The same rows and rules as the interface-6 picker, with the folder's
//! own pictures added.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{items::Item, mem::push, text::copy};

/// What marks the chosen row at its right.
pub const MARK: &str = "•";
/// Pictures offered at most (the folder keeps as many, see `Pictures`).
pub const MOST_PICTURES: usize = crate::native_sleep::MAX_PICTURES;
/// Presets offered at most.
pub const MOST_PRESETS: usize = 32;
/// Widgets offered at most.
pub const MOST_WIDGETS: usize = 256;
/// A path longer than this is not offered: a setting could not keep it.
pub const MOST_PATH: usize = 1024;

/// A preset that offers a picture (`sleep.overlay`), and whether it is applied
/// now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub id: String,
    pub title: String,
    pub applied: bool,
}

/// A widget that can go over the page: its id and the row's title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Widget {
    pub id: String,
    pub title: String,
}

/// What the rows after the first stand for: the picture rows (their names are
/// the rows' titles), then these presets and widgets, in order.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pictures: usize,
    presets: Vec<String>,
    widgets: Vec<String>,
}

/// A row, as the layout reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source<'a> {
    /// The pictures folder, one after the other.
    Folder,
    /// A picture of the folder, by name.
    Picture(&'a str),
    /// A preset, by id.
    Preset(&'a str),
    /// A widget, by id.
    Widget(&'a str),
}

/// What was chosen, owned: what the commands of a choice need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Folder,
    /// The picture's path on the card.
    Picture(String),
    Preset(String),
    Widget(String),
}

/// What a choice sets: the widget and the picture (`None`: left as it is) and
/// the preset to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sets<'a> {
    pub widget: Option<&'a str>,
    pub picture: Option<&'a str>,
    pub preset: Option<&'a str>,
}

/// The rows built.
#[derive(Debug)]
pub struct Built {
    pub items: Vec<Item>,
    pub layout: Layout,
    /// The row the focus starts on: the chosen one, else the first.
    pub focus: usize,
}

/// The folder of the pictures from its setting: without the slashes round it,
/// as the card's paths are written.
#[must_use]
pub fn folder(setting: &str) -> &str {
    setting.trim().trim_matches('/')
}

/// The path of picture `name` in `folder` (both as [`folder`] writes them).
fn path_len(folder: &str, name: &str) -> usize {
    if folder.is_empty() {
        name.len()
    } else {
        folder.len().saturating_add(1).saturating_add(name.len())
    }
}

/// Whether the picture setting `picture` names `name` in `folder`.
#[must_use]
pub fn names_picture(picture: &str, folder: &str, name: &str) -> bool {
    let picture = picture.trim().trim_matches('/');
    if folder.is_empty() {
        return picture == name;
    }
    picture
        .strip_prefix(folder)
        .and_then(|rest| rest.strip_prefix('/'))
        == Some(name)
}

/// The rows, in order: the pictures folder (`folder_title`), its pictures, the
/// presets, the widgets. `widget` and `picture` are the settings as they are
/// now: a widget set overrides every picture, so only it is marked then.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn build(
    folder_title: &str,
    folder: &str,
    names: &[String],
    presets: &[Preset],
    widgets: &[Widget],
    widget: &str,
    picture: &str,
) -> Result<Built, Error> {
    let (widget, picture) = (widget.trim(), picture.trim());
    let mut items = Vec::new();
    let mut layout = Layout::default();
    let total = 1usize
        .saturating_add(names.len().min(MOST_PICTURES))
        .saturating_add(presets.len().min(MOST_PRESETS))
        .saturating_add(widgets.len().min(MOST_WIDGETS));
    items.try_reserve_exact(total).map_err(|_| Error::NoMemory)?;
    layout
        .presets
        .try_reserve_exact(presets.len().min(MOST_PRESETS))
        .map_err(|_| Error::NoMemory)?;
    layout
        .widgets
        .try_reserve_exact(widgets.len().min(MOST_WIDGETS))
        .map_err(|_| Error::NoMemory)?;
    let mut chosen = None;
    let mut mark = |items: &mut Vec<Item>, title: &str, on: bool| -> Result<(), Error> {
        if on && chosen.is_none() {
            chosen = Some(items.len());
        }
        items.push(Item::row(title, if on { MARK } else { "" })?);
        Ok(())
    };
    mark(&mut items, folder_title, widget.is_empty() && picture.is_empty())?;
    for name in names.iter().take(MOST_PICTURES) {
        if path_len(folder, name) > MOST_PATH {
            continue;
        }
        let on = widget.is_empty() && names_picture(picture, folder, name);
        mark(&mut items, name, on)?;
        layout.pictures += 1;
    }
    for preset in presets.iter().take(MOST_PRESETS) {
        mark(&mut items, &preset.title, widget.is_empty() && preset.applied)?;
        push(&mut layout.presets, copy(&preset.id)?)?;
    }
    for entry in widgets.iter().take(MOST_WIDGETS) {
        mark(&mut items, &entry.title, entry.id == widget)?;
        push(&mut layout.widgets, copy(&entry.id)?)?;
    }
    Ok(Built {
        items,
        layout,
        focus: chosen.unwrap_or(0),
    })
}

impl Layout {
    /// What row `index` stands for; `None` for a row that is not there.
    #[must_use]
    pub fn source<'a>(&'a self, items: &'a [Item], index: usize) -> Option<Source<'a>> {
        if index == 0 {
            return matches!(items.first(), Some(Item::Row(_))).then_some(Source::Folder);
        }
        let n = index - 1;
        if n < self.pictures {
            return match items.get(index)? {
                Item::Row(row) => Some(Source::Picture(&row.title)),
                Item::Heading(_) => None,
            };
        }
        let n = n - self.pictures;
        if let Some(id) = self.presets.get(n) {
            return Some(Source::Preset(id));
        }
        self.widgets
            .get(n - self.presets.len())
            .map(|id| Source::Widget(id))
    }

    /// What choosing row `index` is, owned; `None` for a row that is not
    /// there.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn choice(
        &self,
        items: &[Item],
        folder: &str,
        index: usize,
    ) -> Result<Option<Choice>, Error> {
        Ok(match self.source(items, index) {
            None => None,
            Some(Source::Folder) => Some(Choice::Folder),
            Some(Source::Preset(id)) => Some(Choice::Preset(copy(id)?)),
            Some(Source::Widget(id)) => Some(Choice::Widget(copy(id)?)),
            Some(Source::Picture(name)) => {
                let mut path = String::new();
                path.try_reserve_exact(path_len(folder, name))
                    .map_err(|_| Error::NoMemory)?;
                if !folder.is_empty() {
                    path.push_str(folder);
                    path.push('/');
                }
                path.push_str(name);
                Some(Choice::Picture(path))
            }
        })
    }
}

impl Choice {
    /// What the choice sets. A folder, a picture or a preset leaves no widget
    /// over the picture; a widget leaves the picture as it is.
    #[must_use]
    pub fn sets(&self) -> Sets<'_> {
        match self {
            Self::Folder => Sets {
                widget: Some(""),
                picture: Some(""),
                preset: None,
            },
            Self::Picture(path) => Sets {
                widget: Some(""),
                picture: Some(path),
                preset: None,
            },
            Self::Preset(id) => Sets {
                widget: Some(""),
                picture: None,
                preset: Some(id),
            },
            Self::Widget(id) => Sets {
                widget: Some(id),
                picture: None,
                preset: None,
            },
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "fixture failures fail the test")]
mod tests {
    use super::*;
    use crate::fail_alloc::sweep;
    use alloc::string::ToString;

    fn names() -> Vec<String> {
        ["a.jpg", "b.png"].iter().map(ToString::to_string).collect()
    }

    fn presets() -> Vec<Preset> {
        Vec::from([Preset {
            id: "owls.night".to_string(),
            title: "Night owl".to_string(),
            applied: false,
        }])
    }

    fn widgets() -> Vec<Widget> {
        Vec::from([
            Widget {
                id: "home.clock".to_string(),
                title: "Widget: Clock".to_string(),
            },
            Widget {
                id: "home.date".to_string(),
                title: "Widget: Date".to_string(),
            },
        ])
    }

    fn built(widget: &str, picture: &str) -> Built {
        build(
            "From the pictures folder",
            "sleep",
            &names(),
            &presets(),
            &widgets(),
            widget,
            picture,
        )
        .unwrap()
    }

    fn marked(built: &Built) -> Vec<usize> {
        built
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, Item::Row(row) if row.value == MARK))
            .map(|(at, _)| at)
            .collect()
    }

    #[test]
    fn the_rows_are_the_folder_its_pictures_the_presets_the_widgets() {
        let built = built("", "");
        let titles: Vec<&str> = built
            .items
            .iter()
            .map(|item| match item {
                Item::Row(row) => row.title.as_str(),
                Item::Heading(_) => "",
            })
            .collect();
        assert_eq!(
            titles,
            [
                "From the pictures folder",
                "a.jpg",
                "b.png",
                "Night owl",
                "Widget: Clock",
                "Widget: Date"
            ]
        );
        let at = |index| built.layout.source(&built.items, index);
        assert_eq!(at(0), Some(Source::Folder));
        assert_eq!(at(1), Some(Source::Picture("a.jpg")));
        assert_eq!(at(2), Some(Source::Picture("b.png")));
        assert_eq!(at(3), Some(Source::Preset("owls.night")));
        assert_eq!(at(4), Some(Source::Widget("home.clock")));
        assert_eq!(at(5), Some(Source::Widget("home.date")));
        assert_eq!(at(6), None);
    }

    #[test]
    fn the_chosen_row_is_marked_and_the_focus_starts_on_it() {
        // Nothing set: the folder's own pictures in turn.
        let built_none = built("", "");
        assert_eq!((marked(&built_none), built_none.focus), (Vec::from([0]), 0));
        // A picture of the folder, written with slashes round it or not.
        for picture in ["sleep/b.png", " /sleep/b.png/ "] {
            let built = built("", picture);
            assert_eq!((marked(&built), built.focus), (Vec::from([2]), 2));
        }
        // A picture the folder does not have marks no row, the focus is first.
        let gone = built("", "sleep/gone.png");
        assert_eq!((marked(&gone), gone.focus), (Vec::new(), 0));
        // A widget set overrides every picture, which are then not marked.
        let widget = built("home.date", "sleep/a.jpg");
        assert_eq!((marked(&widget), widget.focus), (Vec::from([5]), 5));
        // A widget no plugin has any more marks nothing.
        let stale = built("owls.gone", "");
        assert_eq!(marked(&stale), Vec::<usize>::new());
    }

    #[test]
    fn a_preset_applied_is_marked_unless_a_widget_overrides_it() {
        let mut applied = presets();
        applied[0].applied = true;
        let make = |widget: &str| {
            build("Folder", "", &[], &applied, &widgets(), widget, "").unwrap()
        };
        assert_eq!(marked(&make("")), [1]);
        assert_eq!(marked(&make("home.clock")), [2]);
    }

    #[test]
    fn a_picture_in_the_card_root_has_no_folder_in_its_path() {
        assert!(names_picture("a.jpg", "", "a.jpg"));
        assert!(names_picture("/a.jpg", "", "a.jpg"));
        assert!(!names_picture("sleep/a.jpg", "", "a.jpg"));
        assert!(!names_picture("sleepa.jpg", "sleep", "a.jpg"));
        assert!(!names_picture("other/a.jpg", "sleep", "a.jpg"));
        assert!(!names_picture("sleep/sub/a.jpg", "sleep", "a.jpg"));
        assert_eq!(folder(" /wallpapers/ "), "wallpapers");
        assert_eq!(folder(""), "");
    }

    #[test]
    fn a_choice_says_what_it_sets() {
        let built = built("", "");
        let choose = |index| {
            built
                .layout
                .choice(&built.items, "sleep", index)
                .unwrap()
                .map(|choice| {
                    let sets = choice.sets();
                    (
                        sets.widget.map(ToString::to_string),
                        sets.picture.map(ToString::to_string),
                        sets.preset.map(ToString::to_string),
                    )
                })
        };
        let some = |text: &str| Some(text.to_string());
        assert_eq!(choose(0), Some((some(""), some(""), None)));
        assert_eq!(choose(2), Some((some(""), some("sleep/b.png"), None)));
        assert_eq!(choose(3), Some((some(""), None, some("owls.night"))));
        assert_eq!(choose(5), Some((some("home.date"), None, None)));
        assert_eq!(choose(6), None);
    }

    #[test]
    fn a_picture_of_the_card_root_is_chosen_by_its_name() {
        let built = build("F", "", &names(), &[], &[], "", "").unwrap();
        assert_eq!(
            built.layout.choice(&built.items, "", 1).unwrap(),
            Some(Choice::Picture("a.jpg".to_string()))
        );
    }

    #[test]
    fn a_path_a_setting_could_not_keep_is_not_offered() {
        let long = "x".repeat(MOST_PATH);
        let names = Vec::from([long, "ok.png".to_string()]);
        let built = build("F", "dir", &names, &[], &[], "", "").unwrap();
        assert_eq!(built.items.len(), 2);
        assert_eq!(
            built.layout.source(&built.items, 1),
            Some(Source::Picture("ok.png"))
        );
    }

    #[test]
    fn more_than_the_most_are_cut() {
        let names: Vec<String> = (0..MOST_PICTURES + 5).map(|n| n.to_string()).collect();
        let widgets: Vec<Widget> = (0..MOST_WIDGETS + 5)
            .map(|n| Widget {
                id: n.to_string(),
                title: n.to_string(),
            })
            .collect();
        let built = build("F", "", &names, &[], &widgets, "", "").unwrap();
        assert_eq!(built.items.len(), 1 + MOST_PICTURES + MOST_WIDGETS);
    }

    #[test]
    fn refused_allocations_are_errors() {
        let (names, presets, widgets) = (names(), presets(), widgets());
        sweep(|| {
            let built = build(
                "Folder",
                "sleep",
                &names,
                &presets,
                &widgets,
                "home.date",
                "",
            )?;
            for index in 0..built.items.len() {
                built.layout.choice(&built.items, "sleep", index)?;
            }
            Ok(())
        });
    }
}
