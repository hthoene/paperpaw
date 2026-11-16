//! The kernel-owned Literata upgrade dialog, on the host's confirmation box.
use super::dialogs::{ConfirmBox, Touch};
use alloc::{boxed::Box, string::String};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, Handled, Hint, HintVisibility, Id, Params, Rect, Screen,
};

pub(crate) const SCREEN: Id = Id::new_static("core.literata-notice");
pub(crate) const ANSWERED: Id = Id::new_static("core.literata-pack-answered");

pub(crate) static SCREENS: [paperpaw_sdk::FixedScreen; 1] = [paperpaw_sdk::FixedScreen::new(
    SCREEN,
    "core.literata-pack.title",
    |env, params| Box::new(Literata::new(env, params)),
)];

pub(crate) struct Literata {
    board: ConfirmBox,
    labels: [String; 2],
    store: bool,
}

impl Literata {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let store = params.get("store") == Some("yes");
        let tr = env.i18n();
        let labels = [
            String::from(tr.tr(if store {
                "core.literata-pack.store"
            } else {
                "core.confirm.ok"
            })),
            String::from(tr.tr("core.literata-pack.dismiss")),
        ];
        let buttons = [labels[0].as_str(), labels[1].as_str()];
        let buttons = if store { &buttons[..] } else { &buttons[..1] };
        let board = ConfirmBox::new(
            env,
            tr.tr("core.literata-pack.title"),
            tr.tr("core.literata-pack.text"),
            buttons,
            usize::from(store),
        );
        Self {
            board,
            labels,
            store,
        }
    }

    fn answer(&self, button: usize, cx: &mut Cx<'_, '_>) {
        cx.close();
        cx.command(Command::Emit(
            ANSWERED,
            Params::new().with(
                "store",
                if self.store && button == 0 {
                    "yes"
                } else {
                    "no"
                },
            ),
        ));
    }
}

impl Screen for Literata {
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
