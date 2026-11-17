//! The tool's own plugin in the app: a widget that draws nothing and
//! notes the area it is given, so that checks know a widget's cell
//! exactly (the grid gives every widget of a size the same area); and a
//! screen that notes the settings' descriptions their rows would cut
//! ([`HELP`]), measured with the device's own letters.
//!
//! Sessions add it besides the plugins being checked (see [`add`]); the
//! checks leave its namespace, [`PLUGIN`], out.
//!
//! What it keeps is the thread's: a session builds and runs its devices
//! on one thread, and sessions on other threads (tests run side by side)
//! must not change which plugins a device gets or the area it notes.

use std::cell::{Cell, RefCell};

use paperpaw_core::{Items, KernelBuilder};
use paperpaw_platform::Rect;
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{Widget, WidgetSpec};
use paperpaw_sdk::{Canvas, Cx, Env, Handled, Id, Manifest, RegisterError, Version};

use crate::AddPlugins;

/// The tool's plugin id, left out of every check.
pub const PLUGIN: &str = "paperpaw-dev";

/// The widget that notes its area.
pub const WIDGET: Id = Id::new_static("paperpaw-dev.probe");
/// The screen that notes the descriptions their rows cut.
pub const HELP: Id = Id::new_static("paperpaw-dev.help");

thread_local! {
    /// The area the probe was last drawn in.
    static AREA: Cell<Option<Rect>> = const { Cell::new(None) };
    /// The plugins of the target, added after the probe (see [`add`]).
    static TARGET: Cell<Option<AddPlugins>> = const { Cell::new(None) };
    /// The settings whose description their row cuts, with it, as [`HELP`]
    /// last found them.
    static CUT_HELP: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// The settings whose description a row cuts (id, text), as [`HELP`]
/// found them when it was last drawn.
pub fn cut_help() -> Vec<(String, String)> {
    CUT_HELP.take()
}

/// Sets what [`add`] adds besides the probe, on this thread.
pub fn set_target(add: Option<AddPlugins>) {
    TARGET.set(add);
}

/// Adds the probe and the target's plugins (see [`set_target`]).
pub fn add(builder: KernelBuilder) -> KernelBuilder {
    let builder = builder.items(probe_manifest(), probe());
    match TARGET.get() {
        Some(add) => add(builder),
        None => builder,
    }
}

/// Forgets the area noted, before the probe is drawn anew.
pub fn forget() {
    AREA.set(None);
}

/// The area the probe was drawn in since [`forget`].
pub fn area() -> Option<Rect> {
    AREA.get()
}

fn probe() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    |r| {
        r.add_widget(WidgetSpec::new(WIDGET, "paperpaw-dev.probe", |_| {
            Box::new(Noting)
        }))?;
        r.add_fake(paperpaw_core::testing::FakeSpec::new(
            HELP,
            "paperpaw-dev.help",
            Box::new(|_, _| Box::new(Measuring)),
        ))
    }
}

/// What [`probe`] registers under.
fn probe_manifest() -> Manifest {
    Manifest::new(
        Id::new_static(PLUGIN),
        "paperpaw-dev",
        Version::new(0, 1, 0),
    )
    .description("Measures where widgets are drawn, for paperpaw-dev.")
    .uses(paperpaw_sdk::Uses::NOTHING)
}

/// Measures every setting's description against its row's lines.
struct Measuring;

impl paperpaw_core::testing::Fake for Measuring {
    fn draw(&mut self, _: &mut Canvas<'_>, env: &Env<'_>) {
        let style = UiStyle::new(env);
        let width = env.device().display.width;
        let cut: Vec<(String, String)> = env
            .settings()
            .specs()
            .filter_map(|spec| {
                let text = env.i18n().tr(spec.description.as_deref()?);
                (!ui::note_fits(env, &style, width, text))
                    .then(|| (spec.id.to_string(), text.to_string()))
            })
            .collect();
        CUT_HELP.set(cut);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

struct Noting;

impl Widget for Noting {
    fn live(&self) -> bool {
        false
    }

    fn height(&self, _: &Env<'_>, _: u16) -> u16 {
        1
    }

    fn draw(&self, _: &mut Canvas<'_>, _: &Env<'_>, area: Rect) {
        AREA.set(Some(area));
    }
}
