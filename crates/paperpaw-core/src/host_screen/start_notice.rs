//! The kernel's one-time notices at start (the Literata upgrade hint, the
//! low-memory notice, #798, and the passwords that do not open here, #466): a title, a text and one or two buttons on the
//! host's [`Board`], closing on any answer. What an answer does is the
//! notice's own ([`Answer`]).
use crate::{Place, Turn};
use alloc::string::String;
use alloc::vec::Vec;

use super::board::Board;
use crate::Ask;
use crate::native_host::low_memory::{LATER_WORD, PLUGINS_WORD, TEXT, TITLE};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Canvas, Gesture, Handled, Hint, Id, Params, Rect};

/// What an answer does, after the screen closed.
enum Answer {
    /// The Literata hint: emits `answered` (`store` where the first button
    /// goes to the store page).
    Literata { store: bool },
    /// The low-memory notice: *Plugins* (the first button) opens the plugin
    /// manager's list.
    LowMemory,
    /// Passwords that do not open here: *Remove them* (the first button)
    /// removes them (`secrets_i6::REMOVE`), *OK* keeps them.
    Secrets,
}

/// The plugin manager's list, where the low-memory notice's *Plugins* goes.
const PLUGINS: Id = Id::new_static("plugin-manager.list");

/// A start notice's screen.
pub(crate) struct StartNotice {
    board: Board,
    answer: Answer,
}

impl StartNotice {
    /// The Literata hint: *Store* and *Not now* with a store to go to
    /// (`store` = "yes"), else only *OK*.
    pub(super) fn literata(env: &Place<'_>, params: &Params) -> Self {
        let store = params.get("store") == Some("yes");
        let tr = env.i18n();
        let first = tr.tr(if store {
            "core.literata-pack.store"
        } else {
            "core.confirm.ok"
        });
        let mut labels = Vec::from([String::from(first)]);
        if store {
            labels.push(String::from(tr.tr("core.literata-pack.dismiss")));
        }
        let text = (
            tr.tr("core.literata-pack.title"),
            tr.tr("core.literata-pack.text"),
        );
        Self {
            board: Board::new(env, text, labels, usize::from(store)),
            answer: Answer::Literata { store },
        }
    }

    /// The low-memory notice: its words come in `params` (see `low_memory`).
    pub(super) fn low_memory(env: &Place<'_>, params: &Params) -> Self {
        let get = |key| params.get(key).unwrap_or("");
        let labels = Vec::from([
            String::from(get(PLUGINS_WORD)),
            String::from(get(LATER_WORD)),
        ]);
        Self {
            board: Board::new(env, (get(TITLE), get(TEXT)), labels, 0),
            answer: Answer::LowMemory,
        }
    }

    /// The passwords that do not open here: *Remove them* and *OK*, the focus
    /// on *OK* (and *Back* is *OK*).
    pub(super) fn secrets(env: &Place<'_>) -> Self {
        let tr = env.i18n();
        let labels = Vec::from([
            String::from(tr.tr("core.secrets-unopened.remove")),
            String::from(tr.tr("core.confirm.ok")),
        ]);
        let text = (
            tr.tr("core.secrets-unopened.title"),
            tr.tr("core.secrets-unopened"),
        );
        Self {
            board: Board::new(env, text, labels, 1),
            answer: Answer::Secrets,
        }
    }

    pub(super) fn region(&self) -> Rect {
        self.board.region()
    }

    pub(super) fn hint(&self, action: &Id) -> Hint {
        self.board.hint(action)
    }

    pub(super) fn draw(&self, canvas: &mut Canvas<'_>, env: &Place<'_>) {
        self.board.draw(canvas, env);
    }

    /// Closes, then does what the answer of `button` means.
    fn answer(&self, button: usize, cx: &mut Turn<'_, '_>) {
        cx.close();
        match self.answer {
            Answer::Literata { store } => cx.command(Ask::Emit(
                crate::native_host::literata_notice::ANSWERED,
                Params::new().with("store", if store && button == 0 { "yes" } else { "no" }),
            )),
            Answer::LowMemory if button == 0 => {
                cx.command(Ask::OpenScreen(PLUGINS, Params::new()));
            }
            Answer::LowMemory => {}
            Answer::Secrets if button == 0 => {
                cx.command(Ask::Emit(crate::secrets_i6::REMOVE, Params::new()));
            }
            Answer::Secrets => {}
        }
    }

    pub(super) fn on_action(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            self.answer(self.board.focus(), cx);
        } else if *action == actions::BACK {
            self.answer(1, cx);
        } else if !self.board.step(action, cx) {
            return Handled::No;
        }
        Handled::Yes
    }

    pub(super) fn on_gesture(&mut self, gesture: Gesture, cx: &mut Turn<'_, '_>) -> Handled {
        if let Some(at) = self.board.on_gesture(gesture, cx) {
            self.answer(at, cx);
        }
        Handled::Yes
    }
}
