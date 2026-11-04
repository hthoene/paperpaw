//! Chooses the picture: the pictures (JPEG, PNG, GIF) in the pictures
//! folder, by name, and *No picture* at the head.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Rect, Screen, SettingValue,
};

use crate::settings;

/// Pictures listed at most: a longer folder is cut, not slowed down.
const MOST: usize = 300;

/// The names of the pictures in the pictures folder, in order.
pub(crate) fn pictures(env: &Env<'_>) -> Vec<String> {
    let folder = env
        .settings()
        .text(settings::FOLDER.as_str())
        .trim_matches('/')
        .to_string();
    let mut found: Vec<String> = Vec::new();
    let _ = env.storage().list(&folder, &mut |entry| {
        let name = entry.name.to_lowercase();
        if !entry.is_dir
            && !entry.name.starts_with('.')
            && found.len() < MOST
            && [".jpg", ".jpeg", ".png", ".gif"]
                .iter()
                .any(|e| name.ends_with(e))
        {
            found.push(entry.name.to_string());
        }
    });
    found.sort_unstable_by_key(|n| n.to_lowercase());
    found
}

#[allow(missing_debug_implementations, reason = "a screen")]
pub(crate) struct Picker {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// The pictures; row 0 is *No picture*.
    names: Vec<String>,
    chosen: String,
}

impl Picker {
    pub(crate) fn new(env: &Env<'_>) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let names = pictures(env);
        let chosen = env.settings().text(settings::PICTURE.as_str()).to_string();
        let mut list = ListView::new(area, u32::from(style.row())).focus_on_keys(env);
        if let Some(at) = names.iter().position(|n| *n == chosen) {
            list.set_focus(at + 1);
        }
        Self {
            style,
            header,
            list,
            names,
            chosen,
        }
    }

    fn len(&self) -> usize {
        self.names.len() + 1
    }

    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        let name = match index {
            0 => String::new(),
            n => match self.names.get(n - 1) {
                Some(name) => name.clone(),
                None => return,
            },
        };
        cx.command(Command::SetSetting(
            Id::new_static(settings::PICTURE.static_str()),
            SettingValue::Text(name.into()),
        ));
        cx.command(Command::Back);
    }
}

impl Screen for Picker {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let len = self.len();
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header.draw(
            &mut ui,
            env.i18n().tr("picture-home.setting.picture"),
            None,
            self.list.paging(len),
        );
        let (names, chosen) = (&self.names, &self.chosen);
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            let (title, here) = match index {
                0 => (
                    env.i18n().tr("picture-home.no-picture").to_string(),
                    chosen.is_empty(),
                ),
                n => (names[n - 1].clone(), names[n - 1] == *chosen),
            };
            let row = ui::Row::value(&title, if here { "•" } else { "" });
            row.focused(focused).draw(ui, rect);
        });
        if self.names.is_empty() {
            let folder = env.settings().text(settings::FOLDER.as_str());
            let note = env
                .i18n()
                .tr("picture-home.no-pictures")
                .replace("{folder}", folder);
            let area = self.list.area();
            let below = Rect::new(
                area.x,
                area.y + style.row() + style.gap(),
                area.width,
                area.height.saturating_sub(style.row() + style.gap()),
            );
            ui.empty(
                below,
                "camera",
                env.i18n().tr("picture-home.no-pictures.title"),
                &note,
            );
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.len();
        if *action == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
            return Handled::Yes;
        }
        let rows = self.list.rows() as isize;
        let Some(delta) = ui::list_step(action, rows) else {
            return Handled::No;
        };
        self.list.move_focus_in(cx.env(), delta, len).request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let env = *cx.env();
        let len = self.len();
        let paging = self.list.paging(len);
        if let Some(action) = self.header.tap(&env, &self.style, gesture.start, paging) {
            if action == actions::BACK {
                cx.command(Command::run(action));
                return Handled::Yes;
            }
            return self.on_action(&action, cx);
        }
        match self.list.hit(gesture.start, len) {
            Some(index) => {
                self.list.set_focus(index);
                self.choose(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}
