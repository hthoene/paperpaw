//! What the home screen reads of the device to lay its pages out: the page
//! settings, the grid's size, the tools and the widgets there are. All of it is
//! read when the screen needs it, with fallible allocation, into a snapshot the
//! host-free model (`native_pages`) then asks without a way to fail: a refusal
//! ends the whole read, so a default is never made up for a value the host
//! would not give (`PORTING-7.md`, notes for the native screen).
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

use crate::native_grid::Sizes;
use crate::native_pages::{self, MAX_PAGES, Source};

/// The menus the tools come from: the shortcuts, the tools, what there is to
/// open.
const TOOL_MENUS: [&str; 3] = ["home.actions", "core.tools", "core.apps"];
/// A screenshot from here shows only the home screen: it is for the tools opened
/// over a book (unless listed in the shortcuts).
const SCREENSHOT: &str = "core.screenshot";
/// The settings is always a tool.
const SETTINGS: &str = "core.settings";
/// The settings the pages are laid out by.
const STATUS_BAR: &str = "home.status-bar";
const COLUMNS: &str = "home.columns";
const ROWS: &str = "home.rows";
const PENCIL: &str = "home.footer.pencil";
const DOTS: &str = "home.footer.dots";
const TOOL_LABELS: &str = "home.tool-labels";
const FOCUS_AT_ONCE: &str = "home.focus-at-once";
/// The reader's choice that the arrow keys stay on the page.
const ARROWS_STAY: &str = "core.arrows-stay-on-page";
/// The menu of the footer's actions.
const FOOTER: &str = "home.footer";
/// The registry's menu row flag: chosen now (else faint).
const ENABLED: u32 = 1 << 1;
/// The widest a widget is on a screen on its side, in millimetres.
pub const WIDEST_MM: u32 = 110;
/// Most widgets and tools kept.
const MOST: usize = 256;
/// A page's text is at most this long (the settings' own cap).
const TEXT_MAX: usize = 4096;

/// The snapshot.
#[derive(Debug, Default)]
pub struct HostSource {
    texts: Vec<(String, String)>,
    status_bar: bool,
    columns: i64,
    /// What the dashboard shows besides the pages.
    pub rows: i64,
    pub pencil: bool,
    pub dots: bool,
    pub tool_labels: bool,
    pub focus_at_once: bool,
    /// The arrow keys stay on the page (go round at its edges).
    pub arrows_stay: bool,
    widgets: Vec<String>,
    /// The tools' actions and their names.
    tools: Vec<(String, String)>,
    /// The footer's actions: the action, its name, whether it is chosen now.
    pub footer: Vec<(String, String, bool)>,
    widest: u32,
    /// What widgets measured at the grid's width: id, rows, fewest rows.
    measured: Vec<(String, u8, u8)>,
}

impl HostSource {
    /// Reads the pages' settings (the upright and the wide layout), the grid's
    /// columns, the status bar, the widgets and the tools; `dpi` gives the
    /// widest a widget is on a screen on its side.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals; nothing is kept in part.
    pub fn read(host: &Host<'_>, dpi: u16) -> Result<Self, Error> {
        let mut texts = Vec::new();
        let mut scratch = buffer(TEXT_MAX + 64)?;
        for wide in [false, true] {
            for slot in 0..MAX_PAGES {
                let key = native_pages::setting(slot, wide)?;
                let text = match settings::read_setting(host, &key, &mut scratch)? {
                    SettingValue::Text(text) => copy(text)?,
                    _ => String::new(),
                };
                push(&mut texts, (key, text))?;
            }
        }
        let status_bar = matches!(
            settings::read_setting(host, STATUS_BAR, &mut scratch)?,
            SettingValue::Bool(true)
        );
        let columns = match settings::read_setting(host, COLUMNS, &mut scratch)? {
            SettingValue::Int(columns) => columns,
            _ => 4,
        };
        let rows = match settings::read_setting(host, ROWS, &mut scratch)? {
            SettingValue::Int(rows) => rows,
            _ => 0,
        };
        let mut on = |key: &str, default: bool| -> Result<bool, Error> {
            Ok(match settings::read_setting(host, key, &mut scratch)? {
                SettingValue::Bool(on) => on,
                _ => default,
            })
        };
        let pencil = on(PENCIL, true)?;
        let dots = on(DOTS, true)?;
        let tool_labels = on(TOOL_LABELS, true)?;
        let focus_at_once = on(FOCUS_AT_ONCE, false)?;
        let arrows_stay = on(ARROWS_STAY, false)?;
        let registry = Registry::open(host)?;
        let found = Self::registry(host, &registry);
        let released = registry.release(host);
        let (widgets, tools, footer) = found?;
        released?;
        Ok(Self {
            texts,
            status_bar,
            columns,
            rows,
            pencil,
            dots,
            tool_labels,
            focus_at_once,
            arrows_stay,
            widgets,
            tools,
            footer,
            widest: mm(dpi, WIDEST_MM),
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
    ) -> Result<
        (
            Vec<String>,
            Vec<(String, String)>,
            Vec<(String, String, bool)>,
        ),
        Error,
    > {
        let mut input = buffer(512)?;
        let mut out = buffer(SERVICE_BUFFER)?;
        let mut widgets = Vec::new();
        rows!(cursor => registry.widgets(host, "", cursor, &mut input, &mut out), |w| {
            if widgets.len() < MOST {
                push(&mut widgets, copy(w.id())?)
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
        let mut footer = Vec::new();
        rows!(cursor => registry.menu(host, FOOTER, MenuQuery::Current, cursor, &mut input, &mut out), |r| {
            if r.kind() == RegistryMenuKind::Action && footer.len() < MOST {
                push(
                    &mut footer,
                    (copy(r.item())?, copy(r.title())?, r.flags() & ENABLED != 0),
                )
            } else {
                Ok(())
            }
        })?;
        Ok((widgets, tools, footer))
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

/// Pixels for `mm` millimetres at `dpi`, as the SDK's `Metrics::mm` has them:
/// rounded, at least 1.
#[must_use]
pub fn mm(dpi: u16, mm: u32) -> u32 {
    let tenths = u64::from(mm) * 10;
    let px = (tenths * u64::from(dpi) + 127) / 254;
    let px = u32::try_from(px).unwrap_or(u32::MAX);
    if px == 0 && tenths > 0 { 1 } else { px }
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

    fn status_bar(&self) -> bool {
        self.status_bar
    }

    fn columns(&self) -> i64 {
        self.columns
    }

    fn has_widget(&self, id: &str) -> bool {
        let base = native_pages::base(id);
        self.widgets.iter().any(|w| w == base)
    }

    fn tools(&self) -> Result<Vec<String>, Error> {
        let mut found = Vec::new();
        for (tool, _) in &self.tools {
            push(&mut found, copy(tool)?)?;
        }
        Ok(found)
    }

    fn widest_px(&self) -> u32 {
        self.widest
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn millimetres_round_as_the_sdk_does() {
        // 110 mm at 220 dpi (X4): 952.8 px.
        assert_eq!(mm(220, WIDEST_MM), 953);
        assert_eq!(mm(0, 1), 1);
        assert_eq!(mm(220, 0), 0);
    }

    fn source() -> HostSource {
        HostSource {
            widgets: ["home.clock", "home.recent"]
                .iter()
                .map(|s| (*s).into())
                .collect(),
            tools: [("core.library", "Library"), ("core.settings", "Settings")]
                .iter()
                .map(|(id, title)| ((*id).into(), (*title).into()))
                .collect(),
            ..HostSource::default()
        }
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
        src.texts.push(("home.page-1".into(), "a 0/0 1x1".into()));
        crate::fail_alloc::sweep(|| {
            assert_eq!(src.text("home.page-1")?, "a 0/0 1x1");
            assert_eq!(src.text("home.page-2")?, "");
            src.tools()?;
            src.measured("home.clock", 2, 1)?;
            Ok(())
        });
    }
}
