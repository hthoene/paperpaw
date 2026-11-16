//! The screen of the kernel's one-time notices at start (the Literata upgrade
//! hint and the low-memory notice): one title, one text and one or two
//! buttons on the host's `ConfirmBox`, closing on any answer. What an answer
//! does is the notice's own ([`Answer`]); the screen is shared (#798).
use super::dialogs::{ConfirmBox, Touch};
use alloc::string::String;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, Handled, Hint, HintVisibility, Id, Params, Rect, Screen,
};

/// What an answer does, after the screen closed.
enum Answer {
    /// The Literata hint: emits `answered` (`store` where the first button
    /// goes to the store page).
    Literata { store: bool },
    /// The low-memory notice: *Plugins* (the first button) opens the plugin
    /// manager's list.
    LowMemory,
}

/// The plugin manager's list, where the low-memory notice's *Plugins* goes.
const PLUGINS: Id = Id::new_static("plugin-manager.list");

/// A start notice's screen.
pub(crate) struct StartNotice {
    board: ConfirmBox,
    labels: [String; 2],
    answer: Answer,
}

impl StartNotice {
    /// The box of `title` and `text` with `labels` (the second one is not a
    /// button where `buttons` is 1), the button `focus` first.
    fn new(
        env: &Env<'_>,
        (title, text): (&str, &str),
        labels: [String; 2],
        buttons: usize,
        (focus, answer): (usize, Answer),
    ) -> Self {
        let both = [labels[0].as_str(), labels[1].as_str()];
        let board = ConfirmBox::new(env, title, text, &both[..buttons], focus);
        Self {
            board,
            labels,
            answer,
        }
    }

    /// The Literata hint: *Store* and *Not now* with a store to go to
    /// (`store` = "yes"), else only *OK*.
    pub(crate) fn literata(env: &Env<'_>, params: &Params) -> Self {
        let store = params.get("store") == Some("yes");
        let tr = env.i18n();
        let first = tr.tr(if store {
            "core.literata-pack.store"
        } else {
            "core.confirm.ok"
        });
        let labels = [
            String::from(first),
            String::from(tr.tr("core.literata-pack.dismiss")),
        ];
        let text = (
            tr.tr("core.literata-pack.title"),
            tr.tr("core.literata-pack.text"),
        );
        let buttons = if store { 2 } else { 1 };
        Self::new(
            env,
            text,
            labels,
            buttons,
            (usize::from(store), Answer::Literata { store }),
        )
    }

    /// The low-memory notice: its words come in `params` (see `low_memory`).
    pub(crate) fn low_memory(env: &Env<'_>, params: &Params) -> Self {
        use super::low_memory::{LATER_WORD, PLUGINS_WORD, TEXT, TITLE};
        let get = |key| String::from(params.get(key).unwrap_or(""));
        let labels = [get(PLUGINS_WORD), get(LATER_WORD)];
        Self::new(
            env,
            (&get(TITLE), &get(TEXT)),
            labels,
            2,
            (0, Answer::LowMemory),
        )
    }

    /// Closes, then does what the answer of `button` means.
    fn answer(&self, button: usize, cx: &mut Cx<'_, '_>) {
        cx.close();
        match self.answer {
            Answer::Literata { store } => cx.command(Command::Emit(
                super::literata_notice::ANSWERED,
                Params::new().with("store", if store && button == 0 { "yes" } else { "no" }),
            )),
            Answer::LowMemory if button == 0 => {
                cx.command(Command::OpenScreen(PLUGINS, Params::new()));
            }
            Answer::LowMemory => {}
        }
    }
}

impl Screen for StartNotice {
    fn region(&self, env: &Env<'_>) -> Option<Rect> {
        Some(self.board.region(env))
    }
    fn dims_below(&self) -> bool {
        true
    }
    fn hints(&self) -> Option<HintVisibility> {
        Some(HintVisibility::Hidden)
    }
    fn hint(&self, action: &Id, _: &Env<'_>) -> Hint {
        if *action == actions::CONFIRM {
            Hint::Text(self.labels[self.board.focus().min(1)].clone())
        } else {
            Hint::Hidden
        }
    }
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        self.board.draw(canvas, env);
    }
    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            self.answer(self.board.focus(), cx);
        } else if *action == actions::BACK {
            self.answer(1, cx);
        } else if !self.board.on_action(action, cx) {
            return Handled::No;
        }
        Handled::Yes
    }
    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if let Touch::Button(at) = self.board.on_gesture(gesture, cx) {
            self.answer(at, cx);
        }
        Handled::Yes
    }
}
