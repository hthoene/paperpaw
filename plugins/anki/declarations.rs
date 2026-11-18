//! What the Anki plugin declares: the settings (the computer's address and
//! port, the deck, the new cards, syncing when the screen opens), the three
//! screens (the set-up and sync, the deck choice, the study), the actions, the
//! place in the tools, the icons and the events it reads.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, Instances, Kind,
    MenuEntryDecl, ScreenDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const SYNC_TOKEN: u32 = 2;
/// The end of a request.
pub const HTTP_TOKEN: u32 = 3;
/// The plugin stops: a sync under way is cancelled.
pub const STOP_TOKEN: u32 = 4;
/// The screens.
pub const HOME: u32 = 10;
pub const DECKS: u32 = 11;
pub const STUDY: u32 = 12;

/// The settings' group.
pub const GROUP: &str = "core.group.library";

/// The settings: where Anki is, which deck, how many new cards, and whether
/// opening the screen syncs. The API key is a secret, kept apart.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    for (name, title, help) in [
        ("host", "anki.host", "anki.host.help"),
        ("deck", "anki.deck", "anki.deck.help"),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Text(""), GROUP, title);
        spec.help = help;
        spec.kept_on_reset = true;
        d.spec(spec)?;
        let mut extra = ExtraDecl::new(name);
        extra.editor = Some(EditorDecl::Screen(if name == "deck" {
            "decks"
        } else {
            "home"
        }));
        d.extra(extra)?;
    }
    let mut port = SpecDecl::new(
        "port",
        Kind::Int {
            default: 8765,
            min: 1,
            max: 65535,
        },
        GROUP,
        "anki.port",
    );
    port.help = "anki.port.help";
    port.kept_on_reset = true;
    port.advanced = true;
    d.spec(port)?;
    let mut new = SpecDecl::new(
        "new-cards",
        Kind::Int {
            default: 10,
            min: 0,
            max: 50,
        },
        GROUP,
        "anki.new-cards",
    );
    new.help = "anki.new-cards.help";
    d.spec(new)?;
    let mut extra = ExtraDecl::new("new-cards");
    extra.unit = "anki.unit.cards";
    d.extra(extra)?;
    let mut open = SpecDecl::new(
        "sync-on-open",
        Kind::Bool(false),
        GROUP,
        "anki.sync-on-open",
    );
    open.help = "anki.sync-on-open.help";
    d.spec(open)?;
    d.submit()?;
    Ok(())
}

/// The screens: the home (set-up, sync, study), the deck choice and the study.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("home", "anki.title", HOME).instances(Instances::Single))?;
    d.screen(ScreenDecl::new("decks", "anki.deck", DECKS).instances(Instances::Single))?;
    d.screen(ScreenDecl::new("study", "anki.study", STUDY).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The actions, the place in the tools and the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "anki.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new("sync", "anki.sync", SYNC_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "anki.open");
    entry.order = 47;
    d.menu_entry(entry)?;
    d.icon("anki.open", "stack-2")?;
    d.icon("anki.sync", "refresh")?;
    d.submit()?;
    Ok(())
}

/// The events: the end of a request (a request is refused without the
/// subscription) and the plugin's stopping.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.submit()?;
    d.subscribe_event(EventKind::Stopping, STOP_TOKEN)?;
    d.submit()?;
    Ok(())
}
