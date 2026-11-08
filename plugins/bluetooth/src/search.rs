//! Adding a device: what a search finds, the strongest first; choosing
//! one connects and pairs it (the kernel asks the reader to confirm). A
//! remote's or a keyboard's keys go to the key maps once it is paired.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::bluetooth::{self, BtError, BtEvent, BtFailure, Device, DeviceId, ScanPower};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen,
};

use crate::page::kind_text;
use crate::{FOUND, LINK};

/// How long a search runs: the reader's own, so half the time, asking
/// devices for their names.
const SEARCH: core::time::Duration = core::time::Duration::from_secs(30);

/// Where adding the chosen device is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Adding {
    Connecting(DeviceId),
    Pairing(DeviceId),
}

pub(crate) struct Search {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    found: Vec<Device>,
    searching: bool,
    adding: Option<Adding>,
    /// What went wrong last, said below.
    problem: Option<String>,
}

impl Search {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            style,
            header,
            found: Vec::new(),
            searching: false,
            adding: None,
            problem: None,
        }
    }

    /// The rows: what was found, then searching again.
    fn len(&self) -> usize {
        self.found.len() + 1
    }

    fn changed(cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn search(&mut self, cx: &mut Cx<'_, '_>) {
        self.searching = true;
        self.problem = None;
        cx.command(
            bluetooth::scan(&FOUND)
                .lasting(SEARCH)
                .power(ScanPower::Balanced)
                .active()
                .command(),
        );
        Self::changed(cx);
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        if index == self.found.len() {
            self.search(cx);
            return;
        }
        let Some(device) = self.found.get(index) else {
            return;
        };
        if self.adding.is_some() {
            return;
        }
        let id = device.id;
        self.problem = None;
        self.adding = Some(Adding::Connecting(id));
        cx.command(bluetooth::stop_scan(&FOUND));
        cx.command(bluetooth::connect(id, &LINK).command());
        Self::changed(cx);
    }

    /// What happened with the device being added.
    fn link(&mut self, event: BtEvent, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match (event, self.adding) {
            (BtEvent::Connected { device, .. }, Some(Adding::Connecting(adding)))
                if device == adding =>
            {
                self.adding = Some(Adding::Pairing(device));
                cx.command(bluetooth::pair(device));
            }
            (BtEvent::Paired { device, .. }, _) => {
                self.adding = None;
                let typing = self
                    .found
                    .iter()
                    .find(|d| d.id == device)
                    .is_some_and(|d| d.kind.can_type());
                if typing {
                    cx.command(bluetooth::remote_keys(device, &crate::DONE));
                }
                cx.command(bluetooth::disconnect(device));
                let name = self.name_of(&env, device);
                cx.command(Command::Notify(
                    env.i18n()
                        .format("bluetooth.paired", &[("name", Arg::Str(&name))]),
                ));
                cx.command(Command::Back);
            }
            (BtEvent::Failed(BtFailure { error, .. }), Some(_))
            | (
                BtEvent::Disconnected {
                    reason: Some(error),
                    ..
                },
                Some(_),
            ) => {
                self.adding = None;
                let key = match error {
                    BtError::Declined => "bluetooth.refused",
                    _ => "bluetooth.failed",
                };
                self.problem = Some(env.i18n().tr(key).to_string());
            }
            _ => {}
        }
        Self::changed(cx);
    }

    fn name_of(&self, env: &Env<'_>, device: DeviceId) -> String {
        self.found
            .iter()
            .find(|d| d.id == device)
            .and_then(|d| d.name.clone())
            .unwrap_or_else(|| env.i18n().tr("bluetooth.unnamed").to_string())
    }
}

impl Screen for Search {
    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.searching && self.found.is_empty() {
            self.search(cx);
        }
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len();
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            env.i18n().tr("bluetooth.add"),
            None,
            self.list.paging(len),
        );
        let (found, adding, searching) = (&self.found, self.adding, self.searching);
        let problem = self.problem.as_deref();
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            let tr = |key| env.i18n().tr(key);
            let Some(device) = found.get(row) else {
                let note = match problem {
                    Some(problem) => problem,
                    None if searching => tr("bluetooth.searching"),
                    None if found.is_empty() => tr("bluetooth.none-found"),
                    None => "",
                };
                ui::Row::value(tr("bluetooth.search-again"), "")
                    .note(note)
                    .focused(focused)
                    .draw(ui, rect);
                return;
            };
            let name = device
                .name
                .clone()
                .unwrap_or_else(|| tr("bluetooth.unnamed").to_string());
            let state = match adding {
                Some(Adding::Connecting(id)) if id == device.id => tr("bluetooth.connecting"),
                Some(Adding::Pairing(id)) if id == device.id => tr("bluetooth.pairing"),
                _ if device.bonded => tr("bluetooth.paired-before"),
                _ => "",
            };
            let mut note = String::from(kind_text(env, device.kind));
            if !state.is_empty() {
                note.push_str(" · ");
                note.push_str(state);
            }
            ui::Row::value(&name, "›")
                .note(&note)
                .focused(focused)
                .draw(ui, rect);
        });
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        if let Some(found) = FOUND.read(event, params) {
            match found {
                BtEvent::Found(device) => {
                    self.found.retain(|d| d.id != device.id);
                    self.found.push(device);
                    // The strongest first.
                    paperpaw_sdk::sort::sort_by_key(&mut self.found, |d| {
                        core::cmp::Reverse(d.signal.unwrap_or(i8::MIN))
                    });
                }
                BtEvent::ScanEnded => self.searching = false,
                BtEvent::Failed(BtFailure { error, .. }) => {
                    self.searching = false;
                    let key = match error {
                        BtError::Off => "bluetooth.off",
                        _ => "bluetooth.failed",
                    };
                    self.problem = Some(cx.env().i18n().tr(key).to_string());
                }
                _ => {}
            }
            Self::changed(cx);
        } else if let Some(event) = LINK.read(event, params) {
            self.link(event, cx);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        let rows = self.list.rows() as isize;
        let delta = if let Some(delta) = ui::list_step(action, rows) {
            delta
        } else if *action == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.list.move_focus_in(cx.env(), delta, len).request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
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
