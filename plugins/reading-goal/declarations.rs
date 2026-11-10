//! Reading goal's declarations as borrowed builders, shared by the build script
//! (which writes them into the plugin image) and the tests.
//!
//! The settings sit in the statistics' group (`stats.group`), as on interface 6.
//! The sleep screen (*Today's reading cat*) is declared by [`screens`], offered to
//! the slot `core.sleep` below the sleep plugin's own screen, and added to the
//! choices of `sleep.show` by [`navigation`].
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Instances, Kind,
    ScreenDecl, SlotOfferDecl, SpecDecl, StageDecl, WidgetDecl,
};
use paperpaw_ui_kit::widget::options::{Opt, declare_all};

/// Callback tokens are stable within this plugin image.
pub const EVENTS_TOKEN: u32 = 1;

/// The callback token of the sleep screen.
pub const SLEEP_TOKEN: u32 = 10;
/// The sleep screen's full id (and the value of `sleep.show` that chooses it).
pub const SLEEP_SCREEN: &str = "reading-goal.sleep";

/// The callback token of the minute tick.
pub const TICK_TOKEN: u32 = 3;
/// The callback token of the widget (drawing and taps).
pub const WIDGET_TOKEN: u32 = 2;
/// The widget, relative to the plugin: its id is `reading-goal.widget`.
pub const WIDGET: &str = "widget";
/// The widget's group, settings and title (a translation key).
pub const WIDGET_GROUP: &str = "reading-goal.widget";
/// The widget's settings: the shared look without an alignment (the cat is
/// centred), and the part that shows the minutes.
pub const WIDGET_OPTIONS: [Opt; 8] = [
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
    Opt::Part("minutes", "reading-goal.part.minutes", true),
];

/// The statistics' group, where the settings are.
pub const GROUP: &str = "stats.group";
/// The part of the group that is this plugin's.
const SECTION: &str = "reading-goal.title";
/// The stage this plugin runs to ask the statistics for today's reading (its
/// full id is `reading-goal.read-today`).
pub const READ_TODAY: &str = "read-today";

/// The three settings: from which share of the goal the cat is content and
/// happy (percent), and a folder of one's own pictures.
pub fn settings(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    for (name, default, min, max, title, help) in [
        (
            "content",
            50,
            5,
            95,
            "reading-goal.setting.content",
            "reading-goal.setting.content.help",
        ),
        (
            "happy",
            100,
            50,
            200,
            "reading-goal.setting.happy",
            "reading-goal.setting.happy.help",
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Int { default, min, max }, GROUP, title);
        spec.help = help;
        spec.advanced = true;
        declarations.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.section = SECTION;
        extra.unit = "core.unit.percent";
        extra.step = 5;
        extra.slider = true;
        declarations.extra(extra)?;
    }
    let mut pictures = SpecDecl::new(
        "pictures",
        Kind::Text(""),
        GROUP,
        "reading-goal.setting.pictures",
    );
    pictures.help = "reading-goal.setting.pictures.help";
    pictures.advanced = true;
    declarations.spec(pictures)?;
    let mut extra = ExtraDecl::new("pictures");
    extra.section = SECTION;
    extra.editor = Some(EditorDecl::Slot("core.choose-folder"));
    declarations.extra(extra)?;
    declarations.submit()
}

/// What makes the plugin keep today's figure: a book opened or closed, the
/// device's sleep and stop.
pub fn events(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    for kind in [
        EventKind::BookOpened,
        EventKind::BookClosed,
        EventKind::Suspend,
        EventKind::Stopping,
    ] {
        declarations.subscribe_event(kind, EVENTS_TOKEN)?;
    }
    // The picture of the sleep screen is asked for at the start.
    declarations.subscribe_event(EventKind::Started, EVENTS_TOKEN)?;
    // A new day makes the cat sad again; the figure is asked for each minute.
    declarations.subscribe_event(EventKind::Tick, TICK_TOKEN)?;
    // The answer to a picture asked for, and a change of the folder of one's own.
    declarations.subscribe_event(EventKind::ImageReady, EVENTS_TOKEN)?;
    declarations.subscribe_event(EventKind::SettingChanged, EVENTS_TOKEN)?;
    declarations.submit()
}

/// The stage `reading-goal.read-today` (schema 1): this plugin runs it when it
/// needs the figure, and the statistics answer from what they hold in memory.
/// Input: the day as an `i32`, little-endian (empty: today). Reply: the day as an
/// `i32` and the seconds read on it as a `u32`, little-endian; the statistics
/// answer `ERR_NOT_FOUND` for a day they do not know.
pub fn stages(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.stage(StageDecl::new(READ_TODAY, 1, 4, 8, 1))?;
    declarations.submit()
}

/// The sleep screen: the cat large in the middle, the minutes below it. One
/// at a time.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("sleep", SLEEP_SCREEN, SLEEP_TOKEN).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The screen offered to the slot `core.sleep` below the sleep plugin's own
/// (priority 0), and its place among the choices of `sleep.show`.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut offer = SlotOfferDecl::new("core.sleep", SLEEP_SCREEN);
    offer.priority = -1;
    d.slot_offer(offer)?;
    d.setting_choice("sleep.show", SLEEP_SCREEN, SLEEP_SCREEN)?;
    d.submit()?;
    Ok(())
}

/// *Reading cat* (`reading-goal.widget`): its group, the widget (live: asked
/// each minute) and its settings.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.group(GroupDecl::new(WIDGET, WIDGET_GROUP))?;
    let widget = WidgetDecl::new(WIDGET, WIDGET_GROUP, "core.widgets.reading", WIDGET_TOKEN)
        .map_err(|_| DeclareError::TooLong("widget"))?
        .live()
        .cells(2, 2);
    // No `previews`: a widget that previews a settings group is never offered
    // in the Add picker (Kernel ruling, #228), and Beta 3 declared none.
    d.widget(widget)?;
    declare_all(d, WIDGET, WIDGET_GROUP, WIDGET_OPTIONS)?;
    d.submit()
}
