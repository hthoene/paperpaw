//! Mail's declarations as borrowed builders, shared by the build script (which
//! writes them into the plugin image) and the tests.
//!
//! The settings are the interface-6 ones: the group *Mail* with a page for each
//! of the three accounts, and three settings for all of them. The group is
//! `mail.group` (interface 6: `mail`), as every ported plugin names its group.
//! The home widgets' settings come with the widgets (host call: Core todo 76).
//! The actions, the Tools entry, the icons and the screens are declared by
//! [`ui`] and [`screens`] and are not part of the image yet: the screens wait for
//! the sockets, the secrets and the work services (see `PORTING-7.md`).
use paperpaw_sdk::builtin::menus;
use paperpaw_sdk::native::declare::{
    ActionDecl, Declarations, DeclareError, EventKind, ExtraDecl, GroupDecl, Instances, Kind,
    MenuEntryDecl, ScreenDecl, SpecDecl,
};

/// Callback tokens are stable within this plugin image.
pub const OPEN_TOKEN: u32 = 10;
pub const WRITE_TOKEN: u32 = 11;
/// The token of the socket's events (the connection to a server).
pub const SOCKET_TOKEN: u32 = 2;
/// The first token of the screens.
pub const SCREEN_TOKEN: u32 = 20;

/// The group of the plugin, and the parts of it for each account.
pub const GROUP: &str = "mail.group";
/// How many accounts there are.
pub const ACCOUNTS: u8 = 3;

/// The options of the provider setting (translation keys, and the values).
pub const PROVIDERS: [&str; 11] = [
    "mail.provider.own",
    "mail.provider.gmail",
    "mail.provider.outlook",
    "mail.provider.icloud",
    "mail.provider.yahoo",
    "mail.provider.gmx",
    "mail.provider.webde",
    "mail.provider.tonline",
    "mail.provider.posteo",
    "mail.provider.mailbox",
    "mail.provider.fastmail",
];
/// The options of the two security settings.
pub const SECURITIES: [&str; 2] = ["mail.security.implicit", "mail.security.starttls"];

/// The settings of one account `$n`, in the group `$group`: a provider to
/// start from, the address, the password (a secret), the name, and under
/// advanced the user name and the servers with their ports, securities and
/// pinned keys. Names are literals so that they live in the program.
macro_rules! account {
    ($d:expr, $n:literal, $group:literal) => {{
        // The reader's own, not a matter of taste: reset leaves them.
        let mut spec = SpecDecl::new(
            concat!($n, ".provider"),
            Kind::Choice {
                default: PROVIDERS[0],
                choices: &PROVIDERS,
            },
            $group,
            "mail.provider",
        );
        spec.kept_on_reset = true;
        spec.help = "mail.provider.help";
        $d.spec(spec)?;
        text!(
            $d,
            concat!($n, ".address"),
            $group,
            "mail.address",
            "mail.address.help",
            false
        );
        let mut password = SpecDecl::new(
            concat!($n, ".password"),
            Kind::Text(""),
            $group,
            "mail.password",
        );
        password.secret = true;
        password.help = "mail.password.help";
        $d.spec(password)?;
        text!(
            $d,
            concat!($n, ".name"),
            $group,
            "mail.name",
            "mail.name.help",
            false
        );
        text!(
            $d,
            concat!($n, ".user"),
            $group,
            "mail.user",
            "mail.user.help",
            true
        );
        text!(
            $d,
            concat!($n, ".imap-server"),
            $group,
            "mail.imap-server",
            "",
            true
        );
        port!($d, concat!($n, ".imap-port"), $group, 993);
        security!(
            $d,
            concat!($n, ".imap-security"),
            $group,
            "mail.imap-security"
        );
        text!(
            $d,
            concat!($n, ".imap-pin"),
            $group,
            "mail.pin",
            "mail.pin.help",
            true
        );
        text!(
            $d,
            concat!($n, ".smtp-server"),
            $group,
            "mail.smtp-server",
            "",
            true
        );
        port!($d, concat!($n, ".smtp-port"), $group, 465);
        security!(
            $d,
            concat!($n, ".smtp-security"),
            $group,
            "mail.smtp-security"
        );
        text!(
            $d,
            concat!($n, ".smtp-pin"),
            $group,
            "mail.pin",
            "mail.pin.help",
            true
        );
    }};
}

/// A text setting the reader keeps through a reset.
macro_rules! text {
    ($d:expr, $name:expr, $group:expr, $title:expr, $help:expr, $advanced:expr) => {{
        let mut spec = SpecDecl::new($name, Kind::Text(""), $group, $title);
        spec.kept_on_reset = true;
        spec.advanced = $advanced;
        spec.help = $help;
        $d.spec(spec)?;
    }};
}

/// A port: 0 to 65535, advanced.
macro_rules! port {
    ($d:expr, $name:expr, $group:expr, $default:expr) => {{
        let mut spec = SpecDecl::new(
            $name,
            Kind::Int {
                default: $default,
                min: 0,
                max: 65535,
            },
            $group,
            "mail.port",
        );
        spec.kept_on_reset = true;
        spec.advanced = true;
        $d.spec(spec)?;
    }};
}

/// How a server is spoken to: implicit TLS or STARTTLS, advanced.
macro_rules! security {
    ($d:expr, $name:expr, $group:expr, $title:expr) => {{
        let mut spec = SpecDecl::new(
            $name,
            Kind::Choice {
                default: SECURITIES[0],
                choices: &SECURITIES,
            },
            $group,
            $title,
        );
        spec.kept_on_reset = true;
        spec.advanced = true;
        $d.spec(spec)?;
    }};
}

/// The group, the pages of the accounts, and every setting.
pub fn settings(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut d = Declarations::new(sink);
    let mut group = GroupDecl::new("group", "mail.group");
    group.order = 27;
    d.group(group)?;
    for (order, name, title) in [
        (1, "account.1", "mail.account.1"),
        (2, "account.2", "mail.account.2"),
        (3, "account.3", "mail.account.3"),
    ] {
        let mut part = GroupDecl::new(name, title);
        part.parent = GROUP;
        part.order = order;
        d.group(part)?;
    }
    account!(d, "1", "mail.account.1");
    account!(d, "2", "mail.account.2");
    account!(d, "3", "mail.account.3");
    let mut fetch = SpecDecl::new(
        "fetch",
        Kind::Int {
            default: 20,
            min: 5,
            max: 50,
        },
        GROUP,
        "mail.fetch",
    );
    fetch.help = "mail.fetch.help";
    d.spec(fetch)?;
    let mut step = ExtraDecl::new("fetch");
    step.step = 5;
    d.extra(step)?;
    let mut mark = SpecDecl::new("mark-read", Kind::Bool(true), GROUP, "mail.mark-read");
    mark.help = "mail.mark-read.help";
    d.spec(mark)?;
    let mut signature = SpecDecl::new("signature", Kind::Text(""), GROUP, "mail.signature");
    signature.help = "mail.signature.help";
    d.spec(signature)?;
    d.submit()
}

/// The actions *open* and *write*, the Tools entry (order 47) and the icons.
pub fn ui(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut d = Declarations::new(sink);
    d.action(ActionDecl::new("open", "mail.title", OPEN_TOKEN))?;
    d.action(ActionDecl::new("write", "mail.write", WRITE_TOKEN))?;
    let tools = menus::TOOLS;
    let mut entry = MenuEntryDecl::new(tools.as_str(), "mail.open");
    entry.order = 47;
    d.menu_entry(entry)?;
    d.icon("mail.open", "inbox")?;
    d.icon("mail.write", "pencil")?;
    d.submit()
}

/// The screens, in the order interface 6 registered them: the folders and the
/// compose window open once, the lists and the readers once for their
/// parameters.
pub fn screens<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("boxes", "mail.title", SCREEN_TOKEN).instances(Instances::Single))?;
    d.screen(ScreenDecl::new("list", "mail.title", SCREEN_TOKEN + 1))?;
    d.screen(ScreenDecl::new("read", "mail.title", SCREEN_TOKEN + 2))?;
    d.screen(
        ScreenDecl::new("compose", "mail.write", SCREEN_TOKEN + 3).instances(Instances::Single),
    )?;
    d.submit()
}

/// What the connection to a server tells: the events of its socket.
pub fn events(sink: impl FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError> {
    let mut declarations = Declarations::new(sink);
    declarations.subscribe_event(EventKind::Socket, SOCKET_TOKEN)?;
    declarations.submit()
}
