//! What Edge light declares: its settings (what each edge sets, how wide the
//! strips are), the action that switches the front light on or off, the home
//! screen widget with its look, and the events and the input filter it
//! subscribes to. `build.rs` turns it into the image's batches.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, GroupDecl, Kind, SpecDecl,
    SubscribeDecl, WidgetDecl,
};
use paperpaw_ui_kit::widget::options::{declare_all, shared_look};

/// The token of the setting changes.
pub const LIFECYCLE_TOKEN: u32 = 1;
/// The token of the input filter.
pub const FILTER_TOKEN: u32 = 2;
/// The action that switches the light on or off.
pub const TOGGLE_TOKEN: u32 = 3;
/// The action the filter runs for each step of a swipe, which sets the level (a
/// filter sends no command itself).
pub const STEP_TOKEN: u32 = 4;
#[allow(
    dead_code,
    reason = "only the plugin names the action; the build script sees the declarations alone"
)]
pub const STEP_ACTION: &str = "edge-light.step";
/// The widget.
pub const WIDGET_TOKEN: u32 = 20;

/// The stage every finger and button goes through first.
pub const FILTER_STAGE: &str = "core.input-filter";

/// The settings group, as the settings name it.
const GROUP: &str = "edge-light.group";

/// The settings by their full ids.
pub const LEFT: &str = "edge-light.left";
pub const RIGHT: &str = "edge-light.right";
pub const WIDTH: &str = "edge-light.width";

/// What an edge sets: the values of the choices.
pub const FN_BRIGHTNESS: &str = "edge-light.fn.brightness";
pub const FN_WARMTH: &str = "edge-light.fn.warmth";
pub const FN_OFF: &str = "edge-light.fn.off";
pub const FUNCTIONS: [&str; 3] = [FN_BRIGHTNESS, FN_WARMTH, FN_OFF];

/// The strip's width in percent of the screen's: the default and the bounds.
pub const WIDTH_DEFAULT: i64 = 8;
pub const WIDTH_MIN: i32 = 3;
pub const WIDTH_MAX: i32 = 20;

/// The settings page, shown only where a touch screen and a front light are.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", GROUP);
    group.order = 91;
    d.group(group)?;
    d.icon(GROUP, "sun")?;
    for (name, default, title, help) in [
        (
            "left",
            FN_BRIGHTNESS,
            "edge-light.setting.left",
            "edge-light.setting.left.help",
        ),
        (
            "right",
            FN_WARMTH,
            "edge-light.setting.right",
            "edge-light.setting.right.help",
        ),
    ] {
        let mut spec = SpecDecl::new(
            name,
            Kind::Choice {
                default,
                choices: &FUNCTIONS,
            },
            GROUP,
            title,
        );
        spec.help = help;
        d.spec(spec)?;
    }
    let mut width = SpecDecl::new(
        "width",
        Kind::Int {
            default: WIDTH_DEFAULT,
            min: WIDTH_MIN,
            max: WIDTH_MAX,
        },
        GROUP,
        "edge-light.setting.width",
    );
    width.help = "edge-light.setting.width.help";
    d.spec(width)?;
    let mut extra = ExtraDecl::new("width");
    extra.unit = "core.unit.percent";
    d.extra(extra)?;
    // A page for a device with a front light, whose settings need a touch screen
    // as well (a need an item has is one: the page is empty, so hidden, without).
    d.menu_need(GROUP, MenuNeed::Frontlight)?;
    for item in [LEFT, RIGHT, WIDTH] {
        d.menu_need(item, MenuNeed::Touch)?;
    }
    d.submit()
}

/// The action *Light on or off*, for key maps, menus and rules.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new(
        "toggle",
        "edge-light.action.toggle",
        TOGGLE_TOKEN,
    ))?;
    d.action(ActionDecl::new(
        "step",
        "edge-light.action.step",
        STEP_TOKEN,
    ))?;
    d.icon("edge-light.toggle", "sun")?;
    d.menu_need("edge-light.toggle", MenuNeed::Frontlight)?;
    d.submit()
}

/// The widget *Front light*: the level, a tap switches the light on or off.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let name = "light";
    let group = ["edge-light.", name].concat();
    d.group(GroupDecl::new(name, "edge-light.light"))?;
    let spec = WidgetDecl::new(
        name,
        "edge-light.light",
        "core.widgets.device",
        WIDGET_TOKEN,
    )
    .map_err(|_| DeclareError::TooLong("widget"))?
    .cells(2, 1);
    d.widget(spec)?;
    declare_all(d, name, &group, shared_look("core.align.center"))?;
    d.submit()
}

/// The events the plugin watches (a setting that changed: the widget shows
/// the new level, the strips the new width) and the input filter.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::SettingChanged, LIFECYCLE_TOKEN)?;
    d.subscribe(SubscribeDecl::new(FILTER_STAGE, FILTER_TOKEN))?;
    d.submit()
}
