//! What the Picture frame plugin declares: the settings (the card's folder,
//! when the photo changes, how it is fitted, its contrast, the slideshow, the
//! server and its cache), the screens (the home with the server's account, the
//! slideshow and the sleep screen), the actions, the place in the tools, the
//! sleep screen offered to the slot `core.sleep`, the widget *Photo of the
//! day* and the events it reads.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Instances,
    Kind, MenuEntryDecl, ScreenDecl, SlotOfferDecl, SpecDecl, WidgetDecl,
};
use paperpaw_ui_kit::widget::options::{Opt, declare_all};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const SLIDESHOW_TOKEN: u32 = 2;
pub const SYNC_TOKEN: u32 = 3;
/// The end of a request.
pub const HTTP_TOKEN: u32 = 4;
/// The plugin stops: a sync under way is cancelled.
pub const STOP_TOKEN: u32 = 5;
/// Events that change what is shown or when it is fetched.
pub const EVENTS_TOKEN: u32 = 6;
/// The minute tick.
pub const TICK_TOKEN: u32 = 7;
/// The widget's drawing and taps.
pub const WIDGET_TOKEN: u32 = 8;
/// The screens.
pub const HOME: u32 = 10;
pub const SHOW: u32 = 11;
pub const SLEEP: u32 = 12;

/// The sleep screen's full id (and the value of `sleep.show` that chooses it).
pub const SLEEP_SCREEN: &str = "picture-frame.sleep";

/// The settings' group.
pub const GROUP: &str = "picture-frame.group";

/// When the photo of the sleep screen changes.
pub const CHANGE: [&str; 2] = ["picture-frame.change.sleep", "picture-frame.change.daily"];
/// How a photo is fitted to the screen.
pub const FIT: [&str; 3] = [
    "picture-frame.fit.auto",
    "picture-frame.fit.whole",
    "picture-frame.fit.fill",
];
/// When the photos of the server are fetched.
pub const SYNC: [&str; 3] = [
    "picture-frame.sync.manual",
    "picture-frame.sync.charging",
    "picture-frame.sync.online",
];

/// The widget, relative to the plugin: its id is `picture-frame.widget`.
pub const WIDGET: &str = "widget";
/// The widget's group, settings and title (a translation key).
pub const WIDGET_GROUP: &str = "picture-frame.widget";
/// The widget's settings: the shared look; the photo fills what the frame
/// leaves.
pub const WIDGET_OPTIONS: [Opt; 7] = [
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
];

/// The settings: the card's folder, when the photo changes, how it is fitted
/// and made contrasty, the slideshow, and the photo server (its address and
/// user are edited on the home screen with the password, which is a secret).
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "picture-frame.title");
    group.order = 63;
    d.group(group)?;
    let mut folder = SpecDecl::new(
        "folder",
        Kind::Text("/photos"),
        GROUP,
        "picture-frame.setting.folder",
    );
    folder.help = "picture-frame.setting.folder.help";
    d.spec(folder)?;
    let mut extra = ExtraDecl::new("folder");
    extra.editor = Some(EditorDecl::Slot("core.choose-folder"));
    d.extra(extra)?;
    for (name, default, choices, title, help) in [
        (
            "change",
            CHANGE[0],
            &CHANGE[..],
            "picture-frame.setting.change",
            "picture-frame.setting.change.help",
        ),
        (
            "fit",
            FIT[0],
            &FIT[..],
            "picture-frame.setting.fit",
            "picture-frame.setting.fit.help",
        ),
        (
            "sync",
            SYNC[1],
            &SYNC[..],
            "picture-frame.setting.sync",
            "picture-frame.setting.sync.help",
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Choice { default, choices }, GROUP, title);
        spec.help = help;
        d.spec(spec)?;
    }
    for (name, default, min, max, step, unit, title, help) in [
        (
            "contrast",
            10,
            -50,
            50,
            5,
            "core.unit.percent",
            "picture-frame.setting.contrast",
            "picture-frame.setting.contrast.help",
        ),
        (
            "interval",
            10,
            1,
            240,
            1,
            "core.unit.minutes",
            "picture-frame.setting.interval",
            "picture-frame.setting.interval.help",
        ),
        (
            "cache-mb",
            64,
            8,
            1024,
            8,
            "picture-frame.unit.mb",
            "picture-frame.setting.cache",
            "picture-frame.setting.cache.help",
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Int { default, min, max }, GROUP, title);
        spec.help = help;
        d.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.unit = unit;
        extra.step = step;
        extra.slider = name != "cache-mb";
        d.extra(extra)?;
    }
    slideshow_and_server(d)?;
    d.submit()?;
    Ok(())
}

/// The slideshow's switch and the photo server's address and user.
fn slideshow_and_server<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut charge = SpecDecl::new(
        "slideshow-on-charge",
        Kind::Bool(false),
        GROUP,
        "picture-frame.setting.slideshow-on-charge",
    );
    charge.help = "picture-frame.setting.slideshow-on-charge.help";
    d.spec(charge)?;
    for (name, title, help) in [
        (
            "server",
            "picture-frame.setting.server",
            "picture-frame.setting.server.help",
        ),
        (
            "user",
            "picture-frame.setting.user",
            "picture-frame.setting.user.help",
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Text(""), GROUP, title);
        spec.help = help;
        spec.advanced = true;
        spec.kept_on_reset = true;
        d.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.editor = Some(EditorDecl::Screen("home"));
        d.extra(extra)?;
    }
    Ok(())
}

/// The screens: the home (the server's account, sync, the slideshow), the
/// slideshow and the sleep screen.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("home", "picture-frame.title", HOME).instances(Instances::Single))?;
    // The photo has the whole screen: no row of button hints over it.
    d.screen(
        ScreenDecl::new("show", "picture-frame.slideshow", SHOW)
            .instances(Instances::Single)
            .hints_hidden(),
    )?;
    d.screen(
        ScreenDecl::new("sleep", SLEEP_SCREEN, SLEEP)
            .instances(Instances::Single)
            .hints_hidden(),
    )?;
    d.submit()?;
    Ok(())
}

/// The actions, the places in the tools, the icons, and the sleep screen
/// offered to the slot `core.sleep` below the sleep plugin's own (priority
/// below 0) with its place among the choices of `sleep.show`.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "picture-frame.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new(
        "slideshow",
        "picture-frame.slideshow",
        SLIDESHOW_TOKEN,
    ))?;
    d.action(ActionDecl::new("sync", "picture-frame.sync", SYNC_TOKEN))?;
    let mut open = MenuEntryDecl::new("core.tools", "picture-frame.open");
    open.order = 48;
    d.menu_entry(open)?;
    let mut show = MenuEntryDecl::new("core.tools", "picture-frame.slideshow");
    show.order = 49;
    d.menu_entry(show)?;
    d.icon("picture-frame.open", "camera")?;
    d.icon("picture-frame.slideshow", "clock-play")?;
    d.icon("picture-frame.sync", "refresh")?;
    let mut offer = SlotOfferDecl::new("core.sleep", SLEEP_SCREEN);
    offer.priority = -1;
    d.slot_offer(offer)?;
    d.setting_choice("sleep.show", SLEEP_SCREEN, SLEEP_SCREEN)?;
    d.submit()?;
    Ok(())
}

/// *Photo of the day* (`picture-frame.widget`): its group, the widget (live:
/// asked each minute, the photo changes with the day) and its settings.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.group(GroupDecl::new(WIDGET, WIDGET_GROUP))?;
    let widget = WidgetDecl::new(WIDGET, WIDGET_GROUP, "core.widgets.personal", WIDGET_TOKEN)
        .map_err(|_| DeclareError::TooLong("widget"))?
        .live()
        .cells(2, 2);
    d.widget(widget)?;
    declare_all(d, WIDGET, WIDGET_GROUP, WIDGET_OPTIONS)?;
    d.submit()
}

/// The events: the end of a request (a request is refused without the
/// subscription), the start and the plugin's stopping, the answers of the
/// host to a photo asked for, the settings and the charger, the Wi-Fi, the
/// minute and the device's waking and sleeping.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.submit()?;
    d.subscribe_event(EventKind::Stopping, STOP_TOKEN)?;
    d.submit()?;
    for kind in [
        EventKind::Started,
        EventKind::ImageReady,
        EventKind::SettingChanged,
        EventKind::ChargeChanged,
        EventKind::WifiChanged,
    ] {
        d.subscribe_event(kind, EVENTS_TOKEN)?;
    }
    d.submit()?;
    d.subscribe_event(EventKind::Tick, TICK_TOKEN)?;
    d.submit()?;
    Ok(())
}
