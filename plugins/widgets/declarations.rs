//! Native identity of the widget pack: its widgets, the settings group of
//! each, and the tick that keeps clocks right. Callback tokens are stable
//! within this plugin; every widget's settings are declared here, none at
//! run time.
use paperpaw_sdk::native::declare::{Declarations, DeclareError, EventKind, GroupDecl, WidgetDecl};
pub use paperpaw_ui_kit::widget::options::Opt;
use paperpaw_ui_kit::widget::options::declare_all;

/// The callback token of the minute tick.
pub const TICK: u32 = 1;

/// The widget categories of the host's catalogue.
pub mod category {
    pub const TIME: &str = "core.widgets.time";
}

/// A widget of the pack.
#[derive(Debug, Clone, Copy)]
pub struct Def {
    /// Relative to the plugin: `day` is `widgets.day`.
    pub name: &'static str,
    pub token: u32,
    pub category: &'static str,
    /// Read each minute.
    pub live: bool,
    pub options: &'static [Opt],
}

const DAY_OPTIONS: &[Opt] = &[
    Opt::Align("core.align.left"),
    Opt::Frame("core.frame.none"),
    Opt::Padding,
    Opt::Paper,
    Opt::Ink,
    Opt::Corners,
    Opt::FrameLine,
    Opt::Shadow,
    Opt::Choice(
        "style",
        "widgets.setting.style",
        &[
            "widgets.day.style.weekday",
            "widgets.day.style.number",
            "widgets.day.style.sheet",
        ],
        "widgets.day.style.weekday",
        "widgets.day.style.help",
    ),
    Opt::Size("core.size.fit", true),
    Opt::DateFormat("widgets.date-format.long"),
    Opt::Part("year", "widgets.setting.year", true),
    Opt::Part("week", "widgets.setting.week-number", false),
];

/// Every widget of the pack.
pub const WIDGETS: &[Def] = &[Def {
    name: "day",
    token: 100,
    category: category::TIME,
    live: true,
    options: DAY_OPTIONS,
}];

/// The widgets and their settings groups and settings.
pub fn widgets<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for widget in WIDGETS {
        let group = ["widgets.", widget.name].concat();
        let title = ["widgets.", widget.name].concat();
        d.group(GroupDecl::new(widget.name, &title))?;
        let mut spec = WidgetDecl::new(widget.name, &title, widget.category, widget.token)
            .map_err(|_| DeclareError::TooLong("widget"))?;
        if widget.live {
            spec = spec.live();
        }
        d.widget(
            spec.previews(&group)
                .map_err(|_| DeclareError::TooLong("widget preview"))?,
        )?;
        declare_all(d, widget.name, &group, widget.options)?;
        d.submit()?;
    }
    Ok(())
}

/// The minute tick, for widgets that show the time.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Tick, TICK)?;
    d.submit()?;
    Ok(())
}
