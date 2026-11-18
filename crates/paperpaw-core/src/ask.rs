//! What kernel code asks for later, after the code that asks returns
//! (#300, D209): one kernel-private queue of [`Ask`]s ([`Asks`]), applied
//! by the kernel in order (`Kernel::apply`). Native plugins' command
//! records are applied from their wire records; an `Ask` is what the
//! kernel's own code (built-in handlers, services, work, network) asks.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::event::{Event, EventData};
use paperpaw_sdk::{
    Broadcast, HttpRequest, Id, Params, RefreshMode, ServerResponse, SettingValue, SocketRead,
    Speech, Tls,
};

/// A request from a plugin to the kernel.
///
/// Plugins never mutate kernel state directly; commands are queued and
/// applied by the kernel after the plugin returns.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ask {
    /// Shows the screen bound to a slot, passing `Params` to it. If that
    /// slot is already open, the screens above it are closed.
    OpenSlot(Id, Params),
    /// Opens a specific screen on top of the current one.
    OpenScreen(Id, Params),
    /// Closes the current screen (no-op on the root screen).
    Back,
    /// Closes the open screen `.0` wherever it is in the stack, leaving
    /// the screens above it (see `Cx::close`); none
    /// where it is the only one.
    Close(paperpaw_sdk::ScreenKey),
    /// Opens screen `.0` with `.1` in place of the one on top, in one
    /// step: the screen below it is not shown between (the next step of
    /// a guide).
    Replace(Id, Params),
    /// Closes the screens above the topmost open screen `.0`, or the
    /// screen of slot `.0`: back to it in one step. Nothing where none is
    /// open.
    BackTo(Id),
    /// Opens a book with the reading app chosen for it, in
    /// [`slots::READER`](paperpaw_sdk::builtin::slots::READER): its path in
    /// `params::PATH`, and the readers that could not open it. See
    /// `readers::open` and
    /// `readers::cannot_open`, which make
    /// it, and `readers::resolve`, which says
    /// what it does.
    OpenBook(Params),
    /// Opens screen `.1` for slot `.0` with `.2` as [`Ask::OpenSlot`]
    /// opens the slot's own: shown again where the slot is open with it
    /// and these params, else in place of what the slot shows (one
    /// launcher of several, chosen once).
    OpenInSlot(Id, Id, Params),
    /// Runs action `.0`'s own handler with params `.1`, past the handlers
    /// put in its place and the screens: for a handler put in its place
    /// that does more around it. Only from such a handler of action `.0`
    /// (its commands then come where it asked for them); from anywhere
    /// else it is refused. Seen by no observer.
    RunOriginal(Id, Params),
    /// Raises the input `.0` as written in key maps (`button.menu`,
    /// `double-tap.center`, a plugin's `input.<id>`): it runs what the
    /// layers shown bind it to, as if it had come (switch access, chords
    /// of one's own).
    Input(String),
    /// Puts the key map layer `.1` in front of the slot's and the global
    /// layer for open screen `.0`, e.g. a book's own bindings
    /// (`[keymap.<layer>]`): it applies while that screen is on top and
    /// goes with it; not below a screen that guards the device. `None`
    /// takes it away. A screen sends it with
    /// `Cx::set_key_layer`; sent from anywhere
    /// else (a handler, a rule), it is refused.
    SetKeyLayer(paperpaw_sdk::ScreenKey, Option<Id>),
    /// Shows task `.0`, a tab of tabs: the screens open now are kept as
    /// the task shown before (by their ids and params; dropped from memory
    /// and made anew when it is shown again), and those of task `.0` open
    /// again, or the first time the screen of slot `.0` (or screen `.0`).
    /// Each task keeps its own way back; its first screen is the first of
    /// `Env::stack`. At start the task is the home
    /// slot's.
    SwitchTask(Id),
    /// Binds slot `.0` to screen `.1` for the user, or back to what the
    /// plugins bind it to (`None`): at once and kept in `paperpaw.ini`
    /// (`[slots]`), e.g. "use this as the home screen?".
    BindSlot(Id, Option<Id>),
    /// Shows the row of hints for a few seconds on a screen that has it
    /// only on a press ([`HintVisibility::OnPress`](paperpaw_sdk::HintVisibility)).
    ShowHints,
    /// Changes a setting; invalid values are rejected with a warning.
    SetSetting(Id, SettingValue),
    /// Forgets the value set for a setting: its default applies again (in
    /// the active scope, where changes go there: the scope's value, the
    /// global one applies again). See `Settings::reset`.
    ResetSetting(Id),
    /// Shows settings with these values for a while without keeping them
    /// (`None`: as if not set), e.g. a layout before it is chosen: every
    /// screen drawn from them shows them, until the next
    /// `PreviewSettings` (empty: the values kept show again) or until the
    /// screen that asked closes; the event
    /// [`PREVIEW_CHANGED`](paperpaw_sdk::builtin::events::PREVIEW_CHANGED)
    /// follows each change. What the kernel does when a setting changes
    /// (the language, the turn of the screen) waits for the value kept.
    /// See `Settings::set_preview`.
    PreviewSettings(Vec<(Id, Option<SettingValue>)>),
    /// Redraws the current screen completely; with
    /// [`RefreshMode::Full`] the whole screen, flashing.
    Redraw(RefreshMode),
    /// Keeps the device from going to sleep by itself for a while (see
    /// [`WakeHold`](paperpaw_sdk::power::WakeHold)); the same id again renews
    /// it.
    KeepAwake(paperpaw_sdk::power::WakeHold),
    /// Lets go of the hold of this id ([`Ask::KeepAwake`]).
    LetSleep(Id),
    /// A command that comes after this release, by its key: what PaperPaw
    /// adds later reaches plugins built now without a new plugin
    /// interface. A kernel that does not know the key leaves it.
    #[non_exhaustive]
    Extended { key: Id, params: Params },
    /// Notifies all subscribers of an event, then every open screen.
    Emit(Id, Params),
    /// Runs action `.0` as if the user had triggered it (e.g. from a
    /// menu), with params `.1`: its handler gets them, and so does a
    /// screen that takes it (`Cx::params`), e.g. a
    /// rule applying the profile it names. [`Ask::run`] runs it
    /// without.
    RunAction(Id, Params),
    /// Switches a plugin on or off in `paperpaw.ini`; applies after a
    /// restart. The kernel refuses plugins that cannot be switched off
    /// (see [`PluginInfo::can_switch_off`](paperpaw_sdk::PluginInfo)).
    SetPluginEnabled(Id, bool),
    /// Loads the data plugin in this folder on the storage (e.g.
    /// `plugins/font-fredoka`, just downloaded) at once, as if PaperPaw
    /// had found it at start: its fonts can be chosen, its translations,
    /// key bindings and presets apply. The event
    /// [`PLUGINS_CHANGED`](paperpaw_sdk::builtin::events::PLUGINS_CHANGED)
    /// follows; problems are warnings, as at start.
    LoadPlugin(String),
    /// Removes a data plugin: its folder is deleted and its fonts and
    /// presets go at once (a font chosen from it gives way to the
    /// default); what else it brought (translations, key bindings,
    /// styles, defaults) is gone after the next start. The event
    /// [`PLUGINS_CHANGED`](paperpaw_sdk::builtin::events::PLUGINS_CHANGED)
    /// follows. Plugins with code are not removed this way.
    RemovePlugin(Id),
    /// Installs the data plugin `Id` downloaded into this folder (one
    /// PaperPaw does not load, e.g. `plugins/.airy.part`, see
    /// [`install`](paperpaw_sdk::install)): it takes the place of every version
    /// of that plugin on the storage (loaded, switched off or refused at
    /// start), becomes the folder named as it beside the one it was
    /// downloaded into, and is loaded at once. A plugin switched off stays
    /// off; what was chosen from the version before (e.g. its font for
    /// reading) stays chosen where the new one offers it too. The event
    /// [`PLUGINS_CHANGED`](paperpaw_sdk::builtin::events::PLUGINS_CHANGED)
    /// follows. A folder that holds another plugin, or none that loads,
    /// is deleted with a warning, and the version before stays.
    InstallPlugin(Id, String),
    /// Asks for plugins by id, e.g. those a template or a plugin's own
    /// page names: those not on the device (in any version, switched off
    /// or not) go to the plugin that installs plugins as
    /// [`PLUGINS_REQUESTED`](paperpaw_sdk::builtin::events::PLUGINS_REQUESTED),
    /// which asks the reader first; nothing is installed without them.
    /// What came of it comes back as
    /// [`PLUGINS_ANSWERED`](paperpaw_sdk::builtin::events::PLUGINS_ANSWERED).
    /// `by` is the plugin that asks, as the reader is told. At most
    /// [`MAX_REQUESTED_PLUGINS`](paperpaw_sdk::MAX_REQUESTED_PLUGINS)
    /// ids are taken. Made with [`Ask::request_plugins`].
    RequestPlugins { by: Id, plugins: Vec<Id> },
    /// Makes an HTTP request (see [`HttpRequest`]); the answer comes as
    /// the request's reply event. Check [`Capabilities::network`](paperpaw_sdk::Capabilities) first.
    /// Made with [`Ask::http`].
    Http(alloc::boxed::Box<HttpRequest>),
    /// Uses the settings stored in the file at this storage path for
    /// [scoped](paperpaw_sdk::SettingSpec::scoped) settings, e.g. those of the
    /// book being read, while the screen that sent it stays open; `None`
    /// ends the scope.
    SetScope(Option<String>),
    /// Redefines a menu with exactly these entries (actions, settings or
    /// submenus), saved in `[menus]` of `paperpaw.ini`; `None` returns to
    /// the entries plugins contribute.
    SetMenu(Id, Option<Vec<Id>>),
    /// Binds `input` (as in `paperpaw.ini`, e.g. `button.menu` or
    /// `tap.center`) in key map `layer` (a screen or slot id, or
    /// `global`) to `action`, saved in the user's `paperpaw.ini` and in
    /// effect at once; `None` returns it to what plugins and presets bind.
    /// Bind [`actions::NOTHING`](paperpaw_sdk::builtin::actions::NOTHING) to make
    /// an input do nothing.
    #[non_exhaustive]
    Bind {
        layer: String,
        input: String,
        action: Option<Id>,
    },
    /// Shows a short message for a few seconds, e.g. "Sync done", by the
    /// screen bound to [`slots::NOTICE`](paperpaw_sdk::builtin::slots::NOTICE),
    /// over whatever is open and without taking any input. A newer
    /// message replaces it. Only what the reader is doing shows one:
    /// background work's (declared work, events, actions run by rules)
    /// becomes the plugin's note instead (see [`work`](paperpaw_sdk::work)).
    Notify(String),
    /// Shows a message that must reach the reader also from background
    /// work, as [`Notify`](Self::Notify) does: one they set up themself
    /// (a rule's, "the battery is at 80 %: unplug"), or one about the
    /// device itself (safe mode, plugins not installed at start, an
    /// update installed). Never for results of syncs, progress or offers:
    /// those belong on the plugin's own page.
    Announce(String),
    /// Restarts PaperPaw, e.g. to apply switched plugins. The shell builds
    /// a new kernel; the reading position is already saved.
    Restart,
    /// Saves what the screen shows as a gray PNG at this storage path,
    /// once the frame is drawn (so a menu that ran this is closed by
    /// then); a message tells where.
    Screenshot(String),
    /// Keeps or removes a password or key (see
    /// [`secrets`](paperpaw_sdk::secrets)), saved at once.
    SetSecret(alloc::boxed::Box<paperpaw_sdk::secrets::SecretChange>),
    /// Bluetooth, for plugins declaring [`Uses::BLUETOOTH`](paperpaw_sdk::Uses):
    /// written with the functions of [`bluetooth`](paperpaw_sdk::bluetooth).
    Bluetooth(alloc::boxed::Box<paperpaw_sdk::bluetooth::BtRequest>),
    /// The developer link over USB, for plugins declaring
    /// [`Uses::DEVELOPER`](paperpaw_sdk::Uses): written with the functions of
    /// [`developer`](paperpaw_sdk::developer). Reserved: answered as not
    /// available in this release.
    Developer(alloc::boxed::Box<paperpaw_sdk::developer::DevRequest>),
    /// Tells the radio what to do, where PaperPaw manages Wi-Fi
    /// (`Env::wifi`); see [`wifi`](paperpaw_sdk::wifi). Ignored
    /// elsewhere.
    Wifi(paperpaw_sdk::wifi::WifiCommand),
    /// No network is coming for now (none saved, none in reach): the
    /// requests, servers and connections waiting for one fail at once, as
    /// they would after [`NETWORK_WAIT_MS`](paperpaw_sdk::wifi::NETWORK_WAIT_MS),
    /// so their screens say so instead of waiting. The plugin that runs
    /// Wi-Fi sends it.
    NoNetwork,
    /// Declares work for the kernel to run at the cheapest moment (see
    /// [`work`](paperpaw_sdk::work)): the same id replaces what was declared.
    Work(alloc::boxed::Box<paperpaw_sdk::work::Work>),
    /// Takes back the work declared with this id and key (none: every
    /// key of the id), if it has not run; requests made already are taken
    /// back with [`CancelHttp`](Self::CancelHttp).
    CancelWork(Id, Option<String>),
    /// Takes back the plugin's requests whose reply is this event; each
    /// is answered with the refusal `Cancelled`.
    CancelHttp(Id),
    /// The reader chose *Go online* where the mode is Offline (see
    /// [`power`](paperpaw_sdk::power)): the network for the screen sending it
    /// until it closes; work held back runs on the same connection.
    GoOnline,
    /// The reader chose *Go online* where the mode is Offline outside a
    /// screen of the network (a widget's *Update now*, a plugin's own
    /// offer for what its background work would bring): the network for
    /// at most this long (at most [`MAX_ONLINE_FOR`](paperpaw_sdk::power::MAX_ONLINE_FOR)),
    /// whatever screen shows; work held back runs as in Saving, then the
    /// radio goes off once nothing needs it. Only on the reader's input.
    GoOnlineFor(core::time::Duration),
    /// Wears a power mode: its level for plugins and declared work, its
    /// values over the reader's own (see [`power`](paperpaw_sdk::power)). Sent
    /// by the plugin that chooses the mode.
    Wear(paperpaw_sdk::power::Wear),
    /// Installs a firmware update or restarts, where PaperPaw is the
    /// device's firmware (`Env::firmware`); see
    /// [`firmware`](paperpaw_sdk::firmware). Ignored elsewhere.
    Firmware(paperpaw_sdk::firmware::FirmwareCommand),
    /// Asks the shell to sleep once the frame is shown, where it can
    /// ([`Capabilities::sleep`](paperpaw_sdk::Capabilities)); the kernel is told when it wakes.
    Sleep,
    /// Puts text on the clipboard, on devices with one also the system's,
    /// for other apps. Long text is cut at [`MAX_CLIPBOARD`] bytes.
    SetClipboard(String),
    /// Serves HTTP to the local network on `port` (shells that cannot say
    /// so with a failure). Everything about the server comes as the event
    /// `events`: that it started (with its address), each request (answer
    /// with [`Ask::Respond`]), that it failed or stopped: a
    /// [`ServerNotice`](paperpaw_sdk::ServerNotice). Serve only while the user wants it: a server
    /// also stops when the screen showing as it started is closed.
    #[non_exhaustive]
    Serve { server: Id, port: u16, events: Id },
    /// Stops serving.
    StopServing(Id),
    /// Opens a TCP connection to `host` and `port` (see
    /// [ADR 0014](https://github.com/hthoene/paperpaw/blob/main/docs/adr/0014-sockets-for-plugins.md)).
    /// Everything about it comes as the event `events`, with `id` the
    /// socket ([`params::ID`](paperpaw_sdk::builtin::params::ID)) and what
    /// happened, a [`SocketNotice`](paperpaw_sdk::SocketNotice): connected,
    /// received, saved, closed, failed. Nothing is read unasked ([`Ask::SocketRead`]). A socket
    /// also closes when the screen showing as it was opened is closed;
    /// one that background work's event or job step opens closes when
    /// that run ends instead (see [`work`](paperpaw_sdk::work)).
    ///
    /// With `tls` the connection is secure from the start (e.g. IMAP on
    /// 993): it comes as [`SocketNotice::Secured`](paperpaw_sdk::SocketNotice::Secured) once the handshake is
    /// done, instead of `Connected`; what is sent before goes after it.
    /// Made with [`Ask::connect`] or [`Ask::connect_tls`].
    #[non_exhaustive]
    Connect {
        socket: Id,
        host: String,
        port: u16,
        events: Id,
        tls: Option<alloc::boxed::Box<Tls>>,
    },
    /// Sends bytes on a socket.
    #[non_exhaustive]
    SocketSend { socket: Id, data: Vec<u8> },
    /// Asks a socket for what comes next (one read at a time).
    #[non_exhaustive]
    SocketRead { socket: Id, read: SocketRead },
    /// Closes a socket.
    Disconnect(Id),
    /// Sends `data` as a UDP datagram to the local network on each of
    /// `ports`, e.g. to find a service. Each answer within `wait_ms` comes
    /// as the event `events` ([`SocketNotice::Answer`](paperpaw_sdk::SocketNotice::Answer), with `id`), then
    /// [`SocketNotice::Done`](paperpaw_sdk::SocketNotice::Done). Made with [`Ask::broadcast`].
    Broadcast(alloc::boxed::Box<Broadcast>),
    /// Answers a request of a server. Made with [`Ask::respond`].
    Respond(alloc::boxed::Box<ServerResponse>),
    /// Asks for the text on the clipboard (see
    /// `Env::clipboard`). It comes as the event `.0` with
    /// the parameters `.1` plus `text` ([`params::TEXT`](paperpaw_sdk::builtin::params::TEXT)),
    /// empty if there is none: a [`Typed`](paperpaw_sdk::replies::Typed). Read it only when the user asks to paste:
    /// some systems tell the user every time an app reads the clipboard.
    ReadClipboard(Id, Params),
    /// Speaks text aloud where the device can
    /// ([`Capabilities::speech`](paperpaw_sdk::Capabilities)), instead of what is being spoken or
    /// after it (`Speech::queue`). Without speech the reply comes at
    /// once, failed. Speech stops when the screen that started it is
    /// closed. Made with [`Ask::speak`].
    Speak(alloc::boxed::Box<Speech>),
    /// Stops speaking; no reply comes for the texts cut off.
    StopSpeaking,
    /// Plays a sound file (reserved: no shell plays sound yet, see
    /// [`sound`](paperpaw_sdk::sound)). Its reply, a
    /// [`Played`](paperpaw_sdk::replies::Played), comes once it has played, and
    /// at once, not played, where the device has no sound
    /// ([`Capability::AUDIO`](paperpaw_sdk::Capability::AUDIO)). A sound in its
    /// place stops. Made with [`Ask::play_sound`].
    PlaySound(alloc::boxed::Box<paperpaw_sdk::sound::Sound>),
    /// Stops the sound playing (reserved, see [`Ask::PlaySound`]); no
    /// reply comes for it.
    StopSound,
    /// Switches the device off once the frame is shown, where PaperPaw
    /// does so ([`Capabilities::power_off`](paperpaw_sdk::Capabilities)): plugins
    /// hear [`events::SUSPEND`](paperpaw_sdk::builtin::events::SUSPEND) first, and
    /// the next start is a cold one. Ignored elsewhere; to put the device
    /// to sleep, [`Ask::Sleep`].
    PowerOff,
    /// Shows the card to a computer as a USB drive once the frame is shown,
    /// where the device can ([`Capability::USB_DRIVE`](paperpaw_sdk::Capability)):
    /// plugins hear [`events::SUSPEND`](paperpaw_sdk::builtin::events::SUSPEND)
    /// first and PaperPaw leaves the card alone while the computer has it;
    /// when it ejects it or the cable goes, plugins hear
    /// [`events::USB_DRIVE_ENDED`](paperpaw_sdk::builtin::events::USB_DRIVE_ENDED)
    /// and the library is walked anew. The frame shown should say that the
    /// device is connected to a computer. Ignored elsewhere.
    UsbDrive,
    /// Switches an open socket to TLS (STARTTLS, e.g. SMTP on 587), once
    /// the server agreed and everything it sent before was read:
    /// [`SocketNotice::Secured`](paperpaw_sdk::SocketNotice::Secured) when the
    /// handshake is done. Bytes come that were not read, or a read under
    /// way, fail it ([`SocketError::NotReady`](paperpaw_sdk::SocketError::NotReady)),
    /// as they would pass as secure. Made with [`Ask::start_tls`].
    #[non_exhaustive]
    StartTls {
        socket: Id,
        tls: alloc::boxed::Box<Tls>,
    },
}

impl Ask {
    /// [`Ask::Bind`]: `input` in key map `layer` to `action` (`None`:
    /// back to what plugins and presets bind).
    pub fn bind(layer: impl Into<String>, input: impl Into<String>, action: Option<Id>) -> Self {
        Ask::Bind {
            layer: layer.into(),
            input: input.into(),
            action,
        }
    }

    /// [`Ask::Serve`]: server `server` on `port`, heard as `events`.
    pub fn serve(server: Id, port: u16, events: Id) -> Self {
        Ask::Serve {
            server,
            port,
            events,
        }
    }

    /// [`Ask::Connect`]: socket `socket` to `host` and `port`, heard as
    /// `events`.
    pub fn connect(socket: Id, host: impl Into<String>, port: u16, events: Id) -> Self {
        Ask::Connect {
            socket,
            host: host.into(),
            port,
            events,
            tls: None,
        }
    }

    /// [`Ask::Connect`] secure from the start, as `tls` sets up: socket
    /// `socket` to `host` and `port`, heard as `events`.
    pub fn connect_tls(
        socket: Id,
        host: impl Into<String>,
        port: u16,
        events: Id,
        tls: Tls,
    ) -> Self {
        Ask::Connect {
            socket,
            host: host.into(),
            port,
            events,
            tls: Some(alloc::boxed::Box::new(tls)),
        }
    }

    /// [`Ask::StartTls`]: `socket` switched to TLS, as `tls` sets up.
    pub fn start_tls(socket: Id, tls: Tls) -> Self {
        Ask::StartTls {
            socket,
            tls: alloc::boxed::Box::new(tls),
        }
    }

    /// [`Ask::SocketSend`]: `data` on `socket`.
    pub fn socket_send(socket: Id, data: Vec<u8>) -> Self {
        Ask::SocketSend { socket, data }
    }

    /// [`Ask::SocketRead`]: what comes next on `socket`, as `read` says.
    pub fn socket_read(socket: Id, read: SocketRead) -> Self {
        Ask::SocketRead { socket, read }
    }

    /// [`Ask::Broadcast`]: `data` to `ports` on the local network,
    /// answers within `wait_ms` heard as `events` with `id`.
    pub fn broadcast(id: Id, ports: Vec<u16>, data: Vec<u8>, wait_ms: u32, events: Id) -> Self {
        let broadcast = Broadcast::new(id, ports, data, wait_ms, events);
        Ask::Broadcast(alloc::boxed::Box::new(broadcast))
    }

    /// [`Ask::RequestPlugins`]: plugin `by` asks for `plugins`.
    pub fn request_plugins(by: impl Into<Id>, plugins: impl IntoIterator<Item = Id>) -> Self {
        Ask::RequestPlugins {
            by: by.into(),
            plugins: plugins.into_iter().collect(),
        }
    }

    /// [`Ask::Http`]: makes `request`.
    pub fn http(request: HttpRequest) -> Self {
        Ask::Http(alloc::boxed::Box::new(request))
    }

    /// [`Ask::Respond`]: answers a server's request with `response`.
    pub fn respond(response: ServerResponse) -> Self {
        Ask::Respond(alloc::boxed::Box::new(response))
    }

    /// [`Ask::Speak`]: speaks `speech`.
    pub fn speak(speech: Speech) -> Self {
        Ask::Speak(alloc::boxed::Box::new(speech))
    }

    /// [`Ask::PlaySound`]: plays `sound` (reserved).
    pub fn play_sound(sound: paperpaw_sdk::sound::Sound) -> Self {
        Ask::PlaySound(alloc::boxed::Box::new(sound))
    }

    /// [`Ask::Work`]: declares `work`.
    pub fn work(work: paperpaw_sdk::work::Work) -> Self {
        Ask::Work(alloc::boxed::Box::new(work))
    }

    /// [`Ask::SetSecret`]: makes `change`.
    pub fn set_secret(change: paperpaw_sdk::secrets::SecretChange) -> Self {
        Ask::SetSecret(alloc::boxed::Box::new(change))
    }
}

impl Ask {
    /// Changes `setting` to `value` (see [`setting`](paperpaw_sdk::setting)).
    pub fn set<T: paperpaw_sdk::setting::SettingType>(
        setting: &paperpaw_sdk::setting::Setting<T>,
        value: T::Input,
    ) -> Ask {
        Ask::SetSetting(setting.id().clone(), T::write(value))
    }

    /// [`Ask::RunAction`]: runs `action` without params.
    pub fn run(action: impl Into<Id>) -> Ask {
        Ask::RunAction(action.into(), Params::new())
    }

    /// Opens a slot without parameters.
    pub fn open_slot(slot: Id) -> Ask {
        Ask::OpenSlot(slot, Params::new())
    }

    /// [`Ask::Emit`]: `event` with `data`.
    pub fn emit<P: EventData>(event: &Event<P>, data: P) -> Ask {
        Ask::Emit(event.id().clone(), data.into_params())
    }
}

#[cfg(test)]
mod size {
    /// Asks are queued by the hundred: the large ones are boxed, so
    /// that every command takes no more than a few words.
    #[test]
    fn commands_stay_small() {
        let size = core::mem::size_of::<super::Ask>();
        assert!(size <= 88, "Ask takes {size} bytes");
    }
}

#[cfg(test)]
mod printed {
    extern crate std;
    use alloc::format;

    /// A request printed for debugging, alone or in a command, shows no
    /// header value, body, query or param value: where secrets go.
    #[test]
    fn requests_print_no_secrets() {
        let request = paperpaw_sdk::HttpRequest::new(
            "POST",
            "https://sync.example/api?key=QUERYSECRET",
            paperpaw_sdk::Id::new_static("demo.reply"),
        )
        .header("Authorization", "Basic HEADERSECRET")
        .body("{\"password\":\"BODYSECRET\"}")
        .with(paperpaw_sdk::Params::new().with("token", "PARAMSECRET"));
        let printed = format!("{:?}", super::Ask::http(request));
        for secret in ["QUERYSECRET", "HEADERSECRET", "BODYSECRET", "PARAMSECRET"] {
            assert!(!printed.contains(secret), "{printed}");
        }
        assert!(printed.contains("https://sync.example/api") && printed.contains("Authorization"));
        let change =
            paperpaw_sdk::secrets::SecretChange::keep("demo.password", "Password", "KEPTSECRET");
        let printed = format!("{:?}", super::Ask::set_secret(change));
        assert!(
            !printed.contains("KEPTSECRET") && printed.contains("demo.password"),
            "{printed}"
        );
    }
}

impl From<paperpaw_sdk::bluetooth::BtRequest> for Ask {
    fn from(request: paperpaw_sdk::bluetooth::BtRequest) -> Self {
        Ask::Bluetooth(alloc::boxed::Box::new(request))
    }
}

impl From<paperpaw_sdk::developer::DevRequest> for Ask {
    fn from(request: paperpaw_sdk::developer::DevRequest) -> Self {
        Ask::Developer(alloc::boxed::Box::new(request))
    }
}

/// What a handler sends to the kernel: the commands it queues, in order.
///
/// Every handler of a plugin gets one (actions, events, observers, work
/// steps, voices, widgets' input, installers). It is the part of a
/// handler's signature that grows: what is added to it later, such as
/// which plugin runs, its limits or small helpers, reaches every handler
/// through a method, without a new signature. That is why its fields are
/// private.
///
/// It reads as the slice of the commands queued so far
/// (`commands.iter()`, `commands[0]`, `commands.len()`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Asks {
    queued: Vec<Ask>,
}

impl Asks {
    /// None queued yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues `command`.
    pub fn push(&mut self, command: Ask) {
        self.queued.push(command);
    }

    /// Queues `event` with `data` ([`Ask::Emit`]).
    pub fn emit<P: EventData>(&mut self, event: &Event<P>, data: P) {
        self.queued.push(Ask::emit(event, data));
    }

    /// Takes every command queued, leaving none (for the kernel, which
    /// runs them).
    pub fn take(&mut self) -> Vec<Ask> {
        core::mem::take(&mut self.queued)
    }
}

impl core::ops::Deref for Asks {
    type Target = [Ask];

    fn deref(&self) -> &[Ask] {
        &self.queued
    }
}

impl Extend<Ask> for Asks {
    fn extend<I: IntoIterator<Item = Ask>>(&mut self, commands: I) {
        self.queued.extend(commands);
    }
}

impl FromIterator<Ask> for Asks {
    fn from_iter<I: IntoIterator<Item = Ask>>(commands: I) -> Self {
        Self {
            queued: commands.into_iter().collect(),
        }
    }
}

impl From<Vec<Ask>> for Asks {
    fn from(queued: Vec<Ask>) -> Self {
        Self { queued }
    }
}

impl IntoIterator for Asks {
    type Item = Ask;
    type IntoIter = alloc::vec::IntoIter<Ask>;

    fn into_iter(self) -> Self::IntoIter {
        self.queued.into_iter()
    }
}

impl<'a> IntoIterator for &'a Asks {
    type Item = &'a Ask;
    type IntoIter = core::slice::Iter<'a, Ask>;

    fn into_iter(self) -> Self::IntoIter {
        self.queued.iter()
    }
}
