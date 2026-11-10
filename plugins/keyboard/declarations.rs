//! What the keyboard plugin declares, built into its image by `build.rs` and
//! held to it by the declaration tests.
use paperpaw_sdk::MenuNeed;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, ExtraDecl, GroupDecl, Instances,
    KeyBindingDecl, Kind, ScreenDecl, SetupPage, SetupStepDecl, SlotOfferDecl, SpecDecl, StageDecl,
};

/// Callback tokens: stable within this plugin.
pub const SCREEN: u32 = 10;
/// The screen that chooses the layouts.
pub const CHOOSE: u32 = 11;
/// The action that finishes typing.
pub const DONE: u32 = 1;

/// The `keyboard.layouts` bounds, as `stages/keyboard.layouts@1.def` says (a
/// test holds them to the generated codec).
pub const LAYOUTS_SCHEMA: u32 = 1;
pub const LAYOUTS_MAX_INPUT: u32 = 64;
pub const LAYOUTS_MAX_REPLY: u32 = 3200;
pub const LAYOUTS_MAX_RECORDS: u32 = 3;

/// The stage other plugins add keyboard layouts through: a plugin that brings
/// a layout subscribes to `keyboard.layouts` (`registration.stage`) and
/// answers each run with `layouts.layout` records.
pub fn stages<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.stage(StageDecl::new(
        "layouts",
        LAYOUTS_SCHEMA,
        LAYOUTS_MAX_INPUT,
        LAYOUTS_MAX_REPLY,
        LAYOUTS_MAX_RECORDS,
    ))?;
    d.submit()
}

/// The keyboard's settings, on a page of their own (Settings, Device).
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new("group", "keyboard.title");
    group.order = 72;
    d.group(group)?;
    let mut layouts = SpecDecl::new(
        "layouts",
        Kind::List("keyboard.layout.auto"),
        "keyboard.group",
        "keyboard.setting.layouts",
    );
    layouts.help = "keyboard.setting.layouts.help";
    d.spec(layouts)?;
    let mut extra = ExtraDecl::new("layouts");
    extra.editor = Some(EditorDecl::Screen("choose"));
    d.extra(extra)?;
    let mut umlauts = SpecDecl::new(
        "umlaut-keys",
        Kind::Bool(false),
        "keyboard.group",
        "keyboard.setting.umlaut-keys",
    );
    umlauts.help = "keyboard.setting.umlaut-keys.help";
    d.spec(umlauts)?;
    let mut shape = SpecDecl::new(
        "key-shape",
        Kind::Choice {
            default: "keyboard.key-shape.round",
            choices: &[
                "keyboard.key-shape.round",
                "keyboard.key-shape.tall",
                "keyboard.key-shape.wide",
            ],
        },
        "keyboard.group",
        "keyboard.setting.key-shape",
    );
    shape.help = "keyboard.setting.key-shape.help";
    d.spec(shape)?;
    let mut size = SpecDecl::new(
        "size",
        Kind::Choice {
            default: "keyboard.size.medium",
            choices: &[
                "keyboard.size.small",
                "keyboard.size.medium",
                "keyboard.size.large",
            ],
        },
        "keyboard.group",
        "keyboard.setting.size",
    );
    size.help = "keyboard.setting.size.help";
    d.spec(size)?;
    let mut capitals = SpecDecl::new(
        "auto-capitals",
        Kind::Bool(false),
        "keyboard.group",
        "keyboard.setting.auto-capitals",
    );
    capitals.help = "keyboard.setting.auto-capitals.help";
    d.spec(capitals)?;
    d.submit()
}

/// The keyboard and the screen that chooses its layouts.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    // Over the screen below; several may be open (a phone question over it).
    d.screen(
        ScreenDecl::new("screen", "keyboard.title", SCREEN)
            .overlay()
            .instances(Instances::Many),
    )?;
    d.screen(ScreenDecl::new(
        "choose",
        "keyboard.setting.layouts",
        CHOOSE,
    ))?;
    d.submit()
}

/// The slot, the action held Back runs, the icon, the first-run step.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.slot_offer(SlotOfferDecl::new("core.keyboard", "keyboard.screen"))?;
    d.icon("keyboard.group", "keyboard")?;
    // Held Back finishes: elsewhere it goes home, which here would only
    // close the keyboard and lose the text.
    let mut done = ActionDecl::new("done", "keyboard.done-action", DONE);
    done.needs_screen = true;
    d.action(done)?;
    d.key_binding(KeyBindingDecl::new(
        "keyboard.screen",
        "long.back",
        "keyboard.done",
    ))?;
    // Keys chosen with buttons take no shape.
    d.menu_need("keyboard.key-shape", MenuNeed::Touch)?;
    let mut step = SetupStepDecl::new(
        "setup",
        "keyboard.setting.layouts",
        SetupPage::Setting("keyboard.layouts"),
    );
    step.order = 60;
    d.setup_step(step)?;
    d.submit()
}
