//! What the Joplin set-up declares, compared with the interface-6
//! registration by the declaration tests: the four settings of the account,
//! all edited on the set-up screen, the screen, the two actions, the place in
//! the tools and the icons.
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EditorDecl, EventKind, ExtraDecl, Instances, Kind,
    MenuEntryDecl, ScreenDecl, SpecDecl,
};

/// Callback tokens: stable within this plugin.
pub const OPEN_TOKEN: u32 = 1;
pub const SEND_TOKEN: u32 = 2;
/// The start: declares the retry of what waits.
pub const STARTED_TOKEN: u32 = 3;
/// The end of a request (`core.net`).
pub const HTTP_TOKEN: u32 = 4;
/// The work that sends what waits, when it is due.
pub const WORK_TOKEN: u32 = 5;
/// A setting changed: the account's, so signed in anew.
pub const SETTING_TOKEN: u32 = 6;
pub const SCREEN: u32 = 10;

/// The kinds of sync target (the setting's values), and the notebook the
/// notes go into unless the setting says otherwise.
pub const SERVICES: [&str; 3] = [
    "joplin.service.cloud",
    "joplin.service.server",
    "joplin.service.webdav",
];
pub const DEFAULT_NOTEBOOK: &str = "PaperPaw";

/// Where the notes go, the address (Joplin Server's, or the WebDAV folder),
/// the e-mail or user name and the notebook, all set on the set-up.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let group = "core.group.library";
    let mut service = SpecDecl::new(
        "service",
        Kind::Choice {
            default: SERVICES[0],
            choices: &SERVICES,
        },
        group,
        "joplin.service",
    );
    service.help = "joplin.service.help";
    service.kept_on_reset = true;
    d.spec(service)?;
    for (name, default, title, help) in [
        ("address", "", "joplin.address", "joplin.address.help"),
        ("user", "", "joplin.user", "joplin.user.help"),
        (
            "notebook",
            DEFAULT_NOTEBOOK,
            "joplin.notebook",
            "joplin.notebook.help",
        ),
    ] {
        let mut spec = SpecDecl::new(name, Kind::Text(default), group, title);
        spec.help = help;
        spec.kept_on_reset = true;
        spec.advanced = true;
        d.spec(spec)?;
    }
    for name in ["service", "address", "user", "notebook"] {
        let mut extra = ExtraDecl::new(name);
        extra.editor = Some(EditorDecl::Screen("setup"));
        d.extra(extra)?;
    }
    d.submit()?;
    Ok(())
}

/// The set-up screen, one at a time.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("setup", "joplin.title", SCREEN).instances(Instances::Single))?;
    d.submit()?;
    Ok(())
}

/// The actions, the place in the tools and the icons.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.action(ActionDecl::new("open", "joplin.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new("send", "joplin.send", SEND_TOKEN))?;
    let mut entry = MenuEntryDecl::new("core.tools", "joplin.open");
    entry.order = 49;
    d.menu_entry(entry)?;
    d.icon("joplin.open", "note")?;
    d.icon("joplin.send", "upload")?;
    d.submit()?;
    Ok(())
}

/// The events the plugin reads: the start, the end of a request, the work
/// that is due and a setting changed. The notes an export writes arrive with
/// the reader's export stage (`PORTING-7.md`).
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED_TOKEN)?;
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.subscribe_event(EventKind::WorkDue, WORK_TOKEN)?;
    d.subscribe_event(EventKind::SettingChanged, SETTING_TOKEN)?;
    d.submit()?;
    Ok(())
}
