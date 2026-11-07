//! Public-only equivalents of Plugin's batch-2 UI call sites (#1049).
use paperpaw_platform::Rect;
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    ui::{Compose, Notify, Open, Param, Question, TextWrite, Ui},
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
fn profiles_rename_and_confirmation(
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
    let widget = ui.widget_make(host, "reading-goal.progress", "core.home", input, out)?;
    if !widget.is_own() {
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
    core::hint::black_box(profiles_rename_and_confirmation);
    core::hint::black_box(menu_editor_notify_and_back);
    core::hint::black_box(reading_goal_widget_layout);
}
