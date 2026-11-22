//! The screen lock's native declarations: its page of settings (the kind of
//! lock, whether waking up asks for it, the touch lock's switch), the lock
//! screen, the action *Lock now*, the events that lock the device and the
//! first-run setup's step (the kind of lock, as in Beta 3). The touch lock's actions,
//! menu entries, key, status item and input filter (`touch_ui`, `touch_events`)
//! are declared everywhere (`make` cannot tell whether there is touch): the
//! entries carry the need for touch, so a device without it shows none, and the
//! filter passes everything that is not a finger.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, GroupDecl,
    KeyBindingDecl, Kind, MenuEntryDecl, ScreenDecl, SetupPage, SetupStepDecl, SpecDecl,
    StatusItemDecl, SubscribeDecl,
};

/// Callback tokens, as the plugin's callbacks take them.
pub const NOW: u32 = 1;
pub const STARTED: u32 = 2;
pub const RESUME: u32 = 3;
/// The lock screen.
pub const SCREEN: u32 = 20;
/// The touch lock: the toggle, *Lock touch* (the entry in the menus), the hint,
/// the input filter and the status item.
pub const TOUCH_TOGGLE: u32 = 30;
pub const TOUCH_LOCK: u32 = 31;
pub const TOUCH_HINT: u32 = 32;
pub const TOUCH_FILTER: u32 = 33;
pub const TOUCH_STATUS: u32 = 34;

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
    d.icon(GROUP_FULL, "lock")?;
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
    // Devices without a touch screen do not show a switch for the touch lock.
    d.menu_need("lock.touch", MenuNeed::Touch)?;
    // The first-run setup offers a lock, off unless one is chosen on its screen.
    let mut step = SetupStepDecl::new(
        "setup",
        "lock.setting.kind",
        SetupPage::Setting("lock.kind"),
    );
    step.order = 70;
    d.setup_step(step)?;
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
    d.icon("lock.now", "lock")?;
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

/// The touch lock's actions, its entries in the quick panel and the reader's
/// menu, the home key held and the status item. *Lock touch* is an action, not
/// the setting's switch: a switch leaves the panel open, and a locked touch
/// cannot close it. The entries need touch.
pub fn touch_ui<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new(
        "touch-toggle",
        "lock.touch-toggle",
        TOUCH_TOGGLE,
    ))?;
    d.action(ActionDecl::new("touch-lock", "lock.touch", TOUCH_LOCK))?;
    d.action(ActionDecl::new("touch-hint", "lock.touch-hint", TOUCH_HINT))?;
    d.icon("lock.touch-toggle", "lock")?;
    d.icon("lock.touch-lock", "lock")?;
    // Beside night mode, in the quick panel and the reader's menu.
    let mut quick = MenuEntryDecl::new("core.quick", "lock.touch-lock");
    quick.order = 11;
    d.menu_entry(quick)?;
    let mut reader = MenuEntryDecl::new("core.reader-menu", "lock.touch-lock");
    reader.order = 60;
    d.menu_entry(reader)?;
    d.menu_need("lock.touch-lock", MenuNeed::Touch)?;
    d.menu_need("lock.touch-toggle", MenuNeed::Touch)?;
    // A way out without touch: while it is locked, the home key held (the X4
    // Pro's key below the screen) unlocks. The power button held does too (the
    // power plugin).
    let mut escape = KeyBindingDecl::new("global", "long.home", "lock.touch-toggle");
    escape.when_key = "lock.touch";
    escape.when_value = "true";
    d.key_binding(escape)?;
    d.status_item(
        StatusItemDecl::new("touch-status", "lock.touch-status", TOUCH_STATUS)
            .map_err(|_| DeclareError::Empty("status item"))?,
    )?;
    d.submit()?;
    Ok(())
}

/// The touch lock's input filter and the status stage it answers.
pub fn touch_events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe(SubscribeDecl::new("core.input-filter", TOUCH_FILTER))?;
    d.subscribe(SubscribeDecl::new("core.status", TOUCH_STATUS))?;
    d.submit()?;
    Ok(())
}
