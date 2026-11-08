use super::*;

#[test]
fn confirmation_labels_and_flags_round_trip_to_the_dialog() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    let mut input = [0; 4096];
    let mut out = [0; 4096];
    for (labels, only, yes, no) in [
        ("", false, None, None),
        ("\n", false, None, None),
        ("Keep\nDiscard", false, Some("Keep"), Some("Discard")),
        ("\nCancel", false, None, Some("Cancel")),
        ("Continue\n", true, Some("Continue"), None),
        ("", true, None, None),
    ] {
        let question = Question::confirm("Title", "Question")
            .unwrap()
            .confirm_labels(labels)
            .unwrap()
            .focus_yes()
            .unwrap();
        let question = if only {
            question.only_yes().unwrap()
        } else {
            question
        };
        set_reply(
            &state,
            wire::Record::ServiceUiRequest(wire::ServiceUiRequest { request: 41 }),
        );
        let request = ui.ask(&host, question, &mut input, &mut out).unwrap();
        let bytes = state.input.borrow();
        let batch = wire::records_decode(&bytes).unwrap();
        let Some(wire::Record::ServiceUiAsk(ask)) = batch.iter().next() else {
            panic!("ask record")
        };
        assert_eq!(
            (ask.kind, ask.flags, ask.choices),
            (2, 0x200 | if only { 0x100 } else { 0 }, labels)
        );
        set_reply(
            &state,
            wire::Record::ServiceUiQuestion(wire::ServiceUiQuestion {
                kind: ask.kind,
                flags: ask.flags,
                max_bytes: 4096,
                title: ask.title,
                text: ask.text,
                choices: ask.choices,
            }),
        );
        drop(bytes);
        let info = ui.question(&host, request, &mut input, &mut out).unwrap();
        assert_eq!((info.yes_label(), info.no_label()), (yes, no));
        assert_eq!((info.only_yes(), info.focus_yes()), (only, true));
    }
}

#[test]
fn confirmation_rejects_malformed_labels_incompatible_flags_and_byte_overflow() {
    for labels in ["Yes", "Yes\nNo\nExtra", "\n\n"] {
        assert!(matches!(
            Question::confirm("", "").unwrap().confirm_labels(labels),
            Err(Error::Invalid)
        ));
    }
    let at_cap = "é".repeat(64);
    let labels = alloc::format!("{at_cap}\n{at_cap}");
    assert!(
        Question::confirm("", "")
            .unwrap()
            .confirm_labels(&labels)
            .is_ok()
    );
    for labels in [alloc::format!("{at_cap}a\n"), alloc::format!("\n{at_cap}a")] {
        assert!(matches!(
            Question::confirm("", "").unwrap().confirm_labels(&labels),
            Err(Error::NoSpace)
        ));
    }
    assert!(matches!(
        Question::confirm("", "")
            .unwrap()
            .confirm_labels("Yes\nNo")
            .unwrap()
            .only_yes(),
        Err(Error::Invalid)
    ));
    assert!(matches!(
        Question::confirm("", "")
            .unwrap()
            .only_yes()
            .unwrap()
            .confirm_labels("Yes\nNo"),
        Err(Error::Invalid)
    ));
    for question in [
        Question::text("", "").unwrap(),
        Question::choice("", "", "A").unwrap(),
        Question::folder("", "").unwrap(),
    ] {
        assert!(matches!(
            question.confirm_labels("Yes\nNo"),
            Err(Error::Invalid)
        ));
        assert!(matches!(question.only_yes(), Err(Error::Invalid)));
        assert!(matches!(question.focus_yes(), Err(Error::Invalid)));
    }
}

#[test]
fn dialog_refuses_malformed_confirmation_replies() {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    let mut input = [0; 256];
    let mut out = [0; 4096];
    for (kind, flags, choices) in [
        (2, 1, ""),
        (2, 0x400, ""),
        (2, 0, "Yes"),
        (2, 0x100, "Yes\nNo"),
        (0, 0x100, ""),
        (1, 0x200, "One"),
        (3, 0, "Yes\nNo"),
    ] {
        set_reply(
            &state,
            wire::Record::ServiceUiQuestion(wire::ServiceUiQuestion {
                kind,
                flags,
                choices,
                max_bytes: 4096,
                title: "",
                text: "",
            }),
        );
        assert!(matches!(
            ui.question(&host, RequestId::new(1).unwrap(), &mut input, &mut out),
            Err(Error::Invalid)
        ));
    }
}
