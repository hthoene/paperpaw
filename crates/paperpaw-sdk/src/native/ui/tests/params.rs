use super::*;

#[test]
fn thirty_two_params_and_exact_content_cap_round_trip_without_registration_limit() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    let mut input = [0; 4096];
    let mut out = [0; 4096];
    let keys: Vec<_> = (0..32).map(|i| alloc::format!("p{i}")).collect();
    let key_bytes: usize = keys.iter().map(alloc::string::String::len).sum();
    let text = "x".repeat(1024 - key_bytes);
    let values: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(i, key)| Param::new(key, if i == 0 { &text } else { "" }).unwrap())
        .collect();
    set_reply(
        &state,
        wire::Record::ServiceUiHandle(wire::ServiceUiHandle { handle: 71 }),
    );
    let handover = ui.handover(&host, &values, &mut input, &mut out).unwrap();
    assert_eq!(handover.get(), 71);
    assert_eq!(handover.text().get(), 71);
    assert_eq!(
        state.input.borrow()[0],
        32,
        "the whole counted batch is sent"
    );
    let mut too_many = values.clone();
    too_many.push(Param::new("extra", "").unwrap());
    let calls = state.calls.get();
    assert!(matches!(
        ui.handover(&host, &too_many, &mut input, &mut out),
        Err(Error::NoSpace)
    ));
    assert!(matches!(
        ui.handover(&host, &[values[0], values[0]], &mut input, &mut out),
        Err(Error::Invalid)
    ));
    let over = "x".repeat(1025 - 1);
    assert!(matches!(
        ui.handover(
            &host,
            &[Param::new("k", &over).unwrap()],
            &mut input,
            &mut out
        ),
        Err(Error::NoSpace)
    ));
    assert_eq!(state.calls.get(), calls);
    let mut rows: Vec<_> = values.iter().map(|p| p.record()).collect();
    rows.push(wire::Record::ServiceEnd(wire::ServiceEnd {
        flags: 0,
        total: 32,
        generation: 0,
        next: 0,
    }));
    set_stream(&state, &rows);
    let params = ui
        .params(&host, handover, &mut input, &mut out)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(params, values);
}

#[test]
fn param_syntax_and_caps_are_checked_without_truncation() {
    for key in ["", "core.request", "space key", "Upper"] {
        assert!(matches!(Param::new(key, ""), Err(Error::Invalid)));
    }
    assert!(matches!(
        Param::new(&"x".repeat(33), ""),
        Err(Error::NoSpace)
    ));
    assert!(matches!(
        Param::new("x", &"x".repeat(1025)),
        Err(Error::NoSpace)
    ));
    let value = "界".repeat(341);
    assert_eq!(Param::new("x", &value).unwrap().value(), value);
}

#[test]
fn dialog_parameters_decode_only_canonical_host_values() {
    fn host_param<'a>(key: &'a str, value: &'a str) -> Param<'a> {
        Param::decode(wire::Record::ServiceUiParam(wire::ServiceUiParam {
            key,
            value,
        }))
        .unwrap()
    }
    for raw in [1, u32::MAX] {
        let text = alloc::format!("{raw}");
        assert_eq!(
            host_param("core.request", &text)
                .request()
                .unwrap()
                .unwrap()
                .get(),
            raw
        );
    }
    for value in ["", "0", "01", "+1", "-1", " 1", "1 ", "4294967296", "١"] {
        assert_eq!(
            host_param("core.request", value).request(),
            Err(Error::Invalid)
        );
    }
    for (value, kind) in [
        ("0", QuestionKind::Text),
        ("1", QuestionKind::Choice),
        ("2", QuestionKind::Confirm),
        ("3", QuestionKind::Folder),
    ] {
        assert_eq!(host_param("core.kind", value).kind(), Ok(Some(kind)));
    }
    for value in ["", "00", "01", "+1", "4", "1 "] {
        assert_eq!(host_param("core.kind", value).kind(), Err(Error::Invalid));
    }
    let other = Param::new("other", "not a number").unwrap();
    assert_eq!(other.request(), Ok(None));
    assert_eq!(other.kind(), Ok(None));
}
