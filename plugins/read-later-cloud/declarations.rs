//! What the Readwise Reader plugin declares, compared with the interface-6
//! registration by the declaration tests: the setting for where articles
//! are fetched from, the set-up screen, the two actions, the place in the
//! tools, the icons, and the events it reads: the end of a request, the
//! reader's page turns, the work that is due and the plugin's stopping.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, Instances, Kind, MenuEntryDecl, ScreenDecl,
    SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const FETCH_TOKEN: u32 = 2;
/// The end of a request (`core.net`).
pub const HTTP_TOKEN: u32 = 3;
/// The reader's page turns: an article read to the end is noted.
pub const PAGE_TOKEN: u32 = 4;
/// The plugin stops: a fetch under way is cancelled and its files go.
pub const STOP_TOKEN: u32 = 5;
/// The work that fetches, when it is due.
pub const WORK_TOKEN: u32 = 6;
pub const SCREEN: u32 = 10;

/// The places articles are fetched from, as the setting's values.
pub const FROM_LATER: &str = "read-later-cloud.from.later";
pub const FROM_INBOX: &str = "read-later-cloud.from.inbox";
const FROM: [&str; 2] = [FROM_LATER, FROM_INBOX];

/// Where the articles are fetched from: the *Later* list or the *Inbox*.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut from = SpecDecl::new(
        "from",
        Kind::Choice {
            default: FROM_LATER,
            choices: &FROM,
        },
        "core.group.library",
        "read-later-cloud.from",
    );
    from.help = "read-later-cloud.from.help";
    from.kept_on_reset = true;
    d.spec(from)?;
    d.submit()?;
    Ok(())
}

/// The set-up screen, one at a time.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(
        ScreenDecl::new("setup", "read-later-cloud.title", SCREEN).instances(Instances::Single),
    )?;
    d.submit()?;
    Ok(())
}

/// The actions, the place in the tools and the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new(
        "open",
        "read-later-cloud.title",
        OPEN_TOKEN,
    ))?;
    d.action(ActionDecl::new(
        "fetch",
        "read-later-cloud.fetch",
        FETCH_TOKEN,
    ))?;
    let mut entry = MenuEntryDecl::new("core.tools", "read-later-cloud.open");
    entry.order = 49;
    d.menu_entry(entry)?;
    d.icon("read-later-cloud.open", "article")?;
    d.icon("read-later-cloud.fetch", "refresh")?;
    d.submit()?;
    Ok(())
}

/// The events the plugin reads.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.subscribe_event(EventKind::PageTurned, PAGE_TOKEN)?;
    d.subscribe_event(EventKind::WorkDue, WORK_TOKEN)?;
    d.subscribe_event(EventKind::Stopping, STOP_TOKEN)?;
    d.submit()?;
    Ok(())
}
