//! The choice and the yes or no question (`docs/abi/ui.md` 3.1, kinds 1 and
//! 2) on the slots `core.choice` and `core.confirm`: every plugin's question
//! and the kernel's own. The choice is a sheet at the bottom (its title as a
//! heading, a row for each line of the question, as the menu's sheet); the
//! question is a dialog in the middle of the screen (title, text and its
//! buttons, as Help's). Both answer through `core.ui` op 11: the line's index,
//! or 1 for yes and 0 for no. *Back*, a tap outside, or a close with no answer
//! cancels (the host tells the asker so).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        snapshot,
        ui::{Answer, ButtonHint, HandoverId, QuestionInfo, QuestionKind, RequestId, Ui},
    },
};
use paperpaw_ui_kit::{
    Dialog, Item, ItemList, ItemStep, Messages, Style,
    dialog::Response,
    mem::{SERVICE_BUFFER, buffer, push},
    paint,
    text::copy,
};

/// The buttons of a question, in their order.
const YES: usize = 0;
const NO: usize = 1;

/// The actions whose hint a question hides: they do nothing in the row of
/// hints there (up and down and the page buttons move the focus or scroll).
const HIDDEN: [&str; 5] = [
    "core.up",
    "core.down",
    "core.prev-page",
    "core.next-page",
    "core.menu",
];

/// The dialog the host opened the screen for: the choice or the yes or no
/// question its request asks (one screen for both slots).
#[derive(Debug)]
pub(crate) enum AskView {
    Choice(ChoiceView),
    Confirm(ConfirmView),
}

impl AskView {
    /// The dialog of the request the host opened the screen for.
    #[inline(never)]
    pub(crate) fn open(
        ui: &Ui,
        host: &Host<'_>,
        handover: Option<HandoverId>,
    ) -> Result<Self, Error> {
        let request = request_of(ui, host, handover)?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let question = ui.question(host, request, &mut input, &mut output)?;
        match question.kind() {
            QuestionKind::Choice => Ok(Self::Choice(ChoiceView::of(host, request, question)?)),
            QuestionKind::Confirm => Ok(Self::Confirm(ConfirmView::of(host, request, question)?)),
            _ => Err(Error::Invalid),
        }
    }

    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        match self {
            Self::Choice(view) => view.draw(host, frame),
            Self::Confirm(view) => view.draw(host, frame),
        }
    }

    pub(crate) fn input(
        &mut self,
        ui: &Ui,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        match self {
            Self::Choice(view) => view.input(ui, host, input),
            Self::Confirm(view) => view.input(ui, host, input),
        }
    }
}

/// A choice being answered.
#[derive(Debug)]
pub(crate) struct ChoiceView {
    request: RequestId,
    /// Where the lines start: after the heading, and after what the question
    /// says where it says anything.
    lead: usize,
    /// The title as a heading, what it says, then a row for each line.
    list: ItemList,
    /// Whether the focus shows: with buttons from the start, on a touch
    /// screen after the first button that moves it.
    keys: bool,
    /// Where the sheet was in the last frame: a tap outside it cancels.
    region: Rect,
}

/// A yes or no question being answered.
#[derive(Debug)]
pub(crate) struct ConfirmView {
    request: RequestId,
    title: String,
    text: String,
    /// The buttons' words: yes, then no where there is one.
    labels: Vec<String>,
    focus: usize,
    /// Laid out at the first frame (the screen's size and the look).
    laid: Option<Dialog>,
}

/// The request the host opened this dialog for, read from its hand-over.
fn request_of(ui: &Ui, host: &Host<'_>, handover: Option<HandoverId>) -> Result<RequestId, Error> {
    let handover = handover.ok_or(Error::Invalid)?;
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    let mut request = None;
    for param in ui.params(host, handover, &mut input, &mut output)? {
        if let Some(id) = param?.request()? {
            request = Some(id);
        }
    }
    request.ok_or(Error::Invalid)
}

/// Answers `request` and closes. A request the host ended already (the
/// asker went, a first answer won) is not this dialog's any more: it closes
/// all the same.
fn answer_and_close(
    ui: &Ui,
    host: &Host<'_>,
    request: RequestId,
    answer: Answer,
) -> Result<Handled, Error> {
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    let sent = ui.answer(host, request, answer, &mut input, &mut output);
    close(host)?;
    match sent {
        Err(Error::State) => Ok(Handled::Yes),
        Err(error) => Err(error),
        Ok(()) => Ok(Handled::Yes),
    }
}

/// Closes the dialog with no answer: the host cancels the request.
fn close(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).back()
}

fn redraw(host: &Host<'_>) -> Result<Handled, Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Fast)?;
    Ok(Handled::Yes)
}

impl ChoiceView {
    /// The sheet of a choice.
    fn of(host: &Host<'_>, request: RequestId, question: QuestionInfo<'_>) -> Result<Self, Error> {
        let mut items = Vec::new();
        push(&mut items, Item::heading(question.title())?)?;
        // What the question says, in full ink, not to be chosen (the words of
        // *Reset all settings*, a file's path).
        let mut lead = 1;
        if !question.text().is_empty() {
            let mut said = Item::whole(question.text(), "", "")?;
            if let Item::Row(row) = &mut said {
                row.plain = true;
            }
            push(&mut items, said)?;
            lead = 2;
        }
        for line in question.choices() {
            push(&mut items, Item::row(line, "")?)?;
        }
        let keys = !snapshot::capture_snapshot(host)?.has_touch();
        Ok(Self {
            request,
            lead,
            list: ItemList::new(items),
            keys,
            region: Rect::new(0, 0, 0, 0),
        })
    }

    fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        self.region = self.list.draw_sheet(host, frame, self.keys)?;
        Ok(())
    }

    pub(crate) fn input(
        &mut self,
        ui: &Ui,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        match *input {
            // A button comes first as a key: the host makes it the action.
            Input::Key { .. } => return Ok(Handled::No),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } if !self.region.contains(position) => {
                close(host)?;
                return Ok(Handled::Yes);
            }
            Input::Action {
                action: "core.confirm",
                ..
            } if self.list.items().len() < 2 => {
                // Nothing to choose: closed at once.
                close(host)?;
                return Ok(Handled::Yes);
            }
            Input::Action { action, .. }
                if !self.keys && (moves(action) || action == "core.confirm") =>
            {
                // The first button that moves the focus, or a first
                // *Confirm*, shows it: nothing is chosen that nobody saw.
                self.keys = true;
                return redraw(host);
            }
            _ => {}
        }
        let step = self.list.input(host, input)?;
        match step {
            // The heading is item 0 and what the question says item 1, if it says
            // anything: the lines start after them.
            ItemStep::Chosen(index) => {
                let line =
                    u32::try_from(index.saturating_sub(self.lead)).map_err(|_| Error::Invalid)?;
                answer_and_close(ui, host, self.request, Answer::Pick(line))
            }
            ItemStep::Held(_) => Ok(Handled::Yes),
            other => Ok(ItemList::handled(other)),
        }
    }
}

/// Whether `action` moves the focus of a list.
fn moves(action: &str) -> bool {
    matches!(
        action,
        "core.up"
            | "core.down"
            | "core.left"
            | "core.right"
            | "core.previous"
            | "core.next"
            | "core.prev-page"
            | "core.next-page"
    )
}

impl ConfirmView {
    /// The box of a yes or no question.
    fn of(host: &Host<'_>, request: RequestId, question: QuestionInfo<'_>) -> Result<Self, Error> {
        let mut messages = Messages::new()?;
        let only_yes = question.only_yes();
        let mut word = |own: Option<&str>, key: &str| match own {
            Some(own) => copy(own),
            None => messages.copy(host, key),
        };
        let mut labels = Vec::new();
        let yes_key = if only_yes {
            "core.confirm.ok"
        } else {
            "core.confirm.yes"
        };
        push(&mut labels, word(question.yes_label(), yes_key)?)?;
        if !only_yes {
            push(&mut labels, word(question.no_label(), "core.confirm.no")?)?;
        }
        let view = Self {
            request,
            title: copy(question.title())?,
            text: copy(question.text())?,
            labels,
            focus: if only_yes || question.focus_yes() {
                YES
            } else {
                NO
            },
            laid: None,
        };
        view.hint(host);
        Ok(view)
    }

    /// The row of hints: the focused button's word below *Confirm*; nothing
    /// for the buttons that only move the focus or scroll. A hint the host
    /// refuses leaves the row as it was.
    fn hint(&self, host: &Host<'_>) {
        let mut scratch = [0; 256];
        let mut commands = CommandSender::new(host, &mut scratch);
        if let Some(word) = self.labels.get(self.focus)
            && let Ok(hint) = ButtonHint::words("core.confirm", word)
        {
            let _ = commands.set_hint(hint);
        }
        for action in HIDDEN {
            if let Ok(hint) = ButtonHint::hidden(action) {
                let _ = commands.set_hint(hint);
            }
        }
    }

    /// The dialog, laid out for the screen as it is: in the middle, at the top
    /// when it is taller.
    fn lay_out(&mut self, host: &Host<'_>) -> Result<&mut Dialog, Error> {
        if self.laid.is_none() {
            let display = snapshot::capture_snapshot(host)?;
            let font = Font::acquire_role(host, FontRole::Ui)?;
            let style = Style::read(host, &display, &font)?;
            let (width, height) = (display.display_width(), display.display_height());
            let mut lines: Vec<&str> = Vec::new();
            if !self.text.is_empty() {
                for line in self.text.split('\n') {
                    push(&mut lines, line)?;
                }
            }
            let mut buttons: Vec<&str> = Vec::new();
            for label in &self.labels {
                push(&mut buttons, label.as_str())?;
            }
            let made = Dialog::new(
                &font,
                &style,
                (width, height),
                &self.title,
                &lines,
                &buttons,
            );
            font.release()?;
            let mut dialog = made?;
            dialog.place(height.saturating_sub(dialog.height()) / 2)?;
            self.laid = Some(dialog);
        }
        self.laid.as_mut().ok_or(Error::State)
    }

    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let describing = paint::asking(frame);
        let focus = self.focus;
        let dialog = self.lay_out(host)?;
        dialog.set_describing(describing);
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let mut ops = Vec::new();
        let prepared = dialog.prepare(&mut ops, &font, Some(focus));
        let drawn = prepared.and_then(|()| paint::replay(frame, &font, ops.iter()));
        font.release()?;
        drawn
    }

    pub(crate) fn input(
        &mut self,
        ui: &Ui,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let request = self.request;
        match *input {
            // A button comes first as a key: the host makes it the action.
            Input::Key { .. } => Ok(Handled::No),
            Input::Action {
                action: "core.confirm",
                ..
            } => {
                let answer = if self.focus == YES {
                    Answer::Yes
                } else {
                    Answer::No
                };
                answer_and_close(ui, host, request, answer)
            }
            Input::Action { action, .. } => {
                let focus = self.focus;
                let dialog = self.lay_out(host)?;
                // Where title and text do not fit, the page buttons scroll them.
                match dialog.on_action(action) {
                    Response::Moved => return redraw(host),
                    Response::Held => return Ok(Handled::Yes),
                    Response::Ignored => {}
                }
                // The page buttons (beside the screen on the X4) move along
                // the buttons too, as left and right.
                let along = match action {
                    "core.prev-page" => "core.left",
                    "core.next-page" => "core.right",
                    other => other,
                };
                // Back closes, without an answer; the others do as they do.
                let Some(to) = dialog.step(focus, along) else {
                    return Ok(Handled::No);
                };
                if to != focus {
                    self.focus = to;
                    self.hint(host);
                    return redraw(host);
                }
                Ok(Handled::Yes)
            }
            Input::Gesture { position, kind, .. } => {
                let dialog = self.lay_out(host)?;
                match kind {
                    InputGestureKind::Tap => match dialog.hit(position) {
                        Some(YES) => answer_and_close(ui, host, request, Answer::Yes),
                        Some(_) => answer_and_close(ui, host, request, Answer::No),
                        // Outside the box: closed, without an answer.
                        None if !dialog.panel().contains(position) => {
                            close(host)?;
                            Ok(Handled::Yes)
                        }
                        None => Ok(Handled::Yes),
                    },
                    InputGestureKind::SwipeUp | InputGestureKind::SwipeDown => {
                        let up = kind == InputGestureKind::SwipeUp;
                        match dialog.on_swipe(Point::new(position.x, position.y), up) {
                            Response::Moved => redraw(host),
                            _ => Ok(Handled::Yes),
                        }
                    }
                    _ => Ok(Handled::Yes),
                }
            }
            // A finger's contacts make the gestures above: left to the host.
            _ => Ok(Handled::No),
        }
    }
}
