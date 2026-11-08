//! Native identity of Hints: its two settings, its two screens and the events
//! it follows. Callback tokens are stable within this plugin.
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, Kind, ScreenDecl, SpecDecl,
};

/// Screen tokens, as the plugin's callbacks take them.
pub const OVERLAY: u32 = 10;
pub const AGAIN: u32 = 11;
/// Event tokens.
pub const SHOWN: u32 = 2;
pub const READY: u32 = 3;

const SECTION: &str = "help.section";

/// `help.show` (hints on or off) and `help.again` (its editor shows them once more).
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let system = groups::SYSTEM;
    let mut show = SpecDecl::new(
        "show",
        Kind::Bool(true),
        system.as_str(),
        "help.setting.show",
    );
    show.help = "help.setting.show.help";
    d.spec(show)?;
    let mut again = SpecDecl::new(
        "again",
        Kind::Text(""),
        system.as_str(),
        "help.setting.again",
    );
    again.help = "help.setting.again.help";
    d.spec(again)?;
    let mut extra = ExtraDecl::new("show");
    extra.section = SECTION;
    d.extra(extra)?;
    let mut extra = ExtraDecl::new("again");
    extra.section = SECTION;
    extra.editor = Some(EditorDecl::Screen("again"));
    d.extra(extra)?;
    d.submit()?;
    Ok(())
}

/// The hints over a screen (it shows through) and *Show hints again*.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("overlay", "help.title", OVERLAY).overlay())?;
    d.screen(ScreenDecl::new("again", "help.setting.again", AGAIN))?;
    d.submit()?;
    Ok(())
}

/// A screen shown, or ready for its hints: they may open.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::ScreenShown, SHOWN)?;
    d.subscribe_event(EventKind::ScreenReady, READY)?;
    d.submit()?;
    Ok(())
}
