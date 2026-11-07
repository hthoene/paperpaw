use super::*;

fn check_terminal(end: wire::ServiceEnd, rows: usize) -> Result<(), Error> {
    let state = State::default();
    let row = Param::new("key", "value").unwrap();
    let mut records = vec![row.record(); rows];
    records.push(wire::Record::ServiceEnd(end));
    set_stream(&state, &records);
    let bytes = state.reply.borrow();
    let op = crate::native::wire::services_wire::service_schema(schema::KEY, schema::MAJOR)
        .unwrap()
        .op(schema::OP_PARAMS)
        .unwrap();
    let page = UiPage::terminal(&bytes, op, Param::decode)?;
    assert!(page.end().next().is_none());
    assert_eq!(page.count(), rows);
    Ok(())
}

#[test]
fn unpaged_params_require_exact_total_and_zero_continuation_metadata() {
    for rows in [0, 1, 32] {
        check_terminal(
            wire::ServiceEnd {
                flags: 0,
                total: u32::try_from(rows).unwrap(),
                generation: 0,
                next: 0,
            },
            rows,
        )
        .unwrap();
    }
    for (total, next, generation, flags, rows) in [
        (0, 0, 0, 0, 1),
        (1, 1, 0, 0, 1),
        (1, 0, 1, 0, 1),
        (1, 0, 0, 1, 1),
        (33, 0, 0, 0, 33),
    ] {
        assert_eq!(
            check_terminal(
                wire::ServiceEnd {
                    flags,
                    total,
                    generation,
                    next,
                },
                rows,
            ),
            Err(Error::Invalid)
        );
    }
}

#[test]
fn question_refuses_a_well_formed_record_for_another_operation() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    set_reply(
        &state,
        wire::Record::ServiceUiRequest(wire::ServiceUiRequest { request: 4 }),
    );
    assert!(matches!(
        ui.question(
            &host,
            RequestId::new(4).unwrap(),
            &mut [0; 4096],
            &mut [0; 4096],
        ),
        Err(Error::State)
    ));
}

#[test]
fn paged_stack_refuses_stale_generation_and_nonprogressing_pages() {
    let state = State::default();
    let op = crate::native::wire::services_wire::service_schema(schema::KEY, schema::MAJOR)
        .unwrap()
        .op(schema::OP_STACK)
        .unwrap();
    for (generation, total, flags, expected) in [
        (2, 0, 1, Error::State),
        (1, 1, 0, Error::Invalid),
        (0, 0, 1, Error::Invalid),
    ] {
        set_stream(
            &state,
            &[wire::Record::ServiceEnd(wire::ServiceEnd {
                flags,
                total,
                generation,
                next: 0,
            })],
        );
        let bytes = state.reply.borrow();
        let result = UiPage::checked(
            &bytes,
            UiCursor {
                offset: 0,
                generation: 1,
            },
            op,
            |_| Ok(()),
        );
        assert_eq!(result.unwrap_err(), expected);
    }
}
