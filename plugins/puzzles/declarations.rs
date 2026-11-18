//! What the Puzzles plugin declares: its screen, the action and Tools menu
//! entry that open it (and one that opens the puzzle of the day), the home
//! widget *Puzzle of the day* with its look, and the events it subscribes to.
//! `build.rs` turns it into the image's batches.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, GroupDecl, MenuEntryDecl, ScreenDecl,
    WidgetDecl,
};
use paperpaw_ui_kit::widget::options::{declare_all, shared_look};

/// The token of the lifecycle events (start, wake, a setting changed).
pub const LIFECYCLE_TOKEN: u32 = 1;
/// The actions' tokens.
pub const OPEN_TOKEN: u32 = 3;
pub const DAILY_TOKEN: u32 = 4;
/// The screen.
pub const SCREEN_TOKEN: u32 = 10;
/// The widget.
pub const WIDGET_TOKEN: u32 = 20;

/// The screen.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("screen", "puzzles.title", SCREEN_TOKEN))?;
    d.submit()
}

/// The actions, the Tools menu entry and the icon.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "puzzles.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new(
        "daily",
        "puzzles.action.daily",
        DAILY_TOKEN,
    ))?;
    let mut entry = MenuEntryDecl::new("core.tools", "puzzles.open");
    entry.order = 85;
    d.menu_entry(entry)?;
    d.icon("puzzles.open", "layout-grid")?;
    d.submit()
}

/// The widget, its settings group and its look.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = "puzzles.daily";
    d.group(GroupDecl::new("daily", "puzzles.widget.name"))?;
    let spec = WidgetDecl::new(
        "daily",
        "puzzles.widget.name",
        "core.widgets.reading",
        WIDGET_TOKEN,
    )
    .map_err(|_| DeclareError::TooLong("widget"))?
    .cells(2, 1);
    d.widget(spec)?;
    let look = shared_look("core.align.left");
    declare_all(d, "daily", group, look)?;
    d.submit()
}

/// The events the plugin watches: the start and the wake (a new day), and a
/// setting that changed (the widget's look).
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for kind in [
        EventKind::Started,
        EventKind::Resume,
        EventKind::SettingChanged,
    ] {
        d.subscribe_event(kind, LIFECYCLE_TOKEN)?;
    }
    d.submit()
}
