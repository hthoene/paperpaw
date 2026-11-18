//! What the Pomodoro plugin declares: its settings (lengths, rounds, auto-start;
//! under *More* the end signal and the daily goal), its screen and the actions
//! that open it, start or pause and skip, its place in the Tools menu, the three
//! home widgets with their look and parts, and the events it subscribes to.
//! `build.rs` turns it into the image's batches.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, GroupDecl, Kind, MenuEntryDecl,
    ScreenDecl, SpecDecl, WidgetDecl,
};
use paperpaw_ui_kit::widget::options::{Opt, declare_all, shared_look};

/// The token of the lifecycle events (start, wake, a setting changed).
pub const LIFECYCLE_TOKEN: u32 = 1;
/// The token of the work that ends a phase, ticks the minutes and brings the
/// front light back.
pub const WORK_TOKEN: u32 = 2;
/// The actions' tokens.
pub const OPEN_TOKEN: u32 = 3;
pub const TOGGLE_TOKEN: u32 = 4;
pub const SKIP_TOKEN: u32 = 5;
/// The screen.
pub const SCREEN_TOKEN: u32 = 10;
/// The widgets: the timer, the start/pause button and today's rounds.
pub const TIMER_TOKEN: u32 = 20;
pub const BUTTON_TOKEN: u32 = 21;
pub const TODAY_TOKEN: u32 = 22;

/// The settings group, as the settings name it.
const GROUP: &str = "pomodoro.group";
/// The part of the group the less common settings go under.
const SECTION: &str = "pomodoro.section.more";

/// The settings by their full ids.
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const FOCUS: &str = "pomodoro.focus";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const SHORT: &str = "pomodoro.short";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const LONG: &str = "pomodoro.long";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const ROUNDS: &str = "pomodoro.rounds";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const AUTO: &str = "pomodoro.auto";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const SIGNAL: &str = "pomodoro.signal";
#[allow(
    dead_code,
    reason = "only the plugin reads the settings; the build script sees the declarations alone"
)]
pub const GOAL: &str = "pomodoro.goal";

/// The values of the end signal: the notice alone, a flash of the screen (one
/// full refresh), a blink of the front light.
pub const SIGNAL_NOTICE: &str = "pomodoro.signal.notice";
pub const SIGNAL_FLASH: &str = "pomodoro.signal.flash";
pub const SIGNAL_LIGHT: &str = "pomodoro.signal.light";
pub const SIGNALS: [&str; 3] = [SIGNAL_NOTICE, SIGNAL_FLASH, SIGNAL_LIGHT];

/// The group, the lengths, the rounds, auto-start, and under *More* the end
/// signal and the daily goal.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", GROUP);
    group.order = 90;
    d.group(group)?;
    for (name, default, min, max, title, help) in [
        (
            "focus",
            25,
            1,
            90,
            "pomodoro.setting.focus",
            "pomodoro.setting.focus.help",
        ),
        (
            "short",
            5,
            1,
            30,
            "pomodoro.setting.short",
            "pomodoro.setting.short.help",
        ),
        (
            "long",
            15,
            1,
            60,
            "pomodoro.setting.long",
            "pomodoro.setting.long.help",
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Int { default, min, max }, GROUP, title);
        spec.help = help;
        d.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.unit = "core.unit.minutes";
        d.extra(extra)?;
    }
    let mut rounds = SpecDecl::new(
        "rounds",
        Kind::Int {
            default: 4,
            min: 2,
            max: 8,
        },
        GROUP,
        "pomodoro.setting.rounds",
    );
    rounds.help = "pomodoro.setting.rounds.help";
    d.spec(rounds)?;
    let mut auto = SpecDecl::new("auto", Kind::Bool(false), GROUP, "pomodoro.setting.auto");
    auto.help = "pomodoro.setting.auto.help";
    d.spec(auto)?;
    let mut signal = SpecDecl::new(
        "signal",
        Kind::Choice {
            default: SIGNAL_NOTICE,
            choices: &SIGNALS,
        },
        GROUP,
        "pomodoro.setting.signal",
    );
    signal.help = "pomodoro.setting.signal.help";
    signal.advanced = true;
    d.spec(signal)?;
    let mut goal = SpecDecl::new(
        "goal",
        Kind::Int {
            default: 0,
            min: 0,
            max: 24,
        },
        GROUP,
        "pomodoro.setting.goal",
    );
    goal.help = "pomodoro.setting.goal.help";
    goal.advanced = true;
    d.spec(goal)?;
    for name in ["signal", "goal"] {
        let mut extra = ExtraDecl::new(name);
        extra.section = SECTION;
        d.extra(extra)?;
    }
    d.submit()
}

/// The screen.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("screen", "pomodoro.title", SCREEN_TOKEN))?;
    d.submit()
}

/// The actions, the Tools menu entry and the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "pomodoro.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new(
        "toggle",
        "pomodoro.action.toggle",
        TOGGLE_TOKEN,
    ))?;
    d.action(ActionDecl::new("skip", "pomodoro.action.skip", SKIP_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "pomodoro.open");
    entry.order = 80;
    d.menu_entry(entry)?;
    d.icon("pomodoro.open", "clock")?;
    d.icon(GROUP, "clock")?;
    d.submit()
}

/// The settings of *Pomodoro timer* after the shared look: the bar and the
/// dots of the set can be left out.
pub const TIMER_OPTIONS: &[Opt] = &[
    Opt::Part("bar", "pomodoro.part.bar", true),
    Opt::Part("dots", "pomodoro.part.dots", true),
];
/// The settings of *Pomodoro start/pause*: none beyond the look.
pub const BUTTON_OPTIONS: &[Opt] = &[];
/// The settings of *Pomodoro today*: the week below the day's count.
pub const TODAY_OPTIONS: &[Opt] = &[Opt::Part("week", "pomodoro.part.week", true)];

/// The three widgets, their settings groups and their settings.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, title, token, cells, frame, options) in [
        (
            "timer",
            "pomodoro.timer",
            TIMER_TOKEN,
            (2, 2),
            "core.frame.none",
            TIMER_OPTIONS,
        ),
        (
            "button",
            "pomodoro.button",
            BUTTON_TOKEN,
            (1, 1),
            "core.frame.line",
            BUTTON_OPTIONS,
        ),
        (
            "today",
            "pomodoro.today",
            TODAY_TOKEN,
            (2, 1),
            "core.frame.none",
            TODAY_OPTIONS,
        ),
    ] {
        let group = ["pomodoro.", name].concat();
        d.group(GroupDecl::new(name, title))?;
        // No `live`: the plugin says when what a widget shows changed (each
        // minute while the timer runs), and no widget asks for a redraw of its own.
        let spec = WidgetDecl::new(name, title, "core.widgets.reading", token)
            .map_err(|_| DeclareError::TooLong("widget"))?
            .cells(cells.0, cells.1);
        d.widget(spec)?;
        let mut look = shared_look("core.align.center");
        look[1] = Opt::Frame(frame);
        declare_all(
            d,
            name,
            &group,
            look.into_iter().chain(options.iter().copied()),
        )?;
        d.submit()?;
    }
    Ok(())
}

/// The events the plugin watches: the start and the wake (a round goes on where
/// it was), a setting that changed, and the work that ends a phase.
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
    d.submit()
}
