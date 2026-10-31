//! The first-run setup's offer of the plugins most readers add (ADR
//! 0024, the lean firmware): a step after Wi-Fi with a few of PaperPaw's
//! own plugins, each with a switch, all off. *Done* goes back to the
//! setup and installs those chosen, and so does leaving the page with
//! *Back* or the header's arrow: the switches are the choice, as
//! everywhere else. The setup's card for the step says what became of
//! them.
//!
//! Where PaperPaw loads code plugins (the X4, the X4 Pro) they are
//! fetched from the plugin library one after another in the background,
//! through the store's queue ([`queue::add_quietly`]), and said once when
//! all are done; they run from the next start. Where they are built in
//! but off (Android), they are switched on. The step is left out where
//! none of them could be added, and the store has them all later.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::setup::{SetupPage, SetupStepSpec};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, CloseCause, Color, Command, Cx, Env, FixedScreen, Gesture, GestureKind, Handled, Id,
    ItemState, PluginKind, Rect, RefreshMode, RegisterError, Registrar, Screen,
};

use crate::{queue, trust};

/// The plugins offered, in this order: reading statistics, the widgets
/// for the home screen, Calibre, KOReader's progress sync.
pub(crate) const OFFERED: [&str; 4] = ["stats", "widgets", "calibre", "kosync"];
const SCREEN: Id = Id::new_static("store.offer");
const STEP: Id = Id::new_static("store.setup");

static SCREENS: [FixedScreen; 1] = [FixedScreen::new(SCREEN, "store.offer.title", |env, _| {
    Box::new(OfferScreen::new(env))
})];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_screens(&SCREENS)?;
    r.add_setup_step(
        SetupStepSpec::new(STEP, "store.offer.title", SetupPage::Screen(SCREEN))
            .help("store.offer.help")
            .order(55)
            .applies(Box::new(|env| OFFERED.iter().any(|id| can_add(env, id)))),
    )?;
    // The setup's card for the step: what became of those chosen.
    r.set_item_state(STEP, |env| match badge(env) {
        Some(badge) => ItemState::new().badge(badge),
        None => ItemState::new(),
    })
}

/// Where the plugins chosen on the page are kept, once added.
fn chosen_path() -> String {
    alloc::format!(
        "{}/offer-chosen.txt",
        Env::plugin_dir(crate::PLUGIN.as_str())
    )
}

/// What the setup's card says of the plugins chosen: how many are being
/// added, or that they were; nothing before any was chosen.
fn badge(env: &Env<'_>) -> Option<String> {
    let waiting = queue::waiting(env)
        .iter()
        .filter(|id| OFFERED.contains(&id.as_str()))
        .count();
    if waiting > 0 {
        return Some(
            env.i18n()
                .plural("store.offer.being-added", waiting as i64, &[]),
        );
    }
    env.storage()
        .exists(&chosen_path())
        .then(|| env.i18n().tr("store.offer.added").to_string())
}

/// How plugin `id` can be added here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    /// Built in and switched off: switched on.
    SwitchOn,
    /// From the plugin library, as a code plugin.
    Install,
    /// There already, switched on.
    There,
    /// Not here: neither built in nor loadable as a file.
    No,
}

fn way(env: &Env<'_>, id: &str) -> Way {
    match env.plugins().get(id) {
        Some(info) if info.enabled => Way::There,
        Some(info) if info.kind == PluginKind::BuiltIn => Way::SwitchOn,
        // A code plugin there but off is the plugin manager's.
        Some(_) => Way::There,
        None if env.native().is_some() && env.caps().network => Way::Install,
        None => Way::No,
    }
}

fn can_add(env: &Env<'_>, id: &str) -> bool {
    matches!(way(env, id), Way::SwitchOn | Way::Install)
}

struct OfferScreen {
    /// The plugins that can be added here, and whether each is chosen.
    rows: Vec<(&'static str, bool)>,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// The bar at the bottom: the hint, and *Done*.
    bar: Rect,
    /// The focus is on the button.
    on_button: bool,
    /// Those chosen were added (*Done*, or the page left): not again.
    added: bool,
}

impl OfferScreen {
    fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        // A bar along the bottom: its button keeps the side margin from
        // the edge too.
        let bar_height = ui::edge_bar_height(&style);
        let bar = Rect::new(
            0,
            display.height.saturating_sub(bar_height),
            display.width,
            bar_height,
        );
        let area = Rect::new(
            0,
            top,
            display.width,
            display.height.saturating_sub(top + bar_height),
        );
        Self {
            rows: OFFERED
                .iter()
                .filter(|id| can_add(env, id))
                .map(|id| (*id, false))
                .collect(),
            style,
            header,
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            bar,
            on_button: false,
            added: false,
        }
    }

    fn chosen(&self) -> Vec<&'static str> {
        self.rows
            .iter()
            .filter(|(_, on)| *on)
            .map(|(id, _)| *id)
            .collect()
    }

    /// The *Done* button, always there: the plain way back to the setup.
    fn button(&self, env: &Env<'_>) -> Option<Rect> {
        let font = self.style.font(env)?;
        let label = env.i18n().tr("store.offer.finish");
        let pad = self.style.margin();
        let width = font.measure(self.style.text(), label) as u16 + 2 * pad;
        let band = ui::edge_bar_band(&self.style, self.bar, false);
        Some(Rect::new(
            self.bar.width.saturating_sub(width + pad),
            band.y,
            width,
            band.height,
        ))
    }

    /// The focus went from the last row to the button below, or back:
    /// the two drawn again.
    fn button_left(&self, env: &Env<'_>, cx: &mut Cx<'_, '_>) {
        let row = self.list.row_rect(self.list.focus() % self.list.rows());
        match self.button(env) {
            Some(button) => {
                ui::ListChange::focus(row, ui::button_reach(&self.style, button)).request(cx);
            }
            None => Self::redraw(cx),
        }
    }

    fn redraw(cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    fn toggle(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        if let Some((_, on)) = self.rows.get_mut(index) {
            *on = !*on;
            Self::redraw(cx);
        }
    }

    /// Adds the plugins chosen, if any, and goes back to the setup.
    fn done(&mut self, cx: &mut Cx<'_, '_>) {
        if !self.chosen().is_empty() {
            self.add_chosen(cx);
        }
        cx.command(Command::Back);
    }

    /// Adds the plugins chosen, once, and says so; kept for the setup's
    /// card.
    fn add_chosen(&mut self, cx: &mut Cx<'_, '_>) {
        if core::mem::replace(&mut self.added, true) {
            return;
        }
        let env = *cx.env();
        let chosen = self.chosen();
        let _ = env
            .storage()
            .create_dir(&Env::plugin_dir(crate::PLUGIN.as_str()));
        let _ = env
            .storage()
            .write(&chosen_path(), chosen.join("\n").as_bytes());
        let mut fetch: Vec<String> = Vec::new();
        for id in chosen {
            match way(&env, id) {
                Way::SwitchOn => cx.command(Command::SetPluginEnabled(Id::new_static(id), true)),
                Way::Install => fetch.push(id.to_string()),
                Way::There | Way::No => {}
            }
        }
        if !fetch.is_empty() {
            // This page said what they are: no need to tell it again on
            // a plugin's page.
            trust::tell(env.storage());
            let mut out = paperpaw_sdk::Commands::new();
            queue::add_quietly(&env, &fetch, &mut out);
            out.into_iter().for_each(|command| cx.command(command));
        }
        let key = if fetch.is_empty() {
            "store.offer.switched-on"
        } else {
            "store.offer.adding"
        };
        cx.command(Command::Notify(env.i18n().tr(key).to_string()));
    }
}

impl Screen for OfferScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.rows.len();
        self.list.set_len(len);
        let button = self.button(env);
        self.on_button &= button.is_some();
        self.list.set_active(!self.on_button);
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            env.i18n().tr("store.offer.title"),
            None,
            self.list.paging(len),
        );
        let rows = &self.rows;
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            let (id, on) = rows[index];
            let (name, note) = (
                alloc::format!("store.offer.{id}"),
                alloc::format!("store.offer.{id}.help"),
            );
            let (name, note) = (env.i18n().tr(&name), env.i18n().tr(&note));
            ui::Row::switch(name, on)
                .note(note)
                .focused(focused)
                .draw(ui, rect);
        });
        let line = Rect::new(0, self.bar.y, self.bar.width, (style.line()).max(1));
        ui.canvas().fill(line, Color::gray(150));
        if let Some(button) = button {
            let label = env.i18n().tr("store.offer.finish");
            ui.button(button, label, true, self.on_button);
        }
        // How to choose; once some are chosen, that they are added on
        // going on. Beside the button, on two lines where one is short.
        let hint = if !self.chosen().is_empty() {
            env.i18n().tr("store.offer.hint.chosen")
        } else {
            ui::tr_input(env, "store.offer.hint")
        };
        let right = button.map_or(self.bar.width, |b| b.x);
        let area = Rect::new(
            style.margin(),
            self.bar.y,
            right.saturating_sub(style.gap() + style.margin()),
            self.bar.height,
        );
        ui::Label::new(hint)
            .px(style.small())
            .color(style.faint(ui::Ink::NOTE))
            .lines(2)
            .draw(&mut ui, area);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let env = *cx.env();
        let a = action;
        let len = self.rows.len();
        let back = [actions::UP, actions::LEFT, actions::PREVIOUS].contains(a);
        let forward = [actions::DOWN, actions::RIGHT, actions::NEXT].contains(a);
        if self.on_button {
            if *a == actions::CONFIRM {
                self.done(cx);
            } else if back {
                self.on_button = false;
                self.button_left(&env, cx);
            } else if !forward {
                return Handled::No;
            }
            return Handled::Yes;
        }
        if forward && self.list.focus() + 1 >= len && self.button(&env).is_some() {
            self.on_button = true;
            self.button_left(&env, cx);
            return Handled::Yes;
        }
        let delta = if back {
            -1
        } else if forward {
            1
        } else if *a == actions::CONFIRM {
            self.toggle(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.list.move_focus(delta, len).request(cx);
        Handled::Yes
    }

    /// Left with plugins chosen (*Back*, the header's arrow): they are
    /// added as *Done* adds them.
    fn on_close_request(&mut self, _: CloseCause, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.chosen().is_empty() {
            self.add_chosen(cx);
        }
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let env = *cx.env();
        let paging = self.list.paging(self.rows.len());
        if let Some(action) = self.header.tap(&env, &self.style, gesture.start, paging) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        if self.button(&env).is_some_and(|b| b.contains(gesture.start)) {
            self.done(cx);
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, self.rows.len()) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        self.on_button = false;
        self.toggle(index, cx);
        Handled::Yes
    }
}
