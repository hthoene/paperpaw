//! The screen lock's native declarations: its page of settings (the kind of
//! lock, whether waking up asks for it, the touch lock's switch), the lock
//! screen, the action *Lock now* and the events that lock the device. The
//! page's icon and the first-run step are declarable but the host refuses them
//! until it takes icons and setup steps; the menu entries, the input filter and
//! the status item of the touch lock follow with their host pieces
//! (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl, Kind,
    ScreenDecl, SpecDecl,
};

/// Callback tokens, as the plugin's callbacks take them.
pub const NOW: u32 = 1;
pub const STARTED: u32 = 2;
pub const RESUME: u32 = 3;
/// The lock screen.
pub const SCREEN: u32 = 20;

/// The page's group, relative to the plugin; in full `lock.group`.
const GROUP: &str = "group";
const GROUP_FULL: &str = "lock.group";

/// The kinds of lock as the setting stores them.
pub const KIND_VALUES: [&str; 4] = [
    "lock.kind.off",
    "lock.kind.pin",
    "lock.kind.pattern",
    "lock.kind.password",
];

/// The page of the lock's settings.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new(GROUP, "lock.plugin.name");
    group.order = 92;
    d.group(group)?;
    let mut kind = SpecDecl::new(
        "kind",
        Kind::Choice {
            default: KIND_VALUES[0],
            choices: &KIND_VALUES,
        },
        GROUP_FULL,
        "lock.setting.kind",
    );
    kind.help = "lock.setting.kind.help";
    kind.guarded = true;
    d.spec(kind)?;
    // The kind and its code are set on the lock's own screen.
    let mut editor = ExtraDecl::new("kind");
    editor.editor = Some(EditorDecl::Screen("screen"));
    d.extra(editor)?;
    let mut on_wake = SpecDecl::new(
        "on-wake",
        Kind::Bool(true),
        GROUP_FULL,
        "lock.setting.on-wake",
    );
    on_wake.help = "lock.setting.on-wake.help";
    on_wake.guarded = true;
    d.spec(on_wake)?;
    let mut touch = SpecDecl::new("touch", Kind::Bool(false), GROUP_FULL, "lock.touch");
    touch.help = "lock.touch.help";
    d.spec(touch)?;
    d.submit()?;
    Ok(())
}

/// The lock screen: one per params (to unlock, or to set up), its content secret
/// and the edges held; it guards the device while it unlocks
/// (`command.screen_policy`).
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(
        ScreenDecl::new("screen", "lock.setting.kind", SCREEN)
            .secret()
            .holds_edges(),
    )?;
    d.submit()?;
    Ok(())
}

/// *Lock now*: locks at once, from any button, gesture, menu or rule.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("now", "lock.now", NOW))?;
    d.submit()?;
    Ok(())
}

/// What locks the device: the start and waking up. (The kind setting changing
/// is not subscribed to: the host refuses that subscription for now, see
/// `PORTING-7.md`.)
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED)?;
    d.subscribe_event(EventKind::Resume, RESUME)?;
    d.submit()?;
    Ok(())
}
