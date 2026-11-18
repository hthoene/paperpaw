//! Public-only equivalents of Plugin's batch-2 UI call sites (#1049).
use paperpaw_sdk::native::callbacks::FrameRect as Rect;
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    events::Event,
    ui::{
        Answer, Compose, MadeWidget, Notify, Open, Param, Question, RequestId, TextPart, TextWrite,
        Ui,
    },
};

fn keys_parameterized_open(
    host: &Host<'_>,
    ui: &Ui,
    input: &mut [u8],
    out: &mut [u8],
    commands: &mut [u8],
) -> Result<(), Error> {
    let params = [
        Param::new("action", "keys.pick")?,
        Param::new("title", "Binding")?,
    ];
    let handover = ui.handover(host, &params, input, out)?;
    CommandSender::new(host, commands).open(Open::screen("keys.lib-edit")?.handover(handover))
}
fn profiles_rename(
    host: &Host<'_>,
    ui: &Ui,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<(), Error> {
    let request = ui.ask(
        host,
        Question::text("Rename", "New name")?.live()?,
        input,
        out,
    )?;
    ui.text_write(
        host,
        request.text(),
        0,
        "Current name",
        TextWrite::Last,
        input,
        out,
    )?;
    Ok(())
}
fn profiles_confirmation(
    host: &Host<'_>,
    ui: &Ui,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<RequestId, Error> {
    ui.ask(
        host,
        Question::confirm("Delete profile", "Delete this profile?")?,
        input,
        out,
    )
}
fn keys_choice(
    host: &Host<'_>,
    ui: &Ui,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<RequestId, Error> {
    ui.ask(
        host,
        Question::choice("Binding", "Choose an action", "Open\nBack")?,
        input,
        out,
    )
}
fn folder_question(
    host: &Host<'_>,
    ui: &Ui,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<RequestId, Error> {
    let request = ui.ask(
        host,
        Question::folder("Folder", "Choose a folder")?,
        input,
        out,
    )?;
    ui.text_write(
        host,
        request.text(),
        0,
        "/books",
        TextWrite::Last,
        input,
        out,
    )?;
    Ok(request)
}
fn dialog_answer(
    host: &Host<'_>,
    ui: &Ui,
    opening: Event<'_>,
    answer: Answer,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<(), Error> {
    let Event::ScreenOpened {
        handover: Some(handover),
        ..
    } = opening
    else {
        return Err(Error::State);
    };
    let (request, kind) = {
        let mut request = None;
        let mut kind = None;
        for param in ui.params(host, handover, input, out)? {
            let param = param?;
            if let Some(value) = param.request()? {
                request = Some(value);
            }
            if let Some(value) = param.kind()? {
                kind = Some(value);
            }
        }
        (request.ok_or(Error::State)?, kind.ok_or(Error::State)?)
    };
    if ui.question(host, request, input, out)?.kind() != kind {
        return Err(Error::State);
    }
    ui.answer(host, request, answer, input, out)
}
fn reply_outlives_service_handle<'a>(
    host: &Host<'_>,
    request: RequestId,
    input: &mut [u8],
    out: &'a mut [u8],
) -> Result<TextPart<'a>, Error> {
    let ui = Ui::open(host)?;
    let reply = ui.text_read(host, request.text(), 0, input, out)?;
    ui.release(host)?;
    Ok(reply)
}
fn menu_editor_notify_and_back(host: &Host<'_>, commands: &mut [u8]) -> Result<(), Error> {
    let mut sender = CommandSender::new(host, commands);
    sender.notify(Notify::new("Saved")?)?;
    sender.back()
}
fn reading_goal_widget_layout(
    host: &Host<'_>,
    ui: &Ui,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<(), Error> {
    if let MadeWidget::Widget(widget) =
        ui.widget_make(host, "reading-goal.progress", "core.home", input, out)?
    {
        let size = ui.widget_measure(host, widget, 300, input, out)?;
        let rect = Rect::new(0, 0, 300, size.height());
        ui.widget_place(host, widget, rect, input, out)?;
        Ui::compose(host, Compose::Widget(widget), rect)?;
    }
    Ok(())
}
#[test]
fn plugin_callsite_shapes_compile_without_wire_records_or_raw_submission() {
    core::hint::black_box(keys_parameterized_open);
    core::hint::black_box(profiles_rename);
    core::hint::black_box(profiles_confirmation);
    core::hint::black_box(keys_choice);
    core::hint::black_box(folder_question);
    core::hint::black_box(dialog_answer);
    core::hint::black_box(reply_outlives_service_handle);
    core::hint::black_box(menu_editor_notify_and_back);
    core::hint::black_box(reading_goal_widget_layout);
}
