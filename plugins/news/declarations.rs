//! What the news plugin declares (slice 1), compared with the interface-6
//! registration by the declaration tests: the setting for how many articles a
//! feed keeps (in the library's settings, in a part named after the plugin),
//! the list of feeds as a screen (one at a time), the actions to open it and
//! to fetch, the place in the tools and the icons. The headlines widget and
//! its settings follow with the widget pack's kit (`PORTING-7.md`).
use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, Instances, Kind, MenuEntryDecl,
    ScreenDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const FETCH_TOKEN: u32 = 2;
pub const SCREEN: u32 = 10;
/// The end of a request (`core.net`).
pub const HTTP_TOKEN: u32 = 4;
/// The work that fetches the news in the background.
pub const WORK_TOKEN: u32 = 5;

/// Articles kept of each feed, in steps of five.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = groups::LIBRARY;
    let mut spec = SpecDecl::new(
        "articles",
        Kind::Int {
            default: 30,
            min: 5,
            max: 200,
        },
        group.as_str(),
        "news.setting.articles",
    );
    spec.help = "news.setting.articles.help";
    spec.advanced = true;
    d.spec(spec)?;
    let mut extra = ExtraDecl::new("articles");
    extra.section = "news.plugin.name";
    extra.step = 5;
    d.extra(extra)?;
    d.submit()?;
    Ok(())
}

/// The list of feeds, one at a time.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("list", "news.title", SCREEN).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The actions, the place in the tools and the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "news.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new("fetch", "news.fetch", FETCH_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "news.open");
    entry.order = 44;
    d.menu_entry(entry)?;
    d.icon("news.open", "rss")?;
    d.icon("news.fetch", "refresh")?;
    d.submit()?;
    Ok(())
}

/// The events the plugin watches: the end of a request and a work that is due.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.subscribe_event(EventKind::WorkDue, WORK_TOKEN)?;
    d.submit()?;
    Ok(())
}
