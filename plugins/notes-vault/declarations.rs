//! What the notes vault declares: its settings group and the vault's folder,
//! the page, the lists and the tags as screens, the action and the Tools entry
//! that open the page, the Markdown format, and the six home widgets with their
//! settings groups (`PORTING-7.md`). `build.rs` builds the batches into the image.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, FormatDecl,
    GroupDecl, Instances, Kind, MenuEntryDecl, ScreenDecl, SpecDecl, WidgetDecl,
};
pub use paperpaw_ui_kit::widget::options::Opt;
use paperpaw_ui_kit::widget::options::{declare_all, shared_look};

/// The settings group (`notes-vault.group`).
pub const GROUP: &str = "notes-vault.group";

/// Callback tokens are stable within this plugin image.
pub const OPEN: u32 = 1;
/// The vault's page.
pub const HOME: u32 = 10;
/// A list of notes: folder, latest changes, tag, search results.
pub const LIST: u32 = 11;
/// The list of tags.
pub const TAGS: u32 = 12;
/// The index's work comes due.
pub const WORK_DUE: u32 = 20;
/// The Markdown format's `core.format` calls.
pub const FORMAT: u32 = 30;
/// The token of the settings that change (the widgets show them anew).
pub const SETTING_CHANGED: u32 = 21;
/// The widgets: *Recent notes*, *Pinned note*, *Note of the day*, *Random note*,
/// *Note tags* and *Search notes*.
pub const RECENT_TOKEN: u32 = 40;
pub const PINNED_TOKEN: u32 = 41;
pub const DAY_TOKEN: u32 = 42;
pub const RANDOM_TOKEN: u32 = 43;
pub const TAGS_TOKEN: u32 = 44;
pub const SEARCH_TOKEN: u32 = 45;

/// The settings group and the vault's folder.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.group(GroupDecl::new("group", "notes-vault.title"))?;
    let mut folder = SpecDecl::new(
        "folder",
        Kind::Text(""),
        GROUP,
        "notes-vault.setting.folder",
    );
    folder.help = "notes-vault.setting.folder.help";
    d.spec(folder)?;
    let mut extra = ExtraDecl::new("folder");
    extra.editor = Some(EditorDecl::Slot("core.choose-folder"));
    d.extra(extra)?;
    d.submit()
}

/// The vault's page, the lists and the tags, the action that opens the page,
/// its Tools entry (order 38), its icons and the subscription to the work that
/// makes the index.
pub fn ui<F: FnMut(&[u8]) -> Result<(), i32>>(d: &mut Declarations<F>) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("screen", "notes-vault.title", HOME).instances(Instances::Single))?;
    // A folder opens in a list of its own: each open is an instance.
    d.screen(ScreenDecl::new("list", "notes-vault.title", LIST).instances(Instances::Many))?;
    d.screen(ScreenDecl::new("tags", "notes-vault.tags", TAGS).instances(Instances::Single))?;
    d.action(ActionDecl::new("open", "notes-vault.title", OPEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "notes-vault.open");
    entry.order = 38;
    d.menu_entry(entry)?;
    d.icon("notes-vault.open", "note")?;
    d.icon(GROUP, "note")?;
    d.subscribe_event(EventKind::WorkDue, WORK_DUE)?;
    d.submit()
}

/// The Markdown format, as a vault reads it: before the format plugins' own
/// (priority 0) for the extensions both claim, and the user's choice of
/// format for them wins over both.
pub fn formats<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.format(
        FormatDecl::new("markdown", "notes-vault.markdown", "md\nmarkdown", FORMAT).priority(10),
    )?;
    d.submit()
}

const HEADING: Opt = Opt::Part("heading", "notes-vault.part.heading", true);
const WHERE: Opt = Opt::Part("where", "notes-vault.part.where", true);
const EXCERPT: Opt = Opt::Part("excerpt", "notes-vault.part.excerpt", true);
const FOLDER: Opt = Opt::Text(
    "folder",
    "notes-vault.widget.folder",
    "",
    "notes-vault.widget.folder.help",
);

/// The settings of *Recent notes* after the shared look, in their order.
pub const RECENT_OPTIONS: &[Opt] = &[
    HEADING,
    WHERE,
    Opt::Number("count", "notes-vault.widget.count", 1, 10, 5, ""),
    FOLDER,
];
/// The settings of *Pinned note* after the shared look.
pub const PINNED_OPTIONS: &[Opt] = &[
    HEADING,
    EXCERPT,
    Opt::Text(
        "title",
        "notes-vault.widget.title",
        "",
        "notes-vault.widget.title.help",
    ),
];
/// The settings of *Note of the day* after the shared look.
pub const DAY_OPTIONS: &[Opt] = &[HEADING, EXCERPT, FOLDER];
/// The settings of *Random note* after the shared look.
pub const RANDOM_OPTIONS: &[Opt] = &[HEADING, EXCERPT, FOLDER];
/// The settings of *Note tags* after the shared look.
pub const TAGS_OPTIONS: &[Opt] = &[
    HEADING,
    Opt::Number("count", "notes-vault.widget.count", 1, 12, 5, ""),
];
/// *Search notes* has the shared look alone.
pub const SEARCH_OPTIONS: &[Opt] = &[];

/// A widget: its name (the settings group and the settings carry it), its
/// token, its size in cells, the frame it has by default and its own settings.
type Widget = (&'static str, u32, (u8, u8), &'static str, &'static [Opt]);

/// The six widgets, in the order the catalogue lists them.
pub const WIDGETS: [Widget; 6] = [
    (
        "recent",
        RECENT_TOKEN,
        (2, 2),
        "core.frame.none",
        RECENT_OPTIONS,
    ),
    (
        "pinned",
        PINNED_TOKEN,
        (2, 1),
        "core.frame.none",
        PINNED_OPTIONS,
    ),
    ("day", DAY_TOKEN, (2, 1), "core.frame.none", DAY_OPTIONS),
    (
        "random",
        RANDOM_TOKEN,
        (2, 1),
        "core.frame.none",
        RANDOM_OPTIONS,
    ),
    ("tags", TAGS_TOKEN, (2, 1), "core.frame.none", TAGS_OPTIONS),
    (
        "search",
        SEARCH_TOKEN,
        (2, 1),
        "core.frame.line",
        SEARCH_OPTIONS,
    ),
];

/// The widgets, their settings groups and their settings: the look every
/// widget shares (text at the start of its line), then the widget's own.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, token, (columns, rows), frame, options) in WIDGETS {
        let title = ["notes-vault.", name].concat();
        let group = title.as_str();
        d.group(GroupDecl::new(name, group))?;
        let spec = WidgetDecl::new(name, group, "core.widgets.personal", token)
            .map_err(|_| DeclareError::TooLong("widget"))?
            .cells(columns, rows);
        d.widget(spec)?;
        let mut look = shared_look("core.align.left");
        look[1] = Opt::Frame(frame);
        declare_all(
            d,
            name,
            group,
            look.into_iter().chain(options.iter().copied()),
        )?;
        d.submit()?;
    }
    Ok(())
}

/// The events the plugin watches for its widgets: a setting that changed (the
/// vault's folder, a widget's own settings).
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::SettingChanged, SETTING_CHANGED)?;
    d.submit()
}
