//! The page of the networks in reach, opened from the Wi-Fi page: choose
//! one to join it (a saved one at once, a secured one after its password
//! is typed); the menu button or a long press forgets a saved one.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, params};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::wifi::{self, WifiCommand, WifiNetwork, WifiState};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, Params, Rect,
    RefreshMode, Screen,
};

use crate::{PASSWORD_ENTERED, Shared};

/// A row of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Network(WifiNetwork),
    /// Looking for networks again.
    Search,
}

pub(crate) struct NetworksPage {
    policy: Shared,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// What the notes of the rows were when their height was measured:
    /// measured again only when they change, not with every key press.
    measured: u64,
}

impl core::fmt::Debug for NetworksPage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NetworksPage").finish_non_exhaustive()
    }
}

/// What is known of a network, in words: joined or saved, its signal,
/// open or not.
fn network_note(env: &Env<'_>, network: &WifiNetwork, state: &WifiState, saved: bool) -> String {
    let mut parts: Vec<&str> = Vec::new();
    match state {
        WifiState::Online { ssid, .. } if *ssid == network.ssid => {
            parts.push(env.i18n().tr("wifi.network.connected"));
        }
        WifiState::Joining { ssid } if *ssid == network.ssid => {
            parts.push(env.i18n().tr("wifi.network.connecting"));
        }
        _ if saved => parts.push(env.i18n().tr("wifi.network.saved")),
        _ => {}
    }
    let signal = match network.bars() {
        4 => "wifi.signal.excellent",
        3 => "wifi.signal.good",
        2 => "wifi.signal.fair",
        _ => "wifi.signal.weak",
    };
    parts.push(env.i18n().tr(signal));
    if !network.secured {
        parts.push(env.i18n().tr("wifi.network.open"));
    }
    parts.join(" · ")
}

/// Looks for networks if the radio is on and nothing else goes on (the
/// Wi-Fi page does when it comes, the networks' page leaves it to that and
/// to its row that looks again: a scan takes the radio's time).
pub(crate) fn search(cx: &mut Cx<'_, '_>) {
    let env = *cx.env();
    if let Some(wifi) = env.wifi()
        && wifi.state.is_on()
        && !wifi.scanning
    {
        cx.command(Command::Wifi(WifiCommand::Scan));
    }
}

/// FNV-1a over `bytes`, continuing from `hash`.
fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

impl NetworksPage {
    pub(crate) fn new(env: &Env<'_>, policy: Shared) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height - top);
        Self {
            policy,
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            measured: 0,
        }
    }

    fn rows(env: &Env<'_>) -> Vec<Row> {
        let mut rows: Vec<Row> = env
            .wifi()
            .filter(|w| w.state.is_on())
            .map(|w| w.networks.iter().cloned().map(Row::Network).collect())
            .unwrap_or_default();
        rows.push(Row::Search);
        rows
    }

    fn changed(cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    /// What choosing a row does.
    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(wifi) = env.wifi() else {
            return;
        };
        let Some(row) = Self::rows(&env).into_iter().nth(index) else {
            return;
        };
        let network = match row {
            Row::Search => {
                search(cx);
                Self::changed(cx);
                return;
            }
            Row::Network(network) => network,
        };
        let mut policy = self.policy.borrow_mut();
        let known = wifi::known(env.secrets().iter());
        let saved = known.iter().find(|(ssid, _)| *ssid == network.ssid);
        if wifi.state.ssid() == Some(&network.ssid) {
            // Joined already (or joining).
        } else if let Some((ssid, password)) = saved {
            for command in policy.choose(&wifi.state, ssid, password) {
                cx.command(Command::Wifi(command));
            }
        } else if network.secured {
            let title = env
                .i18n()
                .format("wifi.password", &[("ssid", Arg::Str(&network.ssid))]);
            // Typed before and not taken (not wrong, e.g. no answer):
            // offered again, not typed anew.
            let typed = policy
                .pending
                .as_ref()
                .filter(|(ssid, _)| *ssid == network.ssid)
                .map(|(_, password)| password.clone())
                .unwrap_or_default();
            cx.command(
                paperpaw_sdk::ask::text(title, &PASSWORD_ENTERED)
                    .password()
                    .initial(typed)
                    .with(params::ID, network.ssid.clone())
                    .command(),
            );
        } else {
            // Not kept: an open network is not joined by itself again
            // (see `policy::best_known`).
            for command in policy.choose(&wifi.state, &network.ssid, "") {
                cx.command(Command::Wifi(command));
            }
        }
        drop(policy);
        Self::changed(cx);
    }

    /// Forgets the saved network in row `index`, leaving it if joined.
    fn forget(&self, index: usize, cx: &mut Cx<'_, '_>) -> bool {
        let env = *cx.env();
        let Some(Row::Network(network)) = Self::rows(&env).into_iter().nth(index) else {
            return false;
        };
        if !wifi::known(env.secrets().iter())
            .iter()
            .any(|(ssid, _)| *ssid == network.ssid)
        {
            return false;
        }
        cx.command(Command::set_secret(wifi::forget(&network.ssid)));
        if env.wifi().and_then(|w| w.state.ssid()) == Some(&network.ssid) {
            cx.command(Command::Wifi(WifiCommand::Leave));
        }
        cx.command(Command::Notify(
            env.i18n()
                .format("wifi.forgotten", &[("ssid", Arg::Str(&network.ssid))]),
        ));
        Self::changed(cx);
        true
    }
}

impl Screen for NetworksPage {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let rows = Self::rows(env);
        let len = rows.len();
        let title = env.i18n().tr("wifi.networks.title");
        let Some(wifi) = env.wifi() else {
            return;
        };
        let known = wifi::known(env.secrets().iter());
        let tr = |key| env.i18n().tr(key);
        // The notes the rows have; their height is as high as the
        // longest needs (the largest text), measured when they change.
        let notes: Vec<String> = rows
            .iter()
            .map(|row| match row {
                Row::Network(network) => {
                    let saved = known.iter().any(|(ssid, _)| *ssid == network.ssid);
                    network_note(env, network, &wifi.state, saved)
                }
                Row::Search if wifi.scanning => String::from(tr("wifi.state.searching")),
                Row::Search if wifi.networks.is_empty() => String::from(tr("wifi.no-networks")),
                Row::Search => String::new(),
            })
            .collect();
        let key = notes.iter().fold(0xcbf2_9ce4_8422_2325, |h, note| {
            fnv(fnv(h, note.as_bytes()), b"\n")
        });
        if key != self.measured {
            self.measured = key;
            let row = style.noted_row_for(
                env,
                self.list.area().width,
                notes.iter().map(String::as_str),
            );
            self.list.set_row(row);
        }
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            let line = match &rows[row] {
                Row::Network(network) => ui::Row::plain(&network.ssid).note(&notes[row]),
                Row::Search => ui::Row::plain(tr("wifi.search")).note(&notes[row]),
            };
            line.focused(focused).draw(ui, rect);
        });
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        let ours = *event == events::WIFI_CHANGED
            || *event == events::WIFI_SCANNED
            || *event == events::SETTING_CHANGED;
        if !ours {
            return;
        }
        // The radio went off (by itself after a while, or from elsewhere):
        // there is nothing to choose from here, so this page goes, not
        // what lies above it (a password being typed stays).
        if !cx.env().wifi().is_some_and(|w| w.state.is_on()) {
            cx.close();
            return;
        }
        Self::changed(cx);
    }

    fn hint(&self, action: &Id, env: &Env<'_>) -> Hint {
        let row = Self::rows(env).into_iter().nth(self.list.focus());
        let saved = |network: &WifiNetwork| {
            wifi::known(env.secrets().iter())
                .iter()
                .any(|(ssid, _)| *ssid == network.ssid)
        };
        match row {
            Some(Row::Network(network)) if *action == actions::MENU && saved(&network) => {
                Hint::Text(String::from(env.i18n().tr("wifi.forget")))
            }
            _ => Hint::Default,
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = Self::rows(cx.env()).len();
        let rows = self.list.rows() as isize;
        let delta = if let Some(delta) = ui::list_step(action, rows) {
            delta
        } else if *action == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
            return Handled::Yes;
        } else if *action == actions::MENU && self.forget(self.list.focus(), cx) {
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.list.move_focus_in(cx.env(), delta, len).request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = Self::rows(cx.env()).len();
        let paging = self.list.paging(len);
        if gesture.kind == GestureKind::Tap
            && let Some(action) = self
                .header
                .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        match gesture.kind {
            GestureKind::Tap => {
                self.activate(index, cx);
                Handled::Yes
            }
            // A long press forgets a saved network (the menu button's
            // part on devices without touch).
            GestureKind::LongPress if self.forget(index, cx) => Handled::Yes,
            _ => Handled::No,
        }
    }
}
