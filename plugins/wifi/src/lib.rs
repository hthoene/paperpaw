//! Wi-Fi, where PaperPaw manages it itself (the X4): when the radio is on,
//! which network it joins, and the page to choose networks (see
//! `paperpaw_sdk::wifi`). Another plugin may take its place: the shell
//! only does what it is told, and the networks joined are kept with the
//! passwords and keys, the same for any plugin.
//!
//! By default the radio is off and comes on only when a plugin wants the
//! network (a catalog, a sync, a transfer), joins the network joined
//! before that the user picked last, or the strongest ([`JOIN_FIRST`]),
//! and goes off again a minute after nothing uses it, or
//! at once when home or a book shows: it costs battery and memory.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;
mod native_entry;
pub mod native_ini;
pub mod native_known;
#[doc(hidden)]
pub mod native_model;
pub mod native_own;
pub mod native_phone;
/// The interface-7 plugin.
pub mod native_plugin;
pub mod native_policy;
pub mod native_radio;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;
#[cfg(test)]
mod native_secret_tests;

mod networks;
mod page;
mod phone;
mod policy;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::builtin::{events, params, slots};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::wifi::{self, JoinFailure, WifiCommand, WifiState};
use paperpaw_sdk::work::Work;
use paperpaw_sdk::{
    Arg, Command, Commands, Env, Id, Manifest, MenuNeed, Params, Plugin, RegisterError, Registrar,
    ScreenSpec, SettingSpec, StatusItemSpec, StatusValue,
};

use paperpaw_sdk::setting::{Setting, Text};
use policy::{Goal, Policy};

pub const PLUGIN: Id = Id::new_static("wifi");
/// The settings group, shown as the Wi-Fi page.
pub const GROUP: Id = Id::new_static("wifi.group");
/// The Wi-Fi page.
pub const PAGE: Id = Id::new_static("wifi.page");
/// The page of the networks in reach, opened from the Wi-Fi page.
pub const NETWORKS: Id = Id::new_static("wifi.networks");
/// Opens the Wi-Fi page: what a tap on Wi-Fi's status item runs.
pub const OPEN: Id = Id::new_static("wifi.open");
/// Setting up Wi-Fi with a phone (see `phone`).
pub const PHONE: Id = Id::new_static("wifi.phone");
/// A file on the card with networks to join, read once at a start, kept
/// with the passwords and removed (e.g. written by the flasher): lines
/// `ssid = …` and `password = …`, a pair for each network.
pub const WIFI_FILE: &str = "wifi.ini";
/// The largest [`WIFI_FILE`] read; a larger one is removed, unread.
const WIFI_FILE_MOST: usize = 16 * 1024;
/// The networks of the [`WIFI_FILE`] are kept: it goes.
const WIFI_FILE_KEPT: Id = Id::new_static("wifi.file-kept");
/// The status item: the Wi-Fi sign while the device is in a network.
pub const STATUS: Id = Id::new_static("wifi.status");
/// The radio comes on by itself when a plugin wants the network.
pub const ON_DEMAND: Setting<bool> = Setting::new("wifi.on-demand");
/// How long after the last use the radio goes off.
pub const IDLE_OFF: Setting<Text> = Setting::new("wifi.idle-off");
/// The radio stays on (in the network joined before) while charging.
pub const WHILE_CHARGING: Setting<bool> = Setting::new("wifi.while-charging");
/// Which network joined before is joined first: the one picked last on
/// the page (the default), or the strongest.
pub const JOIN_FIRST: Setting<Text> = Setting::new("wifi.join-first");
pub const FIRST_PICKED: &str = "wifi.join-first.picked";
pub const FIRST_STRONGEST: &str = "wifi.join-first.strongest";
/// The network the user picked last on the page, its name in a line of
/// its own (in the plugin's folder, see [`Env::plugin_dir`]).
const PICKED_FILE: &str = "picked.txt";
/// The keyboard's answer: a network's password.
pub(crate) const PASSWORD_ENTERED: Event<Typed> = Event::new("wifi.password-entered");
/// The timer to switch the radio off.
const IDLE_CHECK: Id = Id::new_static("wifi.idle-check");
/// The timer to try the networks joined before again.
const RETRY: Id = Id::new_static("wifi.retry");
/// How long after a round that joined nothing it is tried again.
const RETRY_MS: u64 = 10_000;
/// The parameter of [`IDLE_CHECK`]: the policy's generation it was set at.
const GENERATION: &str = "generation";

/// The choices of [`IDLE_OFF`] and their seconds.
pub(crate) const IDLE_OFF_CHOICES: [(&str, u64); 5] = [
    ("wifi.idle-off.30s", 30),
    ("wifi.idle-off.1m", 60),
    ("wifi.idle-off.2m", 120),
    ("wifi.idle-off.5m", 300),
    ("wifi.idle-off.10m", 600),
];

pub(crate) type Shared = Rc<RefCell<Policy>>;

/// The radio stays on now: charging, and the user wants that.
fn charging_keeps_it(env: &Env<'_>) -> bool {
    // Offline keeps the radio off also while charging.
    env.settings().read(&WHILE_CHARGING)
        && env.status().battery.is_some_and(|b| b.charging)
        && env.power().online()
}

fn idle_seconds(env: &Env<'_>) -> u64 {
    let choice = env.settings().read(&IDLE_OFF);
    IDLE_OFF_CHOICES
        .iter()
        .find(|(id, _)| *id == choice)
        .map_or(60, |(_, seconds)| *seconds)
}

fn wifi_commands(out: &mut Commands, commands: Vec<WifiCommand>) {
    out.extend(commands.into_iter().map(Command::Wifi));
}

/// Home or a book shows (the screen on top is the home's or the
/// reader's): nothing there uses the network.
fn at_rest(env: &Env<'_>) -> bool {
    env.stack()
        .last()
        .and_then(|top| top.slot.as_ref())
        .is_some_and(|slot| *slot == slots::HOME || *slot == slots::READER)
}

/// Sets the timer to switch the radio off, if nothing keeps it on.
fn schedule_off(env: &Env<'_>, policy: &Policy, out: &mut Commands) {
    let Some(wifi) = env.wifi() else {
        return;
    };
    if wifi.state.is_on() && policy.may_go_off(charging_keeps_it(env)) {
        let at = env.now().add_millis(idle_seconds(env) * 1000);
        let params = Params::new().with(GENERATION, alloc::format!("{}", policy.generation));
        out.push(Command::work(Work::at(IDLE_CHECK, at).with(params)));
    }
}

/// After a round that joined nothing: sets the timer to try again, if
/// the network is still wanted (or charging keeps the radio on), or else
/// the one to switch the radio off.
fn schedule_retry(env: &Env<'_>, policy: &Policy, out: &mut Commands) {
    let Some(wifi) = env.wifi() else {
        return;
    };
    if policy.still_wanted(wifi, env.now()) || charging_keeps_it(env) {
        let params = Params::new().with(GENERATION, alloc::format!("{}", policy.generation));
        out.push(Command::work(
            Work::at(RETRY, env.now().add_millis(RETRY_MS)).with(params),
        ));
    } else if !wifi.busy {
        schedule_off(env, policy, out);
    }
}

/// No network is coming for what waits: it fails now, and the network
/// counts as no longer wanted.
fn no_network(policy: &mut Policy, out: &mut Commands) {
    policy.asked_at = None;
    out.push(Command::NoNetwork);
}

/// Why joining `ssid` failed, in words.
pub(crate) fn failure_text(env: &Env<'_>, ssid: &str, reason: JoinFailure) -> String {
    let key = match reason {
        JoinFailure::Password => "wifi.failed.password",
        JoinFailure::NotFound => "wifi.failed.not-found",
        JoinFailure::Timeout => "wifi.failed.timeout",
        JoinFailure::Other => "wifi.failed.other",
        _ => "wifi.failed.other",
    };
    env.i18n().format(key, &[("ssid", Arg::Str(ssid))])
}

/// The networks in [`WIFI_FILE`], to keep, and the file removed; nothing
/// where there is none. A password goes with the `ssid` line before it.
fn read_wifi_file(env: &Env<'_>) -> Vec<Command> {
    let bytes = match paperpaw_sdk::read_file(env.storage(), WIFI_FILE, WIFI_FILE_MOST) {
        Ok(bytes) => bytes,
        // No file has that many networks: gone, so that no password stays
        // on the card, and said.
        Err(paperpaw_sdk::StorageError::TooLarge) => {
            let _ = env.storage().remove(WIFI_FILE);
            return alloc::vec![Command::Notify(env.i18n().tr("wifi.file-too-large").into())];
        }
        Err(_) => return Vec::new(),
    };
    let networks = wifi_file_networks(&bytes);
    let known = wifi::known(env.secrets().iter());
    let mut out: Vec<Command> = networks
        .iter()
        // A network without a password does not take the password kept.
        .filter(|(ssid, password)| !password.is_empty() || !known.iter().any(|(k, _)| k == ssid))
        .map(|(ssid, password)| Command::set_secret(wifi::keep(ssid, password)))
        .collect();
    // Removed once kept (the commands before it carried out).
    out.push(Command::Emit(WIFI_FILE_KEPT, Params::new()));
    out
}

/// The networks of a [`WIFI_FILE`]: a password goes with the `ssid` line
/// before it.
fn wifi_file_networks(bytes: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(bytes);
    let mut networks: Vec<(String, String)> = Vec::new();
    for line in text.lines().map(str::trim) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "ssid" if !value.is_empty() => networks.push((String::from(value), String::new())),
            "password" => {
                if let Some(network) = networks.last_mut() {
                    network.1 = String::from(value);
                }
            }
            _ => {}
        }
    }
    networks
}

/// The Wi-Fi plugin.
#[derive(Debug, Default)]
pub struct Wifi;

/// How long the radio stays on unused, to choose from.
const IDLE_OFF_IDS: [&str; IDLE_OFF_CHOICES.len()] = {
    let mut ids = [""; IDLE_OFF_CHOICES.len()];
    let mut i = 0;
    while i < IDLE_OFF_CHOICES.len() {
        ids[i] = IDLE_OFF_CHOICES[i].0;
        i += 1;
    }
    ids
};

/// When the radio is on.
static SETTINGS: [SettingSpec; 4] = [
    SettingSpec::switch(
        ON_DEMAND.static_str(),
        "wifi.setting.on-demand",
        true,
        GROUP,
    )
    .description("wifi.setting.on-demand.help"),
    SettingSpec::choice(
        IDLE_OFF.static_str(),
        "wifi.setting.idle-off",
        &IDLE_OFF_IDS,
        "wifi.idle-off.1m",
        GROUP,
    )
    .description("wifi.setting.idle-off.help"),
    SettingSpec::switch(
        WHILE_CHARGING.static_str(),
        "wifi.setting.while-charging",
        false,
        GROUP,
    )
    .description("wifi.setting.while-charging.help"),
    SettingSpec::choice(
        JOIN_FIRST.static_str(),
        "wifi.setting.join-first",
        &[FIRST_PICKED, FIRST_STRONGEST],
        FIRST_PICKED,
        GROUP,
    )
    .description("wifi.setting.join-first.help"),
];

/// Where the network picked last is kept.
fn picked_path() -> String {
    alloc::format!("{}/{PICKED_FILE}", Env::plugin_dir(PLUGIN.as_str()))
}

/// The network to join first, as *Join first* says: the one picked last
/// (if one was), or none for the strongest.
fn join_first(env: &Env<'_>) -> Option<String> {
    if env.settings().read(&JOIN_FIRST) == FIRST_STRONGEST {
        return None;
    }
    picked(env)
}

/// The network the user picked last on the page, if one was.
fn picked(env: &Env<'_>) -> Option<String> {
    let bytes = paperpaw_sdk::read_file(env.storage(), &picked_path(), 256).ok()?;
    let name = String::from_utf8(bytes).ok()?;
    let name = name.trim_end_matches(['\r', '\n']);
    (!name.is_empty()).then(|| String::from(name))
}

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

static ACTIONS: [paperpaw_sdk::FixedAction; 1] =
    [paperpaw_sdk::FixedAction::new(OPEN, "wifi.title")
        .handler(|_, _, out| out.push(Command::OpenScreen(PAGE, Params::new())))];
static ICONS: [(Id, &str); 2] = [(GROUP, "wifi"), (OPEN, "wifi")];

impl Plugin for Wifi {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Wi-Fi", paperpaw_sdk::package_version!())
            .description("Joins Wi-Fi networks when needed and switches the radio off after.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NETWORK)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let policy: Shared = Rc::new(RefCell::new(Policy::default()));

        // The settings, on the group's own page; only where PaperPaw
        // manages Wi-Fi.
        r.add_setting_group({
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(GROUP, "wifi.title")
                    .order(85)
                    .page(PAGE);
            &SPEC
        })?;
        // Tapped, the Wi-Fi page.
        r.add_status_item(
            StatusItemSpec::new(
                STATUS,
                "wifi.status.item",
                Box::new(|env, _| {
                    env.wifi()
                        .is_some_and(|w| w.state.is_online())
                        .then_some(StatusValue::Icon(Cow::Borrowed("wifi")))
                }),
            )
            .tap(OPEN),
        )?;
        r.add_actions(&ACTIONS)?;
        r.add_icons(&ICONS)?;
        r.add_settings(&SETTINGS)?;
        for spec in &SETTINGS {
            r.set_menu_need(spec.id.clone(), MenuNeed::Wifi)?;
        }
        let p = policy.clone();
        r.add_screen(
            ScreenSpec::new(
                PAGE,
                "wifi.title",
                Box::new(move |env, _| Box::new(page::WifiPage::new(env, p.clone()))),
            )
            .instances(paperpaw_sdk::Instances::SINGLE),
        )?;
        let p = policy.clone();
        r.add_screen(
            ScreenSpec::new(
                NETWORKS,
                "wifi.networks.title",
                Box::new(move |env, _| Box::new(networks::NetworksPage::new(env, p.clone()))),
            )
            .instances(paperpaw_sdk::Instances::SINGLE),
        )?;
        let p = policy.clone();
        r.add_screen(
            ScreenSpec::new(
                PHONE,
                "wifi.phone.title",
                Box::new(move |env, _| Box::new(phone::PhoneScreen::new(env, p.clone()))),
            )
            .instances(paperpaw_sdk::Instances::SINGLE),
        )?;
        // Networks put on the card: kept, the file removed once they are.
        r.on(events::STARTED, |env, (), out| {
            if env.wifi().is_some() {
                out.extend(read_wifi_file(env));
            }
        })?;
        r.subscribe(
            WIFI_FILE_KEPT,
            Box::new(|env, _, _| {
                let Ok(bytes) = paperpaw_sdk::read_file(env.storage(), WIFI_FILE, WIFI_FILE_MOST)
                else {
                    return;
                };
                let known = wifi::known(env.secrets().iter());
                // Kept on the card, not only until the device restarts.
                let kept = wifi_file_networks(&bytes).iter().all(|(ssid, _)| {
                    known.iter().any(|(k, _)| k == ssid)
                        && env.secrets().saved(&wifi::secret_id(ssid))
                });
                if kept {
                    let _ = env.storage().remove(WIFI_FILE);
                }
            }),
        )?;

        // A plugin wants the network.
        let p = policy.clone();
        r.on(events::NETWORK_WANTED, move |env, _, out| {
            let Some(wifi) = env.wifi() else {
                return;
            };
            let mut policy = p.borrow_mut();
            // Not during the setup with a phone, nor while the own network
            // is open (a transfer without a router).
            if !env.settings().read(&ON_DEMAND) || policy.phone || wifi.state.is_hosting() {
                return;
            }
            policy.asked_at = Some(env.now());
            let known = wifi::known(env.secrets().iter());
            if known.is_empty() {
                // Nothing to join: what waits fails now, and its screen
                // says so (no notice over it besides).
                no_network(&mut policy, out);
            }
            wifi_commands(out, policy.wanted(&wifi.state, &known));
        })?;

        // Back home or in a book: the radio goes off now, not after its
        // idle time, so that its memory (about 45 KB on the X4) is free
        // before a book opens, and no battery goes on a radio nothing uses.
        let p = policy.clone();
        r.on(events::SCREEN_SHOWN, move |env, _, out| {
            let Some(wifi) = env.wifi() else {
                return;
            };
            let policy = p.borrow();
            if at_rest(env)
                && wifi.state.is_on()
                && !wifi.state.is_hosting()
                && !wifi.busy
                && !policy.phone
                && !policy.still_wanted(wifi, env.now())
                && policy.may_go_off(charging_keeps_it(env))
            {
                out.push(Command::Wifi(WifiCommand::Off));
            }
        })?;

        // The radio's state changed.
        let p = policy.clone();
        r.on(events::WIFI_CHANGED, move |env, _, out| {
            let Some(wifi) = env.wifi() else {
                return;
            };
            let mut policy = p.borrow_mut();
            // A network the user picked on the page and joined: first
            // next time (see [`JOIN_FIRST`]).
            if let WifiState::Online { ssid, .. } = &wifi.state
                && matches!(&policy.goal, Goal::Chosen { ssid: chosen, .. } if chosen == ssid)
                && picked(env).as_deref() != Some(ssid.as_str())
            {
                let _ = env.storage().create_dir(&Env::plugin_dir(PLUGIN.as_str()));
                let line = alloc::format!("{ssid}\n");
                let _ = env.storage().write(&picked_path(), line.as_bytes());
            }
            if let WifiState::Online { ssid, .. } = &wifi.state
                && let Some((pending, password)) = policy.pending.take()
            {
                if pending == *ssid {
                    out.push(Command::set_secret(wifi::keep(&pending, &password)));
                } else {
                    policy.pending = Some((pending, password));
                }
            }
            // The setup with a phone says what the radio does, as does
            // the plugin that opened the own network.
            if policy.phone || wifi.state.is_hosting() {
                return;
            }
            wifi_commands(out, policy.changed(&wifi.state));
            // On and in no network while charging keeps it on (e.g.
            // switched on again, or a network left): join one.
            if wifi.state == WifiState::On
                && policy.goal == Goal::Nothing
                && charging_keeps_it(env)
                && !wifi::known(env.secrets().iter()).is_empty()
            {
                wifi_commands(out, policy.retry());
            }
            // On, with nothing to do (e.g. joined, nothing waits).
            if !wifi.busy && !policy.still_wanted(wifi, env.now()) {
                schedule_off(env, &policy, out);
            }
        })?;

        // Networks found.
        let p = policy.clone();
        r.on(events::WIFI_SCANNED, move |env, _, out| {
            let Some(wifi) = env.wifi() else {
                return;
            };
            let known = wifi::known(env.secrets().iter());
            let mut policy = p.borrow_mut();
            if policy.phone {
                return;
            }
            policy.first = join_first(env);
            match policy.scanned(&wifi.networks, &known) {
                Some(commands) => wifi_commands(out, commands),
                None => {
                    policy.scans += 1;
                    if policy.still_wanted(wifi, env.now()) && policy.scans < policy::SCANS {
                        // The first scans after the radio came on may
                        // find nothing.
                        wifi_commands(out, alloc::vec![WifiCommand::Scan]);
                        return;
                    }
                    policy.goal = Goal::Nothing;
                    if policy.still_wanted(wifi, env.now()) {
                        out.push(Command::Notify(env.i18n().tr("wifi.none-in-reach").into()));
                        no_network(&mut policy, out);
                    }
                    schedule_retry(env, &policy, out);
                }
            }
        })?;

        // Joining a network failed.
        let p = policy.clone();
        r.on(events::WIFI_FAILED, move |env, failed, out| {
            let Some(wifi) = env.wifi() else {
                return;
            };
            let (ssid, reason) = (failed.ssid.as_str(), failed.reason);
            let known = wifi::known(env.secrets().iter());
            let mut policy = p.borrow_mut();
            // The setup with a phone says it, and opens its network again.
            if policy.phone {
                policy.goal = Goal::Nothing;
                if policy.pending.as_ref().is_some_and(|(p, _)| p == ssid) {
                    policy.pending = None;
                }
                return;
            }
            let chosen = matches!(policy.goal, Goal::Chosen { .. });
            policy.first = join_first(env);
            match policy.failed(ssid, reason, &wifi.networks, &known) {
                Some(commands) if !commands.is_empty() => wifi_commands(out, commands),
                // The last try (or the user's choice): say why.
                result => {
                    if result.is_none() {
                        policy.goal = Goal::Nothing;
                        schedule_retry(env, &policy, out);
                    }
                    // Said when the user waits for it, not for tries
                    // in the background.
                    if chosen || policy.still_wanted(wifi, env.now()) {
                        out.push(Command::Notify(failure_text(env, ssid, reason)));
                    }
                }
            }
        })?;

        // The password typed for a network.
        let p = policy.clone();
        r.on(PASSWORD_ENTERED, move |env, typed, out| {
            let (Some(wifi), Some(ssid)) = (env.wifi(), typed.context.get(params::ID)) else {
                return;
            };
            let password = typed.text.as_str();
            let mut policy = p.borrow_mut();
            policy.pending = Some((String::from(ssid), String::from(password)));
            let commands = policy.choose(&wifi.state, ssid, password);
            wifi_commands(out, commands);
        })?;

        // Nothing uses the network: off a while later.
        let p = policy.clone();
        r.on(events::NETWORK_IDLE, move |env, _, out| {
            schedule_off(env, &p.borrow(), out)
        })?;
        // Another power mode: a radio only charging kept on may go off.
        let p = policy.clone();
        r.on(events::POWER_CHANGED, move |env, (), out| {
            if env.wifi().is_some_and(|w| !w.busy) {
                schedule_off(env, &p.borrow(), out);
            }
        })?;
        let p = policy.clone();
        r.subscribe(
            IDLE_CHECK,
            Box::new(move |env, params, out| {
                let Some(wifi) = env.wifi() else {
                    return;
                };
                let policy = p.borrow();
                let current = params.get(GENERATION).and_then(|g| g.parse::<u32>().ok())
                    == Some(policy.generation);
                if current
                    && wifi.state.is_on()
                    && !wifi.busy
                    && !policy.still_wanted(wifi, env.now())
                    && policy.may_go_off(charging_keeps_it(env))
                {
                    out.push(Command::Wifi(WifiCommand::Off));
                }
            }),
        )?;

        // Another round, if the network is still wanted.
        let p = policy.clone();
        r.subscribe(
            RETRY,
            Box::new(move |env, params, out| {
                let Some(wifi) = env.wifi() else {
                    return;
                };
                let mut policy = p.borrow_mut();
                let current = params.get(GENERATION).and_then(|g| g.parse::<u32>().ok())
                    == Some(policy.generation);
                if !current
                    || wifi.state != WifiState::On
                    || policy.goal != Goal::Nothing
                    || policy.phone
                {
                    return;
                }
                if (policy.still_wanted(wifi, env.now()) || charging_keeps_it(env))
                    && !wifi::known(env.secrets().iter()).is_empty()
                {
                    wifi_commands(out, policy.retry());
                } else if !wifi.busy {
                    // What waited has failed meanwhile.
                    schedule_off(env, &policy, out);
                }
            }),
        )?;

        // On while charging, if the user wants that.
        for event in [events::STARTED, events::STATUS_CHANGED] {
            let p = policy.clone();
            let started = event == events::STARTED;
            r.subscribe(
                event,
                Box::new(move |env, _, out| {
                    let Some(wifi) = env.wifi() else {
                        return;
                    };
                    let mut policy = p.borrow_mut();
                    // The status changes with every percent of the
                    // battery: only a change of charging counts.
                    let keeps = charging_keeps_it(env);
                    if (keeps == policy.charging_kept && !started)
                        || policy.phone
                        || wifi.state.is_hosting()
                    {
                        return;
                    }
                    policy.charging_kept = keeps;
                    if keeps {
                        if !wifi.state.is_on() {
                            let known = wifi::known(env.secrets().iter());
                            wifi_commands(out, policy.wanted(&wifi.state, &known));
                        }
                    } else if !wifi.busy && !policy.still_wanted(wifi, env.now()) {
                        schedule_off(env, &policy, out);
                    }
                }),
            )?;
        }

        // The first-run setup: a network to join, where PaperPaw runs
        // the radio.
        r.add_setup_step(
            paperpaw_sdk::setup::SetupStepSpec::new(
                Id::new_static("wifi.setup"),
                "wifi.title",
                paperpaw_sdk::setup::SetupPage::Screen(PAGE),
            )
            .help("wifi.setup.help")
            .order(50)
            .applies(Box::new(|env: &Env<'_>| env.wifi().is_some())),
        )?;
        // The setup's row for it says so where the reader is connected
        // already (e.g. from the network the flasher wrote).
        r.set_item_state(Id::new_static("wifi.setup"), |env: &Env<'_>| {
            let state = paperpaw_sdk::ItemState::new();
            if env.wifi().is_some_and(|w| w.state.is_online()) {
                state.badge(alloc::string::String::from(
                    env.i18n().tr("wifi.network.connected"),
                ))
            } else {
                state
            }
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

/// The settings the plugin declares, made by its build script from the same
/// declarations as tables.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/wifi_settings.rs"));
}

/// The settings the plugin declares as a table kept in the program (as Beta 3
/// kept them), for a linked build to use in place of a copy on the heap of
/// each (`Kernel::register_linked`). Only a linked plugin has it.
#[cfg(feature = "linked")]
pub static NATIVE_SETTINGS: &[paperpaw_sdk::SettingSpec] = &native_settings::SETTINGS;

/// The setting groups the plugin declares, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_GROUPS: &[paperpaw_sdk::SettingGroupSpec] = &native_settings::GROUPS;

/// The ids the plugin's declarations name, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_IDS: &[paperpaw_sdk::Id] = &native_settings::IDS;

/// The id and title of each screen and action the plugin declares, as a table
/// kept in the program like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_TITLES: &[paperpaw_sdk::TitleEntry] = &native_settings::TITLES;
