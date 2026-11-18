use super::*;
use crate::native::callbacks::FrameRect as Rect;

#[test]
fn own_widgets_and_scalar_composition_preserve_host_markers_and_errors() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    let mut input = [0; 4096];
    let mut out = [0; 4096];
    let mut make = |instance, flags| {
        set_reply(
            &state,
            wire::Record::ServiceUiInstance(wire::ServiceUiInstance { instance, flags }),
        );
        ui.widget_make(&host, "fixture.clock", "core.home", &mut input, &mut out)
    };
    assert_eq!(make(0, 1), Ok(MadeWidget::Own));
    assert_eq!(make(9, 1), Err(Error::Invalid));
    assert_eq!(make(0, 0), Err(Error::Invalid));
    let Ok(MadeWidget::Widget(widget)) = make(9, 0) else {
        panic!("a host-made instance reads as a widget");
    };
    assert_eq!(widget.instance().get(), 9);
    let rect = Rect::new(-10, 5, 0, u32::MAX);
    for (status, expected) in [
        (abi::ERR_UNSUPPORTED, Error::Unsupported),
        (abi::ERR_BUSY, Error::Busy),
        (abi::ERR_DENIED, Error::Denied),
    ] {
        state.compose_result.set(status);
        assert_eq!(
            Ui::compose(&host, Compose::Widget(widget), rect),
            Err(expected)
        );
        assert_eq!(state.composed.get(), Some((0, 9, rect)));
    }
}

#[test]
fn status_own_hidden_and_drawn_values_keep_distinct_meanings() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    let mut input = [0; 4096];
    let mut out = [0; 4096];
    let mut context = [0; 1024];
    for flags in [2, 4] {
        set_stream(
            &state,
            &[
                wire::Record::ServiceUiStatusValue(wire::ServiceUiStatusValue {
                    item: "fixture.clock",
                    flags,
                    handle: 0,
                    text: "",
                }),
                wire::Record::ServiceEnd(wire::ServiceEnd {
                    flags: 0,
                    total: 1,
                    generation: 0,
                    next: 0,
                    cursor: None,
                }),
            ],
        );
        let mut page = ui
            .status_values(
                &host,
                "fixture.clock",
                &[],
                &mut context,
                &mut input,
                &mut out,
            )
            .unwrap();
        let value = page.next().unwrap().unwrap();
        assert_eq!(value.is_own(), flags == 4);
        assert_eq!(value.is_hidden(), flags == 2);
        assert!(value.draw_handle().is_none());
    }
    set_stream(
        &state,
        &[
            wire::Record::ServiceUiStatusValue(wire::ServiceUiStatusValue {
                item: "fixture.clock",
                flags: 1,
                handle: 88,
                text: "",
            }),
            wire::Record::ServiceEnd(wire::ServiceEnd {
                flags: 0,
                total: 1,
                generation: 0,
                next: 0,
                cursor: None,
            }),
        ],
    );
    let mut page = ui
        .status_values(
            &host,
            "fixture.clock",
            &[],
            &mut context,
            &mut input,
            &mut out,
        )
        .unwrap();
    let handle = page.next().unwrap().unwrap().draw_handle().unwrap();
    Ui::compose(&host, Compose::StatusItem(handle), Rect::new(0, 0, 10, 10)).unwrap();
    assert_eq!(state.composed.get(), Some((1, 88, Rect::new(0, 0, 10, 10))));
}

#[test]
fn own_line_zero_is_not_a_draw_handle_and_foreign_line_zero_is_invalid() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    let mut input = [0; 4096];
    let mut out = [0; 4096];
    let mut context = [0; 1024];
    set_reply(
        &state,
        wire::Record::ServiceUiInstance(wire::ServiceUiInstance {
            instance: 0,
            flags: 1,
        }),
    );
    assert!(matches!(
        ui.status_line(
            &host,
            "fixture.line",
            &[],
            &mut context,
            &mut input,
            &mut out
        ),
        Ok(StatusLine::Own)
    ));
    set_reply(
        &state,
        wire::Record::ServiceUiInstance(wire::ServiceUiInstance {
            instance: 0,
            flags: 0,
        }),
    );
    assert!(matches!(
        ui.status_line(
            &host,
            "fixture.line",
            &[],
            &mut context,
            &mut input,
            &mut out
        ),
        Err(Error::Invalid)
    ));
}
