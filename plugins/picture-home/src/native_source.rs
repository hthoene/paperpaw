//! What the picture home screen reads of the device to lay its pages out: the
//! page settings, the grid's size, the bar and the plates, the tools and the
//! widgets there are. All of it is read when the screen needs it, with fallible
//! allocation, into a snapshot the host-free model (`native_pages`) then asks
//! without a way to fail: a refusal ends the whole read, so a default is never
//! made up for a value the host would not give (`PORTING-7.md`).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    native::{
        Error, Host,
        registry::{MenuQuery, Registry, RegistryMenuKind},
        settings::{self, SettingValue},
    },
    widgets::grid::Geometry,
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer, push},
    rows,
    text::copy,
};

use paperpaw_ui_kit::status_line::Lists;

use crate::native_bar::{Look, Placing};
use crate::native_grid::Sizes;
use crate::native_pages::{self, MAX_PAGES, Source};
use crate::native_picture::Visible;

/// The menus the tools come from: the shortcuts, the tools, what there is to
/// open (the built-in home screen's, as interface 6 listed them).
const TOOL_MENUS: [&str; 3] = ["home.actions", "core.tools", "core.apps"];
/// A screenshot from here shows only the home screen: it is for the tools opened
/// over a book (unless listed in the shortcuts).
const SCREENSHOT: &str = "core.screenshot";
/// The settings is always a tool.
const SETTINGS: &str = "core.settings";
const COLUMNS: &str = "picture-home.columns";
const ROWS: &str = "picture-home.rows";
const PENCIL: &str = "picture-home.pencil";
const DOTS: &str = "picture-home.dots";
const FOCUS_AT_ONCE: &str = "picture-home.focus-at-once";
const PLATE: &str = "picture-home.plate";
const BAR: &str = "picture-home.bar";
const BAR_AT: &str = "picture-home.bar.at";
const BAR_SHAPE: &str = "picture-home.bar.shape";
const BAR_ALIGN: &str = "picture-home.bar.align";
const BAR_LOOK: &str = "picture-home.bar.look";
const BAR_LEFT: &str = "picture-home.bar.left-items";
const BAR_CENTER: &str = "picture-home.bar.center-items";
const BAR_RIGHT: &str = "picture-home.bar.right-items";
const HINTS: &str = "picture-home.hints";
/// The reader's choice that the arrow keys stay on the page.
const ARROWS_STAY: &str = "core.arrows-stay-on-page";
/// Most widgets and tools kept.
const MOST: usize = 256;
/// A page's text is at most this long (the settings' own cap).
const TEXT_MAX: usize = 4096;

/// A widget the registry lists.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WidgetRow {
    /// Its id.
    pub id: String,
    /// Its name (a catalog key or a name).
    pub title: String,
    /// What kind of widget it is.
    pub kind: String,
}

/// The snapshot.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct HostSource {
    texts: Vec<(String, String)>,
    columns: i64,
    /// The rows setting (0: as many as fit).
    pub rows: i64,
    /// The pencil shows below the page.
    pub pencil: bool,
    /// A dot for each page.
    pub dots: bool,
    /// The focus shows at once at a show.
    pub focus_at_once: bool,
    /// The arrow keys stay on the page (go round at its edges).
    pub arrows_stay: bool,
    /// What the widgets sit on (a value of the plate setting).
    pub plate: String,
    /// When the bar shows.
    pub bar: Visible,
    /// The bar is at the bottom.
    pub bar_bottom: bool,
    /// What the bar is on, and where its plate sits in the strip.
    pub bar_look: Look,
    pub bar_placing: Placing,
    /// The status items on the bar's left, centre and right.
    pub bar_items: Lists,
    /// When the row of hints below shows.
    pub hints: Visible,
    widgets: Vec<WidgetRow>,
    /// The tools' actions and their names.
    tools: Vec<(String, String)>,
    /// What widgets measured at the grid's width: id, rows, fewest rows.
    measured: Vec<(String, u8, u8)>,
}

impl HostSource {
    /// Reads the pages' settings, the grid's columns and rows, the switches, the
    /// bar's, the widgets and the tools.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals; nothing is kept in part.
    pub fn read(host: &Host<'_>) -> Result<Self, Error> {
        let mut texts = Vec::new();
        let mut scratch = buffer(TEXT_MAX + 64)?;
        for slot in 0..MAX_PAGES {
            let key = native_pages::setting(slot)?;
            let text = match settings::read_setting(host, &key, &mut scratch)? {
                SettingValue::Text(text) => copy(text)?,
                _ => String::new(),
            };
            push(&mut texts, (key, text))?;
        }
        let mut int = |key: &str, default: i64| -> Result<i64, Error> {
            Ok(match settings::read_setting(host, key, &mut scratch)? {
                SettingValue::Int(value) => value,
                _ => default,
            })
        };
        let columns = int(COLUMNS, 4)?;
        let rows = int(ROWS, 0)?;
        let mut on = |key: &str, default: bool| -> Result<bool, Error> {
            Ok(match settings::read_setting(host, key, &mut scratch)? {
                SettingValue::Bool(on) => on,
                _ => default,
            })
        };
        let pencil = on(PENCIL, true)?;
        let dots = on(DOTS, true)?;
        let focus_at_once = on(FOCUS_AT_ONCE, false)?;
        let arrows_stay = on(ARROWS_STAY, false)?;
        let mut text = |key: &str| -> Result<String, Error> {
            Ok(match settings::read_setting(host, key, &mut scratch)? {
                SettingValue::Text(text) => copy(text)?,
                _ => String::new(),
            })
        };
        let plate = text(PLATE)?;
        let bar = Visible::of(&text(BAR)?);
        let bar_bottom = text(BAR_AT)? == crate::settings::BAR_ATS[1];
        let bar_look = Look::of(&text(BAR_LOOK)?);
        let bar_placing = Placing::of(&text(BAR_SHAPE)?, &text(BAR_ALIGN)?);
        let bar_items = Lists {
            left: text(BAR_LEFT)?,
            center: text(BAR_CENTER)?,
            right: text(BAR_RIGHT)?,
            line: String::new(),
        };
        let hints = Visible::of(&text(HINTS)?);
        let registry = Registry::open(host)?;
        let found = Self::registry(host, &registry);
        let released = registry.release(host);
        let (widgets, tools) = found?;
        released?;
        Ok(Self {
            texts,
            columns,
            rows,
            pencil,
            dots,
            focus_at_once,
            arrows_stay,
            plate,
            bar,
            bar_bottom,
            bar_look,
            bar_placing,
            bar_items,
            hints,
            widgets,
            tools,
            measured: Vec::new(),
        })
    }

    #[allow(
        clippy::type_complexity,
        reason = "what one read of the registry returns"
    )]
    fn registry(
        host: &Host<'_>,
        registry: &Registry,
    ) -> Result<(Vec<WidgetRow>, Vec<(String, String)>), Error> {
        let mut input = buffer(512)?;
        let mut out = buffer(SERVICE_BUFFER)?;
        let mut widgets = Vec::new();
        rows!(cursor => registry.widgets(host, "", cursor, &mut input, &mut out), |w| {
            if widgets.len() < MOST {
                push(
                    &mut widgets,
                    WidgetRow {
                        id: copy(w.id())?,
                        title: copy(w.title())?,
                        kind: copy(w.kind())?,
                    },
                )
            } else {
                Ok(())
            }
        })?;
        let mut tools: Vec<(String, String)> = Vec::new();
        for menu in TOOL_MENUS {
            rows!(cursor => registry.menu(host, menu, MenuQuery::Current, cursor, &mut input, &mut out), |r| {
                let here_useless = menu == "core.tools" && r.item() == SCREENSHOT;
                let wanted = r.kind() == RegistryMenuKind::Action
                    && !here_useless
                    && tools.len() < MOST
                    && !tools.iter().any(|(t, _)| t == r.item());
                if wanted {
                    push(&mut tools, (copy(r.item())?, copy(r.title())?))
                } else {
                    Ok(())
                }
            })?;
        }
        if !tools.iter().any(|(t, _)| t == SETTINGS) {
            // Named as the core names it; the catalog key stands for the word.
            push(&mut tools, (copy(SETTINGS)?, copy("core.action.settings")?))?;
        }
        Ok((widgets, tools))
    }

    /// The name of a tool, as the user reads it (the action's id where the
    /// menu names none).
    #[must_use]
    pub fn tool_name<'a>(&'a self, action: &'a str) -> &'a str {
        self.tools
            .iter()
            .find(|(id, _)| id == action)
            .map_or(action, |(_, title)| title.as_str())
    }

    /// The widgets there are: id, name and kind.
    #[must_use]
    pub fn widget_rows(&self) -> &[WidgetRow] {
        &self.widgets
    }

    /// The tools there are: their actions and names.
    #[must_use]
    pub fn tool_rows(&self) -> &[(String, String)] {
        &self.tools
    }

    /// The grid's columns upright (`picture-home.columns`), as the grid holds
    /// them: 2 to 6.
    #[must_use]
    pub fn columns(&self) -> u8 {
        u8::try_from(
            self.columns
                .clamp(2, i64::from(paperpaw_sdk::widgets::grid::MAX_COLUMNS)),
        )
        .unwrap_or(4)
    }

    /// A widget measured at the grid's width: its rows and its fewest rows.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn measured(&mut self, id: &str, rows: u8, least: u8) -> Result<(), Error> {
        if let Some(known) = self.measured.iter_mut().find(|(known, ..)| known == id) {
            known.1 = rows;
            known.2 = least;
            return Ok(());
        }
        push(&mut self.measured, (copy(id)?, rows, least))
    }
}

impl Sizes for HostSource {
    /// As measured; a widget not measured takes the grid's width and one row.
    fn widget(&self, geometry: &Geometry, id: &str) -> (u8, u8, u8) {
        let base = native_pages::base(id);
        let (rows, least) = self
            .measured
            .iter()
            .find(|(known, ..)| known == id || known == base)
            .map_or((1, 1), |(_, rows, least)| (*rows, *least));
        let rows = rows.clamp(1, geometry.rows);
        (geometry.columns, rows, least.clamp(1, rows))
    }
}

impl Source for HostSource {
    fn text(&self, setting: &str) -> Result<String, Error> {
        copy(
            self.texts
                .iter()
                .find(|(key, _)| key == setting)
                .map_or("", |(_, text)| text.as_str()),
        )
    }

    fn has_widget(&self, id: &str) -> bool {
        let base = native_pages::base(id);
        self.widgets.iter().any(|w| w.id == base)
    }

    fn tools(&self) -> Result<Vec<String>, Error> {
        let mut found = Vec::new();
        for (tool, _) in &self.tools {
            push(&mut found, copy(tool)?)?;
        }
        Ok(found)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn source() -> HostSource {
        HostSource {
            widgets: ["home.clock", "home.recent"]
                .iter()
                .map(|s| WidgetRow {
                    id: (*s).into(),
                    title: (*s).into(),
                    kind: "k".into(),
                })
                .collect(),
            tools: [("core.library", "Library"), ("core.settings", "Settings")]
                .iter()
                .map(|(id, title)| ((*id).into(), (*title).into()))
                .collect(),
            ..HostSource::default()
        }
    }

    #[test]
    fn the_settings_read_are_those_the_plugin_declares() {
        for key in [
            COLUMNS,
            ROWS,
            PENCIL,
            DOTS,
            FOCUS_AT_ONCE,
            PLATE,
            BAR,
            BAR_AT,
        ] {
            assert!(
                crate::settings::SETTINGS
                    .iter()
                    .any(|s| s.id.as_str() == key),
                "{key}"
            );
        }
        assert_eq!(BAR_AT, crate::settings::BAR_AT.static_str());
        assert_eq!(TOOL_MENUS[0], "home.actions");
    }

    #[test]
    fn the_columns_are_held_to_the_grids() {
        let mut src = source();
        assert_eq!(src.columns(), 2);
        src.columns = 4;
        assert_eq!(src.columns(), 4);
        src.columns = 99;
        assert_eq!(src.columns(), 6);
    }

    #[test]
    fn a_widget_is_there_by_its_base_id() {
        let src = source();
        assert!(src.has_widget("home.clock"));
        assert!(src.has_widget("home.clock@2"));
        assert!(!src.has_widget("home.weather"));
    }

    #[test]
    fn a_tool_is_named_by_its_menu_row() {
        let src = source();
        assert_eq!(src.tool_name("core.library"), "Library");
        assert_eq!(src.tool_name("other.tool"), "other.tool");
    }

    #[test]
    fn a_widget_not_measured_takes_the_grid_width_and_a_row() {
        let geometry = Geometry::of(paperpaw_sdk::Rect::new(0, 0, 400, 300), 4, 3, 0);
        let mut src = source();
        assert_eq!(src.widget(&geometry, "home.clock"), (4, 1, 1));
        src.measured("home.clock", 9, 5).unwrap();
        // Kept inside the grid.
        assert_eq!(src.widget(&geometry, "home.clock@2"), (4, 3, 3));
        src.measured("home.clock", 2, 1).unwrap();
        assert_eq!(src.widget(&geometry, "home.clock"), (4, 2, 1));
    }

    #[test]
    fn tools_and_texts_are_copied_with_fallible_allocation() {
        let mut src = source();
        src.texts
            .push(("picture-home.page-1".into(), "a 0/0 1x1".into()));
        crate::fail_alloc::sweep(|| {
            assert_eq!(src.text("picture-home.page-1")?, "a 0/0 1x1");
            assert_eq!(src.text("picture-home.page-2")?, "");
            src.tools()?;
            src.measured("home.clock", 2, 1)?;
            Ok(())
        });
    }
}
