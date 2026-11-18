//! What the Calendar plugin declares: its settings (when it syncs, how many
//! days the agenda shows, and the calendars, edited on their own screen), its
//! three screens (the calendar, the sources, the sleep screen), the actions
//! that open them and sync, its place in the Tools menu, the sleep screen
//! offered to `core.sleep`, the five home widgets with their look and
//! settings, and the events it subscribes to. `build.rs` turns it into the
//! image's batches.
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
/// The screens: the calendar, the sources, the sleep screen.
pub const SCREEN_TOKEN: u32 = 10;
pub const SOURCES_TOKEN: u32 = 11;
pub const SLEEP_TOKEN: u32 = 12;
/// The widgets: today, the next event, the agenda, the month, birthdays.
pub const TODAY_TOKEN: u32 = 20;
pub const NEXT_TOKEN: u32 = 21;
pub const AGENDA_TOKEN: u32 = 22;
pub const MONTH_TOKEN: u32 = 23;
pub const BIRTHDAYS_TOKEN: u32 = 24;

/// The sleep screen's full id (and the value of `sleep.show` that chooses it).
pub const SLEEP_SCREEN: &str = "calendar.sleep";
/// The settings group, as the settings name it.
const GROUP: &str = "calendar.group";

/// The settings by their full ids.
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const SYNC: &str = "calendar.sync";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const DAYS: &str = "calendar.days";
/// When the calendars are read besides when the calendar is opened and on
/// *Sync now*: only then, also when the radio is on anyway, or also when
/// charging begins.
pub const SYNC_CHOICES: [&str; 3] = [
    "calendar.sync.open",
    "calendar.sync.online",
    "calendar.sync.charging",
];
/// How many days the agenda shows unless set otherwise, and the most.
pub const DAYS_DEFAULT: i64 = 7;
pub const DAYS_MOST: i32 = 30;

/// The group, when to sync, the agenda's days and the sources' row.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", GROUP);
    group.order = 92;
    d.group(group)?;
    let mut sources = SpecDecl::new("sources", Kind::Text(""), GROUP, "calendar.setting.sources");
    sources.help = "calendar.setting.sources.help";
    sources.kept_on_reset = true;
    d.spec(sources)?;
    let mut extra = ExtraDecl::new("sources");
    extra.editor = Some(EditorDecl::Screen("sources"));
    d.extra(extra)?;
    let mut sync = SpecDecl::new(
        "sync",
        Kind::Choice {
            default: SYNC_CHOICES[1],
            choices: &SYNC_CHOICES,
        },
        GROUP,
        "calendar.setting.sync",
    );
    sync.help = "calendar.setting.sync.help";
    d.spec(sync)?;
    let mut days = SpecDecl::new(
        "days",
        Kind::Int {
            default: DAYS_DEFAULT,
            min: 1,
            max: DAYS_MOST,
        },
        GROUP,
        "calendar.setting.days",
    );
    days.help = "calendar.setting.days.help";
    d.spec(days)?;
    d.submit()
}

/// The screens: the calendar, the sources and the sleep screen.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(
        ScreenDecl::new("screen", "calendar.title", SCREEN_TOKEN).instances(Instances::Single),
    )?;
    d.screen(
        ScreenDecl::new("sources", "calendar.sources.title", SOURCES_TOKEN)
            .instances(Instances::Single),
    )?;
    // The agenda has the whole screen: no row of button hints over it.
    d.screen(
        ScreenDecl::new("sleep", SLEEP_SCREEN, SLEEP_TOKEN)
            .instances(Instances::Single)
            .hints_hidden(),
    )?;
    d.submit()
}

/// The actions, the Tools menu entry, the icons, and the sleep screen offered
/// to the slot `core.sleep` below the sleep plugin's own, with its place among
/// the choices of `sleep.show`.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "calendar.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new("sync", "calendar.action.sync", SYNC_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "calendar.open");
    entry.order = 82;
    d.menu_entry(entry)?;
    d.icon("calendar.open", "calendar")?;
    d.icon("calendar.sync", "refresh")?;
    d.icon(GROUP, "calendar")?;
    let mut offer = SlotOfferDecl::new("core.sleep", SLEEP_SCREEN);
    offer.priority = -2;
    d.slot_offer(offer)?;
    d.setting_choice("sleep.show", SLEEP_SCREEN, SLEEP_SCREEN)?;
    d.submit()
}

/// How many rows a widget shows at most, and unless set otherwise.
pub const ROWS_MOST: i32 = 12;
pub const ROWS_DEFAULT: i64 = 5;

/// *Today*: how many events, the date in large letters, the heading.
pub const TODAY_OPTIONS: &[Opt] = &[
    Opt::Number(
        "count",
        "calendar.widget.setting.count",
        1,
        ROWS_MOST,
        4,
        "calendar.widget.setting.count.help",
    ),
    Opt::Part("date", "calendar.widget.setting.date", true),
    Opt::Part("place", "calendar.widget.setting.place", false),
];
/// *Next event*: where it is, and how long until it.
pub const NEXT_OPTIONS: &[Opt] = &[
    Opt::Part("place", "calendar.widget.setting.place", true),
    Opt::Part("until", "calendar.widget.setting.until", true),
];
/// *Agenda*: how many days ahead, how many events, the heading.
pub const AGENDA_OPTIONS: &[Opt] = &[
    Opt::Number(
        "days",
        "calendar.widget.setting.days",
        1,
        14,
        7,
        "calendar.widget.setting.days.help",
    ),
    Opt::Number(
        "count",
        "calendar.widget.setting.count",
        1,
        ROWS_MOST,
        ROWS_DEFAULT,
        "calendar.widget.setting.count.help",
    ),
    Opt::Part("caption", "calendar.widget.setting.caption", true),
    Opt::Part("place", "calendar.widget.setting.place", false),
];
/// *Month*: the dots under days with events, the month's name, the week's
/// number column.
pub const MONTH_OPTIONS: &[Opt] = &[
    Opt::Part("dots", "calendar.widget.setting.dots", true),
    Opt::Part("caption", "calendar.widget.setting.caption", true),
];
/// *Birthdays*: how many days ahead and how many.
pub const BIRTHDAYS_OPTIONS: &[Opt] = &[
    Opt::Number(
        "days",
        "calendar.widget.setting.days",
        7,
        60,
        30,
        "calendar.widget.setting.days.help",
    ),
    Opt::Number(
        "count",
        "calendar.widget.setting.count",
        1,
        8,
        4,
        "calendar.widget.setting.count.help",
    ),
    Opt::Part("caption", "calendar.widget.setting.caption", true),
];

/// The five widgets, their settings groups and their settings.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, title, token, cells, align, options) in [
        (
            "today",
            "calendar.widget.today",
            TODAY_TOKEN,
            (2, 2),
            "core.align.left",
            TODAY_OPTIONS,
        ),
        (
            "next",
            "calendar.widget.next",
            NEXT_TOKEN,
            (2, 1),
            "core.align.left",
            NEXT_OPTIONS,
        ),
        (
            "agenda",
            "calendar.widget.agenda",
            AGENDA_TOKEN,
            (2, 3),
            "core.align.left",
            AGENDA_OPTIONS,
        ),
        (
            "month",
            "calendar.widget.month",
            MONTH_TOKEN,
            (2, 2),
            "core.align.center",
            MONTH_OPTIONS,
        ),
        (
            "birthdays",
            "calendar.widget.birthdays",
            BIRTHDAYS_TOKEN,
            (2, 2),
            "core.align.left",
            BIRTHDAYS_OPTIONS,
        ),
    ] {
        let group = ["calendar.", name].concat();
        d.group(GroupDecl::new(name, title))?;
        // No `live`: the plugin says when what a widget shows changed (a sync,
        // the day's turn, the next event beginning); no widget asks for a
        // redraw itself.
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
/// turned), a setting that changed, the work that is due, the end of a
/// request and charging beginning.
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
