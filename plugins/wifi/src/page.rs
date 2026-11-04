//! The Wi-Fi page (the settings group's own page): the radio's switch
//! with where it is, the setup with a phone (first while no network is
//! kept), the page of the networks in reach (see `networks`), and the
//! plugin's settings.

use alloc::borrow::Cow;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::wifi::{self, WifiCommand, WifiState};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen,
};

use crate::{IDLE_OFF, IDLE_OFF_CHOICES, NETWORKS, ON_DEMAND, Shared, WHILE_CHARGING};

/// A row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Radio,
    /// Setting it up with a phone.
    Phone,
    /// The page of the networks in reach.
    Networks,
    OnDemand,
    IdleOff,
    WhileCharging,
}

pub(crate) struct WifiPage {
    policy: Shared,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// What the page said when it was drawn last (see [`view`]): the
    /// height of its rows is measured again only when that changes, an
    /// event that leaves it as it is draws nothing, and one that changes
    /// some rows draws those.
    seen: ui::RowsSeen,
}

impl core::fmt::Debug for WifiPage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WifiPage").finish_non_exhaustive()
    }
}

/// Where the radio is, in words.
fn state_text(env: &Env<'_>, state: &WifiState, scanning: bool) -> String {
    match state {
        WifiState::Off => env.i18n().tr("wifi.state.off").to_string(),
        WifiState::On if scanning => env.i18n().tr("wifi.state.searching").to_string(),
        WifiState::On => env.i18n().tr("wifi.state.on").to_string(),
        WifiState::Joining { ssid } => env
            .i18n()
            .format("wifi.state.joining", &[("ssid", Arg::Str(ssid))]),
        WifiState::Online { ssid, address, .. } => env.i18n().format(
            "wifi.state.online",
            &[("ssid", Arg::Str(ssid)), ("address", Arg::Str(address))],
        ),
        _ => env.i18n().tr("wifi.state.on").to_string(),
    }
}

/// What the row of the networks says: the network the device is in, if
/// any, and how many are in reach (or that they are being looked for).
fn networks_note(env: &Env<'_>, wifi: &wifi::Wifi) -> String {
    let count = wifi.networks.len();
    let reach = if count == 0 && wifi.scanning {
        env.i18n().tr("wifi.state.searching").to_string()
    } else if count == 0 {
        env.i18n().tr("wifi.no-networks").to_string()
    } else {
        env.i18n()
            .format("wifi.networks.reach", &[("count", Arg::Int(count as i64))])
    };
    match &wifi.state {
        WifiState::Online { ssid, .. } => alloc::format!("{ssid} · {reach}"),
        _ => reach,
    }
}

/// What the page says that changes: where the radio is, the row of the
/// networks, and a key of each row (what it says, besides the focus).
fn view(env: &Env<'_>, rows: &[Row]) -> (String, String, Vec<u64>) {
    let state = env
        .wifi()
        .map(|w| state_text(env, &w.state, w.scanning))
        .unwrap_or_default();
    let networks = env
        .wifi()
        .map(|w| networks_note(env, w))
        .unwrap_or_default();
    let on = env.wifi().is_some_and(|w| w.state.is_on());
    let flag = |on: bool| if on { "1" } else { "0" };
    let key = |parts: &[&str]| ui::RowsSeen::row_key(parts.iter().copied());
    let keys = rows
        .iter()
        .map(|row| match row {
            Row::Radio => key(&["radio", state.as_str(), flag(on)]),
            Row::Phone => key(&["phone"]),
            Row::Networks => key(&["networks", networks.as_str()]),
            Row::OnDemand => key(&["on-demand", flag(env.settings().read(&ON_DEMAND))]),
            Row::WhileCharging => {
                key(&["while-charging", flag(env.settings().read(&WHILE_CHARGING))])
            }
            Row::IdleOff => key(&["idle-off", env.settings().read(&IDLE_OFF)]),
        })
        .collect();
    (state, networks, keys)
}

impl WifiPage {
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
            seen: ui::RowsSeen::fresh(),
        }
    }

    /// The rows: where no network is kept the setup with a phone comes
    /// first, below the radio; else the networks and then the setup.
    fn rows(env: &Env<'_>) -> Vec<Row> {
        let mut rows = alloc::vec![Row::Radio];
        let kept = !wifi::known(env.secrets().iter()).is_empty();
        if !kept {
            rows.push(Row::Phone);
        }
        if env.wifi().is_some_and(|w| w.state.is_on()) {
            rows.push(Row::Networks);
        }
        if kept {
            rows.push(Row::Phone);
        }
        rows.extend([Row::OnDemand, Row::IdleOff, Row::WhileCharging]);
        rows
    }

    /// Rows as high as the notes of the radio and the settings need (the
    /// largest text).
    fn measure(&mut self, env: &Env<'_>, state: &str, networks: &str) {
        let tr = |key| env.i18n().tr(key);
        let notes = [
            state,
            networks,
            tr("wifi.phone.note"),
            tr("wifi.setting.on-demand.help"),
            tr("wifi.setting.while-charging.help"),
            tr("wifi.setting.idle-off.help"),
        ];
        let row = self.style.noted_row_for(env, self.list.area().width, notes);
        self.list.set_row(row);
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
        match row {
            Row::Radio if wifi.state.is_on() => {
                self.policy.borrow_mut().manual = false;
                cx.command(Command::Wifi(WifiCommand::Off));
            }
            Row::Radio => {
                self.policy.borrow_mut().manual = true;
                cx.command(Command::Wifi(WifiCommand::On));
            }
            Row::Networks => {
                cx.command(Command::OpenScreen(NETWORKS, Params::new()));
                return;
            }
            Row::Phone => {
                cx.command(Command::OpenScreen(crate::PHONE, Params::new()));
                return;
            }
            Row::OnDemand | Row::WhileCharging => {
                let id = if row == Row::OnDemand {
                    ON_DEMAND
                } else {
                    WHILE_CHARGING
                };
                let on = env.settings().read(&id);
                cx.command(Command::set(&id, !on));
            }
            Row::IdleOff => {
                let now = env.settings().read(&IDLE_OFF);
                let at = IDLE_OFF_CHOICES
                    .iter()
                    .position(|(choice, _)| *choice == now)
                    .unwrap_or(0);
                let next = IDLE_OFF_CHOICES[(at + 1) % IDLE_OFF_CHOICES.len()].0;
                cx.command(Command::set(&IDLE_OFF, Cow::Borrowed(next)));
            }
        }
        Self::changed(cx);
    }
}

impl Screen for WifiPage {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let rows = Self::rows(env);
        let len = rows.len();
        let title = env.i18n().tr("wifi.title");
        // Rows as high as the notes of the radio and the settings need
        // (the largest text), measured when what the page says changes.
        let (state, networks, keys) = view(env, &rows);
        if self.seen.rows_differ(&keys) {
            self.measure(env, &state, &networks);
            self.seen.see_rows(&self.list, &keys);
        }
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        let Some(wifi) = env.wifi() else {
            return;
        };
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            let tr = |key| env.i18n().tr(key);
            let line = match rows[row] {
                Row::Radio => {
                    return ui::Row::switch(title, wifi.state.is_on())
                        .note(&state)
                        .focused(focused)
                        .draw(ui, rect);
                }
                Row::Networks => ui::Row::value(tr("wifi.networks.title"), "›").note(&networks),
                Row::OnDemand | Row::WhileCharging => {
                    let (id, key) = if rows[row] == Row::OnDemand {
                        (ON_DEMAND, "wifi.setting.on-demand")
                    } else {
                        (WHILE_CHARGING, "wifi.setting.while-charging")
                    };
                    let help = alloc::format!("{key}.help");
                    return ui::Row::switch(tr(key), env.settings().bool(id.as_str()))
                        .note(env.i18n().tr(&help))
                        .focused(focused)
                        .draw(ui, rect);
                }
                Row::Phone => {
                    ui::Row::value(tr("wifi.phone.title"), "›").note(tr("wifi.phone.note"))
                }
                Row::IdleOff => ui::Row::value(
                    tr("wifi.setting.idle-off"),
                    tr(env.settings().read(&IDLE_OFF)),
                )
                .note(tr("wifi.setting.idle-off.help")),
            };
            line.focused(focused).draw(ui, rect);
        });
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        // Fresh networks to count, unless joining one is going on.
        if self.policy.borrow().goal == crate::policy::Goal::Nothing {
            crate::networks::search(cx);
        }
    }

    fn on_event(&mut self, event: &Id, _: &Params, cx: &mut Cx<'_, '_>) {
        let ours = *event == events::WIFI_CHANGED
            || *event == events::WIFI_SCANNED
            || *event == events::SETTING_CHANGED;
        if !ours {
            return;
        }
        // Switched on here: the networks to count.
        let env = *cx.env();
        if *event == events::WIFI_CHANGED
            && env
                .wifi()
                .is_some_and(|w| w.state == WifiState::On && w.networks.is_empty())
            && self.policy.borrow().goal == crate::policy::Goal::Nothing
        {
            crate::networks::search(cx);
        }
        // Drawn again only where the page says something else, and then
        // only the rows that changed (see `view`).
        let (state, networks, keys) = view(&env, &Self::rows(&env));
        if self.seen.rows_differ(&keys) {
            self.measure(&env, &state, &networks);
            self.seen.see_rows(&self.list, &keys).ask_for_it(cx);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = Self::rows(cx.env()).len();
        let rows = self.list.rows() as isize;
        if *action == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
            return Handled::Yes;
        }
        let Some(delta) = ui::list_step(action, rows) else {
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
        if gesture.kind == GestureKind::Tap {
            self.activate(index, cx);
            return Handled::Yes;
        }
        Handled::No
    }
}
