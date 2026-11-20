//! What the Tasks plugin declares: its settings (the account's address and
//! name, set on the account's screen, and how long done tasks stay), its two
//! screens, the actions that open the lists and sync, its place in the Tools
//! menu, the four home widgets with their look and settings, and the events
//! it subscribes to. `build.rs` turns it into the image's batches.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Instances,
    Kind, MenuEntryDecl, ScreenDecl, SlotOfferDecl, SpecDecl, WidgetDecl,
};
use paperpaw_ui_kit::widget::options::{Opt, declare_all, shared_look};

/// The token of the lifecycle events (start, wake, a setting changed).
pub const LIFECYCLE_TOKEN: u32 = 1;
/// The token of the work: a sync, and the day's turn for the widgets.
pub const WORK_TOKEN: u32 = 2;
/// The token of the end of a request.
pub const HTTP_TOKEN: u32 = 3;
/// The actions' tokens.
pub const OPEN_TOKEN: u32 = 4;
pub const SYNC_TOKEN: u32 = 5;
/// The screens: the lists, and the account.
pub const SCREEN_TOKEN: u32 = 10;
pub const ACCOUNT_TOKEN: u32 = 11;
/// The sleep screen's token, and its full id (the value of `sleep.show` that
/// chooses it).
pub const SLEEP_TOKEN: u32 = 12;
pub const SLEEP_SCREEN: &str = "tasks.sleep";
/// The widgets: today's tasks, one list, the count, the next task.
pub const TODAY_TOKEN: u32 = 20;
pub const LIST_TOKEN: u32 = 21;
pub const COUNT_TOKEN: u32 = 22;
pub const NEXT_TOKEN: u32 = 23;

/// The settings group, as the settings name it.
const GROUP: &str = "tasks.group";

/// The settings by their full ids.
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const ADDRESS: &str = "tasks.address";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const USER: &str = "tasks.user";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const KEEP_DONE: &str = "tasks.keep-done";
/// How many days a done task stays unless set otherwise, and the most.
pub const KEEP_DONE_DEFAULT: i64 = 7;
pub const KEEP_DONE_MOST: i32 = 60;

/// The group, the account (edited on its screen) and the days done tasks stay.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", GROUP);
    group.order = 91;
    d.group(group)?;
    for (name, title, help) in [
        (
            "address",
            "tasks.setting.address",
            "tasks.setting.address.help",
        ),
        ("user", "tasks.setting.user", "tasks.setting.user.help"),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Text(""), GROUP, title);
        spec.help = help;
        spec.kept_on_reset = true;
        d.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.editor = Some(EditorDecl::Screen("account"));
        d.extra(extra)?;
    }
    let mut keep = SpecDecl::new(
        "keep-done",
        Kind::Int {
            default: KEEP_DONE_DEFAULT,
            min: 0,
            max: KEEP_DONE_MOST,
        },
        GROUP,
        "tasks.setting.keep-done",
    );
    keep.help = "tasks.setting.keep-done.help";
    d.spec(keep)?;
    d.submit()
}

/// The screens: the lists, the account and the sleep screen, one of each at a time.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("screen", "tasks.title", SCREEN_TOKEN).instances(Instances::Single))?;
    d.screen(
        ScreenDecl::new("account", "tasks.account.title", ACCOUNT_TOKEN)
            .instances(Instances::Single),
    )?;
    // The page has the whole screen: no row of button hints over it.
    d.screen(
        ScreenDecl::new("sleep", SLEEP_SCREEN, SLEEP_TOKEN)
            .instances(Instances::Single)
            .hints_hidden(),
    )?;
    d.submit()
}

/// The actions, the Tools menu entry and the icons, and the sleep screen
/// offered to the slot `core.sleep` below the sleep plugin's own and the other
/// plugins', with its place among the choices of `sleep.show`.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "tasks.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new("sync", "tasks.action.sync", SYNC_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "tasks.open");
    entry.order = 81;
    d.menu_entry(entry)?;
    d.icon("tasks.open", "list-check")?;
    d.icon("tasks.sync", "refresh")?;
    d.icon(GROUP, "list-check")?;
    let mut offer = SlotOfferDecl::new("core.sleep", SLEEP_SCREEN);
    offer.priority = -3;
    d.slot_offer(offer)?;
    d.setting_choice("sleep.show", SLEEP_SCREEN, SLEEP_SCREEN)?;
    d.submit()
}

/// How many tasks a widget shows at most, and unless set otherwise.
pub const ROWS_MOST: i32 = 10;
pub const ROWS_DEFAULT: i64 = 5;

/// The settings of *Tasks today* after the shared look: how many, the heading.
pub const TODAY_OPTIONS: &[Opt] = &[
    Opt::Number(
        "count",
        "tasks.widget.setting.count",
        1,
        ROWS_MOST,
        ROWS_DEFAULT,
        "tasks.widget.setting.count.help",
    ),
    Opt::Part("caption", "tasks.widget.setting.caption", true),
];
/// The settings of *Task list*: which list, how many, the heading, and
/// whether the done ones show.
pub const LIST_OPTIONS: &[Opt] = &[
    Opt::Number(
        "list",
        "tasks.widget.setting.list",
        1,
        12,
        1,
        "tasks.widget.setting.list.help",
    ),
    Opt::Number(
        "count",
        "tasks.widget.setting.count",
        1,
        ROWS_MOST,
        ROWS_DEFAULT,
        "tasks.widget.setting.count.help",
    ),
    Opt::Part("caption", "tasks.widget.setting.caption", true),
    Opt::Part("done", "tasks.widget.setting.done", false),
];
/// The settings of *Open tasks*: the line of what is due.
pub const COUNT_OPTIONS: &[Opt] = &[Opt::Part("due", "tasks.widget.setting.due", true)];
/// The settings of *Next task*: the line of its day and list.
pub const NEXT_OPTIONS: &[Opt] = &[Opt::Part("due", "tasks.widget.setting.when", true)];

/// The four widgets, their settings groups and their settings.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, title, token, cells, align, options) in [
        (
            "today",
            "tasks.widget.today",
            TODAY_TOKEN,
            (2, 2),
            "core.align.left",
            TODAY_OPTIONS,
        ),
        (
            "list",
            "tasks.widget.list",
            LIST_TOKEN,
            (2, 2),
            "core.align.left",
            LIST_OPTIONS,
        ),
        (
            "count",
            "tasks.widget.count",
            COUNT_TOKEN,
            (1, 1),
            "core.align.center",
            COUNT_OPTIONS,
        ),
        (
            "next",
            "tasks.widget.next",
            NEXT_TOKEN,
            (2, 1),
            "core.align.left",
            NEXT_OPTIONS,
        ),
    ] {
        let group = ["tasks.", name].concat();
        d.group(GroupDecl::new(name, title))?;
        // No `live`: the plugin says when what a widget shows changed (a task
        // ticked, a sync, the day's turn); no widget asks for a redraw itself.
        let spec = WidgetDecl::new(name, title, "core.widgets.personal", token)
            .map_err(|_| DeclareError::TooLong("widget"))?
            .cells(cells.0, cells.1);
        d.widget(spec)?;
        declare_all(
            d,
            name,
            &group,
            shared_look(align)
                .into_iter()
                .chain(options.iter().copied()),
        )?;
        d.submit()?;
    }
    Ok(())
}

/// The events the plugin watches: the start and the wake (the day may have
/// turned), a setting that changed, the work that is due and the end of a
/// request.
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
    d.subscribe_event(EventKind::WorkDue, WORK_TOKEN)?;
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.submit()
}
