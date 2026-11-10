#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use crate::native::declare::records_wire::Record;
use alloc::format;
use wire::{
    EventActionRun, EventPageTurned, EventScreenShown, EventServedRequest, EventServer,
    InputAction, Registration, records_encode,
};

// Same scalar vectors as core's abi7_events::simple_events_round_trip_with_distinct_tags.
#[test]
fn host_scalar_vectors_decode_with_distinct_tags() {
    for now_ms in [0_u64, 1, u64::MAX] {
        for tag in [32, 33, 34, 35, 36] {
            let mut bytes = [0; 12];
            bytes[..4].copy_from_slice(&[1, tag, 9, 0]);
            bytes[4..].copy_from_slice(&now_ms.to_le_bytes());
            let expected = match tag {
                32 => Event::Started { now_ms },
                33 => Event::Resume { now_ms },
                34 => Event::Suspend { now_ms },
                35 => Event::Stopping { now_ms },
                _ => Event::BatteryMeasured { now_ms },
            };
            assert_eq!(decode(&bytes), Ok(expected));
            for end in 0..bytes.len() {
                assert!(decode(&bytes[..end]).is_err());
            }
        }
    }
}
fn page() -> EventPageTurned<'static> {
    EventPageTurned {
        now_ms: 0x0102_0304_0506_0708,
        section: 3,
        offset: 4000,
        insert: Some(2),
        progress_ppm: 250_000,
        path: "books/a.epub",
        title: "A Title",
        chapter: "One",
        page: Some(12),
        pages: Some(300),
        chapter_left: None,
    }
}
#[test]
fn page_vectors_preserve_counters_and_borrow_input_strings() {
    let mut bytes = [0; 1200];
    for (p, n) in [
        (None, None),
        (Some(1), None),
        (None, Some(9)),
        (Some(12), Some(300)),
    ] {
        let record = EventPageTurned {
            page: p,
            pages: n,
            ..page()
        };
        let len = records_encode(&[Record::EventPageTurned(record)], &mut bytes).unwrap();
        let Event::PageTurned(got) = decode(&bytes[..len]).unwrap() else {
            panic!()
        };
        assert_eq!(
            (
                got.now_ms(),
                got.section(),
                got.offset(),
                got.progress_ppm()
            ),
            (record.now_ms, 3, 4000, 250_000)
        );
        assert_eq!(
            (got.path(), got.title(), got.chapter()),
            (record.path, record.title, record.chapter)
        );
        assert_eq!((got.page(), got.pages(), got.insert()), (p, n, 2));
        assert!(bytes.as_ptr_range().contains(&got.path().as_ptr()));
        assert!(bytes.as_ptr_range().contains(&got.title().as_ptr()));
        assert!(bytes.as_ptr_range().contains(&got.chapter().as_ptr()));
    }
}
#[test]
fn chapter_left_is_read_present_and_absent_and_bounded_by_the_pages_after_this_one() {
    let mut bytes = [0; 1200];
    // Page 12 of 300: 288 pages follow it; 0 is the chapter's last page.
    for left in [None, Some(0), Some(1), Some(288)] {
        let record = EventPageTurned {
            chapter_left: left,
            ..page()
        };
        let len = records_encode(&[Record::EventPageTurned(record)], &mut bytes).unwrap();
        let Event::PageTurned(got) = decode(&bytes[..len]).unwrap() else {
            panic!()
        };
        assert_eq!(got.chapter_left(), left);
        assert_eq!((got.page(), got.pages()), (Some(12), Some(300)));
    }
    // On the last page nothing follows: only 0.
    let last = EventPageTurned {
        page: Some(300),
        chapter_left: Some(0),
        ..page()
    };
    let len = records_encode(&[Record::EventPageTurned(last)], &mut bytes).unwrap();
    assert!(decode(&bytes[..len]).is_ok());
    for refused in [
        // Past the pages after this one, and past them on the last page.
        EventPageTurned {
            chapter_left: Some(289),
            ..page()
        },
        EventPageTurned {
            page: Some(300),
            chapter_left: Some(1),
            ..page()
        },
        // Without the page, without the page count, or without either.
        EventPageTurned {
            page: None,
            chapter_left: Some(0),
            ..page()
        },
        EventPageTurned {
            pages: None,
            chapter_left: Some(0),
            ..page()
        },
        EventPageTurned {
            page: None,
            pages: None,
            chapter_left: Some(0),
            ..page()
        },
    ] {
        let n = records_encode(&[Record::EventPageTurned(refused)], &mut bytes).unwrap();
        assert_eq!(decode(&bytes[..n]), Err(Error::Invalid), "{refused:?}");
    }
}
#[test]
fn malformed_and_non_event_batches_are_refused() {
    let mut bytes = [0; 1200];
    for record in [
        EventPageTurned {
            progress_ppm: 1_000_001,
            ..page()
        },
        EventPageTurned {
            page: Some(0),
            ..page()
        },
        EventPageTurned {
            pages: Some(0),
            ..page()
        },
        EventPageTurned {
            page: Some(301),
            ..page()
        },
    ] {
        let n = records_encode(&[Record::EventPageTurned(record)], &mut bytes).unwrap();
        assert_eq!(decode(&bytes[..n]), Err(Error::Invalid));
    }
    let r = Record::Registration(Registration::Event {
        event_tag: 32,
        token: 1,
    });
    let n = records_encode(&[r], &mut bytes).unwrap();
    assert_eq!(decode(&bytes[..n]), Err(Error::Unsupported));
    let n = records_encode(&[r, r], &mut bytes).unwrap();
    assert_eq!(decode(&bytes[..n]), Err(Error::Invalid));
    assert_eq!(decode(&[0]), Err(Error::Invalid));
    assert_eq!(decode(&[1, 63, 1, 0]), Err(Error::Unsupported));
}
#[test]
fn forward_fields_are_accepted_and_trailing_batch_bytes_are_refused() {
    let bytes = [1, 32, 12, 1, 1, 7, 0, 0, 0, 0, 0, 0, 0, 99, 42];
    assert_eq!(decode(&bytes), Ok(Event::Started { now_ms: 7 }));
    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert_eq!(decode(&trailing), Err(Error::Invalid));
    let mut bytes = [0; 1200];
    let len = records_encode(&[Record::EventPageTurned(page())], &mut bytes).unwrap();
    let position = bytes[..len].windows(12).position(|s| s == b"books/a.epub");
    // The path is 12 bytes; corrupt its leading byte without changing framing.
    bytes[position.unwrap()] = 255;
    assert_eq!(decode(&bytes[..len]), Err(Error::Utf8));
}

#[test]
fn page_insert_ordinals_preserve_source_position_and_progress() {
    let mut bytes = [0; 1200];
    for insert in [None, Some(0), Some(1), Some(u32::MAX)] {
        let record = EventPageTurned { insert, ..page() };
        let len = records_encode(&[Record::EventPageTurned(record)], &mut bytes).unwrap();
        let Event::PageTurned(got) = decode(&bytes[..len]).unwrap() else {
            panic!()
        };
        assert_eq!(got.insert(), insert.unwrap_or(0));
        assert_eq!(
            (got.section(), got.offset(), got.progress_ppm()),
            (record.section, record.offset, record.progress_ppm)
        );
        assert_eq!((got.page(), got.pages()), (record.page, record.pages));
    }
}

#[test]
fn screen_and_action_events_borrow_the_exact_input_fields() {
    let mut bytes = [0; 256];
    let now_ms = 0x1234_5678_9abc_def0;
    let screen = Record::EventScreenShown(EventScreenShown {
        last_input: 0,
        now_ms,
        screen: "demo.main",
        width: 0x1234_5678,
        height: 0x9abc_def0,
        instance: None,
    });
    let len = records_encode(&[screen], &mut bytes).unwrap();
    assert_eq!(bytes[1], 38);
    let Event::ScreenShown {
        now_ms: got_ms,
        screen,
        width,
        height,
        instance,
        last_input,
    } = decode(&bytes[..len]).unwrap()
    else {
        panic!()
    };
    assert_eq!((instance, last_input), (None, None));
    assert_eq!(
        (got_ms, screen, width, height),
        (now_ms, "demo.main", 0x1234_5678, 0x9abc_def0)
    );
    assert!(bytes[..len].as_ptr_range().contains(&screen.as_ptr()));
    let action = Record::EventActionRun(EventActionRun {
        now_ms,
        action: "demo.refresh",
        flags: 0,
    });
    let len = records_encode(&[action], &mut bytes).unwrap();
    assert_eq!(bytes[1], 39);
    let Event::ActionRun {
        now_ms: got_ms,
        action,
        background,
    } = decode(&bytes[..len]).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (got_ms, action, background),
        (now_ms, "demo.refresh", false)
    );
    assert!(bytes[..len].as_ptr_range().contains(&action.as_ptr()));
}

#[test]
fn action_run_says_whether_the_reader_ran_it_and_refuses_unknown_bits() {
    let run = |flags| {
        let mut bytes = [0; 64];
        let record = Record::EventActionRun(EventActionRun {
            now_ms: 7,
            action: "demo.fetch",
            flags,
        });
        let len = records_encode(&[record], &mut bytes).unwrap();
        decode(&bytes[..len]).map(|event| match event {
            Event::ActionRun { background, .. } => background,
            _ => panic!(),
        })
    };
    assert!(!run(0).unwrap());
    assert!(run(1).unwrap());
    assert!(run(2).is_err() && run(u32::MAX).is_err());
}

#[test]
fn screen_shown_says_what_was_used_last_and_refuses_unknown_values() {
    use crate::keys::InputMode;
    let shown = |last_input| {
        let mut bytes = [0; 128];
        let record = Record::EventScreenShown(EventScreenShown {
            last_input,
            now_ms: 1,
            screen: "demo.main",
            width: 480,
            height: 800,
            instance: None,
        });
        let len = records_encode(&[record], &mut bytes).unwrap();
        decode(&bytes[..len]).map(|event| match event {
            Event::ScreenShown { last_input, .. } => last_input,
            _ => panic!("not screen_shown"),
        })
    };
    assert_eq!(shown(0), Ok(None));
    assert_eq!(shown(1), Ok(Some(InputMode::Buttons)));
    assert_eq!(shown(2), Ok(Some(InputMode::Touch)));
    assert_eq!(shown(3), Ok(Some(InputMode::Pen)));
    assert_eq!(shown(4), Err(Error::Invalid));
}

#[test]
fn screen_and_action_events_refuse_bad_framing_and_utf8() {
    for record in [
        Record::EventScreenShown(EventScreenShown {
            last_input: 0,
            now_ms: 9,
            screen: "demo.main",
            width: 800,
            height: 600,
            instance: None,
        }),
        Record::EventActionRun(EventActionRun {
            now_ms: 13,
            action: "demo.refresh",
            flags: 0,
        }),
    ] {
        let mut bytes = [0; 256];
        let len = records_encode(&[record], &mut bytes).unwrap();
        for end in 0..len {
            assert!(decode(&bytes[..end]).is_err());
        }
        let mut trailing = bytes[..len].to_vec();
        trailing.push(0);
        assert_eq!(decode(&trailing), Err(Error::Invalid));
        let position = bytes[..len].windows(5).position(|s| s == b"demo.").unwrap();
        bytes[position] = 255;
        assert_eq!(decode(&bytes[..len]), Err(Error::Utf8));
        let len = records_encode(&[record, record], &mut bytes).unwrap();
        assert_eq!(decode(&bytes[..len]), Err(Error::Invalid));
    }
}

#[test]
fn event_decoding_uses_exact_tags_for_growth_and_input_records() {
    let mut bytes = [0; 64];
    let len = records_encode(
        &[Record::InputAction(InputAction {
            now_ms: 17,
            action: "demo.refresh",
        })],
        &mut bytes,
    )
    .unwrap();
    assert_eq!(decode(&bytes[..len]), Err(Error::Unsupported));
    // Unassigned tags (64-89 are core, registry and UIX events now) stay unsupported even with
    // valid framing.
    for unknown in [
        &[1, 63, 1, 0][..],
        &[1, 126, 1, 0][..],
        &[1, 127, 1, 0][..],
        &[1, 191, 1, 1, 0][..],
    ] {
        assert_eq!(decode(unknown), Err(Error::Unsupported));
    }
}

#[test]
fn generated_tick_vectors_decode_and_reject_every_truncation() {
    let mut bytes = [0; 12];
    for now_ms in [0_u64, u64::MAX] {
        let len = wire::tick_encode(now_ms, &mut bytes).unwrap();
        assert_eq!(Event::decode(&bytes[..len]), Ok(Event::Tick { now_ms }));
        for end in 0..len {
            assert_eq!(Event::decode(&bytes[..end]), Err(Error::Invalid));
        }
    }
}

#[test]
fn tick_optional_extensions_are_generated_decoder_compatible() {
    // One appended bitmap byte and two appended field bytes are forward-compatible.
    let bytes = [1, 3, 12, 1, 1, 7, 0, 0, 0, 0, 0, 0, 0, 99, 42];
    assert_eq!(wire::tick_decode(&bytes), Ok(7));
    assert_eq!(Event::decode(&bytes), Ok(Event::Tick { now_ms: 7 }));
    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert_eq!(Event::decode(&trailing), Err(Error::Invalid));
}

#[test]
fn tick_fallback_preserves_exact_tags_and_singleton_framing() {
    let mut bytes = [0; 12];
    let len = wire::tick_encode(17, &mut bytes).unwrap();
    for tag in [63, 126, 127] {
        bytes[1] = tag;
        assert_eq!(Event::decode(&bytes[..len]), Err(Error::Unsupported));
    }
    bytes[1] = 3;
    bytes[0] = 2;
    assert_eq!(Event::decode(&bytes[..len]), Err(Error::Invalid));
    let record = Record::Registration(Registration::Draw { token: 3 });
    let len = records_encode(&[record], &mut bytes).unwrap();
    assert_eq!(Event::decode(&bytes[..len]), Err(Error::Unsupported));
}

#[test]
fn stage_call_decodes_exact_tag_and_borrows_opaque_payload() {
    let record = Record::StageCall(wire::StageCall {
        stage: "reader.insert",
        schema: 7,
        payload: &[255, 0, 128],
    });
    let mut bytes = [0; 100];
    let n = records_encode(&[record], &mut bytes).unwrap();
    let Event::StageCall {
        stage,
        schema,
        payload,
    } = decode(&bytes[..n]).unwrap()
    else {
        panic!("expected stage call");
    };
    assert_eq!(
        (stage, schema, payload),
        ("reader.insert", 7, &[255, 0, 128][..])
    );
    assert!(bytes.as_ptr_range().contains(&payload.as_ptr()));
    for end in 0..n {
        assert!(decode(&bytes[..end]).is_err());
    }
    bytes[1] = 129; // tag 513 must never decode as a stage call.
    assert!(decode(&bytes[..n]).is_err());
}

#[test]
fn registry_hints_preserve_unknown_bits_and_expose_every_known_section() {
    let mut bytes = [0; 64];
    let cases = [
        (1, RegistryChanges::menus as fn(RegistryChanges) -> bool),
        (1 << 1, RegistryChanges::bindings),
        (1 << 2, RegistryChanges::slots),
        (1 << 3, RegistryChanges::actions_or_screens),
        (1 << 4, RegistryChanges::setup),
        (1 << 5, RegistryChanges::groups),
        (1 << 6, RegistryChanges::presets),
        (1 << 7, RegistryChanges::widgets),
        (1 << 31, RegistryChanges::dropped),
    ];
    for (bit, accessor) in cases {
        let what = bit | (1 << 20);
        let len = records_encode(
            &[Record::EventRegistryChanged(wire::EventRegistryChanged {
                now_ms: 12,
                what,
            })],
            &mut bytes,
        )
        .unwrap();
        let Event::RegistryChanged {
            now_ms,
            what: changes,
        } = Event::decode(&bytes[..len]).unwrap()
        else {
            panic!("expected registry hint");
        };
        assert_eq!(now_ms, 12);
        assert_eq!(changes.bits(), what);
        assert!(accessor(changes));
        assert!(!accessor(RegistryChanges::from_bits(1 << 20)));
    }
    let len = records_encode(
        &[Record::EventPreviewChanged(wire::EventPreviewChanged {
            now_ms: 42,
        })],
        &mut bytes,
    )
    .unwrap();
    assert_eq!(
        Event::decode(&bytes[..len]),
        Ok(Event::PreviewChanged { now_ms: 42 })
    );
}

#[test]
fn plugin_request_ids_borrow_input_and_keep_order_at_the_generated_limit() {
    let mut bytes = [0; 2048];
    let ids = ["dictionary"; wire::EVENT_PLUGINS_REQUESTED_PLUGINS_MAX_ITEMS];
    let joined = ids.join("\n");
    let len = records_encode(
        &[Record::EventPluginsRequested(wire::EventPluginsRequested {
            now_ms: 14,
            request: 7,
            by: "keys",
            plugins: &joined,
        })],
        &mut bytes,
    )
    .unwrap();
    let Event::PluginsRequested {
        now_ms,
        request,
        by,
        plugins,
    } = Event::decode(&bytes[..len]).unwrap()
    else {
        panic!("expected plugin request");
    };
    assert_eq!((now_ms, request.get(), by), (14, 7, "keys"));
    assert_eq!(plugins.len(), ids.len());
    assert!(!plugins.is_empty());
    assert_eq!(plugins.iter().collect::<alloc::vec::Vec<_>>(), ids);
    assert!(bytes.as_ptr_range().contains(&by.as_ptr()));
    for id in plugins.iter() {
        assert!(bytes.as_ptr_range().contains(&id.as_ptr()));
    }
}

#[test]
fn malformed_plugin_request_lists_and_excess_items_are_refused() {
    let mut bytes = [0; 2048];
    let excess = ["dictionary"; wire::EVENT_PLUGINS_REQUESTED_PLUGINS_MAX_ITEMS + 1].join("\n");
    let overlong = "a".repeat(wire::EVENT_PLUGINS_REQUESTED_PLUGINS_ITEM_MAX_BYTES + 1);
    for plugins in [
        "",
        "dictionary\n",
        "\ndictionary",
        "dictionary\n\nkeys",
        excess.as_str(),
        overlong.as_str(),
    ] {
        let len = records_encode(
            &[Record::EventPluginsRequested(wire::EventPluginsRequested {
                now_ms: 14,
                request: 7,
                by: "keys",
                plugins,
            })],
            &mut bytes,
        )
        .unwrap();
        assert_eq!(
            Event::decode(&bytes[..len]),
            Err(Error::Invalid),
            "{plugins:?}"
        );
    }
}

#[test]
fn plugin_answers_share_typed_outcomes_and_reserved_values_mean_failed() {
    use crate::native::commands::PluginOutcome;
    let mut bytes = [0; 256];
    for (outcome, expected) in [
        (0, PluginOutcome::Installed),
        (1, PluginOutcome::Declined),
        (2, PluginOutcome::Failed),
        (3, PluginOutcome::NotFound),
        (4, PluginOutcome::Failed),
        (u32::MAX, PluginOutcome::Failed),
    ] {
        let len = records_encode(
            &[Record::EventPluginAnswered(wire::EventPluginAnswered {
                now_ms: 42,
                request: 17,
                plugin: "dictionary",
                outcome,
            })],
            &mut bytes,
        )
        .unwrap();
        let Event::PluginAnswered {
            now_ms,
            request,
            plugin,
            outcome,
        } = Event::decode(&bytes[..len]).unwrap()
        else {
            panic!("expected plugin answer");
        };
        assert_eq!(
            (now_ms, request.get(), plugin, outcome),
            (42, 17, "dictionary", expected)
        );
        assert!(bytes.as_ptr_range().contains(&plugin.as_ptr()));
    }
}

#[test]
fn host_plugin_ids_use_wire_caps_instead_of_the_current_loader_grammar() {
    let mut bytes = [0; 512];
    let id = "F".repeat(wire::EVENT_PLUGINS_REQUESTED_PLUGINS_ITEM_MAX_BYTES);
    assert!(id.len() > 40);
    let len = records_encode(
        &[Record::EventPluginsRequested(wire::EventPluginsRequested {
            now_ms: 1,
            request: 2,
            by: &id,
            plugins: &id,
        })],
        &mut bytes,
    )
    .unwrap();
    let Event::PluginsRequested { by, plugins, .. } = Event::decode(&bytes[..len]).unwrap() else {
        panic!("expected request");
    };
    assert_eq!(by, id);
    assert_eq!(
        plugins.iter().collect::<alloc::vec::Vec<_>>(),
        [id.as_str()]
    );
    assert!(bytes.as_ptr_range().contains(&by.as_ptr()));
    assert!(
        bytes
            .as_ptr_range()
            .contains(&plugins.iter().next().unwrap().as_ptr())
    );
    let len = records_encode(
        &[Record::EventPluginAnswered(wire::EventPluginAnswered {
            now_ms: 1,
            request: 2,
            plugin: &id,
            outcome: 0,
        })],
        &mut bytes,
    )
    .unwrap();
    let Event::PluginAnswered { plugin, .. } = Event::decode(&bytes[..len]).unwrap() else {
        panic!("expected answer");
    };
    assert_eq!(plugin, id);
    assert!(bytes.as_ptr_range().contains(&plugin.as_ptr()));
}

#[test]
fn lookup_ready_names_the_provider_that_kept_its_answer() {
    let mut bytes = [0; 128];
    let n = wire::records_encode(
        &[wire::Record::EventLookupReady(wire::EventLookupReady {
            now_ms: 9,
            provider: "wikipedia.lookup",
        })],
        &mut bytes,
    )
    .unwrap();
    assert_eq!(
        Event::decode(&bytes[..n]),
        Ok(Event::LookupReady {
            now_ms: 9,
            provider: "wikipedia.lookup"
        })
    );
    // A provider name of the contract's full 64 bytes decodes whole.
    let mut long = bytes;
    let n = wire::records_encode(
        &[wire::Record::EventLookupReady(wire::EventLookupReady {
            now_ms: 9,
            provider: &"p".repeat(64),
        })],
        &mut long,
    )
    .unwrap();
    assert!(Event::decode(&long[..n]).is_ok());
}

#[test]
fn typed_text_reads_its_text_and_done_flag_and_refuses_other_bits() {
    let encode = |flags| {
        let mut bytes = [0; 128];
        let n = wire::records_encode(
            &[wire::Record::EventTextTyped(wire::EventTextTyped {
                now_ms: 4,
                text: "héllo wörld",
                flags,
            })],
            &mut bytes,
        )
        .unwrap();
        bytes[..n].to_vec()
    };
    assert_eq!(
        Event::decode(&encode(0)),
        Ok(Event::TextTyped {
            now_ms: 4,
            text: TypedText::new("héllo wörld"),
            done: false
        })
    );
    // The text may be a password: `Debug` shows its length, never the text.
    let shown = format!("{:?}", Event::decode(&encode(0)).unwrap());
    assert!(
        shown.contains("len: 13") && !shown.contains("llo"),
        "{shown}"
    );
    assert!(matches!(
        Event::decode(&encode(1)),
        Ok(Event::TextTyped { done: true, .. })
    ));
    for flags in [2, 3, 0x8000_0000] {
        assert_eq!(
            Event::decode(&encode(flags)),
            Err(Error::Invalid),
            "{flags}"
        );
    }
}

#[test]
fn server_events_tell_started_failed_and_stopped_with_their_reasons() {
    use crate::native::net::{Reason, ServerState};
    let mut bytes = [0; 256];
    let cases = [
        (
            0,
            0,
            "192.168.1.20:8080",
            ServerState::Started {
                address: "192.168.1.20:8080",
            },
        ),
        (0, 0, "", ServerState::Started { address: "" }),
        (
            1,
            5,
            "",
            ServerState::Failed {
                reason: Reason::Refused,
            },
        ),
        (
            2,
            1,
            "",
            ServerState::Stopped {
                reason: Reason::NoNetwork,
            },
        ),
        (2, 0, "", ServerState::Stopped { reason: Reason::Ok }),
        (9, 0, "", ServerState::Other(9)),
    ];
    for (state, reason, address, want) in cases {
        let record = Record::EventServer(EventServer {
            now_ms: 7,
            server: 3,
            state,
            reason,
            address,
        });
        let len = records_encode(&[record], &mut bytes).unwrap();
        let Event::Server(got) = decode(&bytes[..len]).unwrap() else {
            panic!("not a server event")
        };
        assert_eq!((got.now_ms(), got.server(), got.state()), (7, 3, want));
    }
}

#[test]
fn a_served_request_borrows_its_text_and_tells_a_body_only_when_flagged() {
    let mut bytes = [0; 1400];
    for (flags, body, want) in [(1, 5_000_000_000, Some(5_000_000_000)), (0, 0, None)] {
        let record = Record::EventServedRequest(EventServedRequest {
            now_ms: 7,
            server: 3,
            request: 77,
            method: "POST",
            target: "/upload?x=1",
            flags,
            body_len: body,
        });
        let len = records_encode(&[record], &mut bytes).unwrap();
        let Event::ServedRequest(got) = decode(&bytes[..len]).unwrap() else {
            panic!("not a served request")
        };
        assert_eq!(
            (
                got.server(),
                got.request(),
                got.method(),
                got.target(),
                got.body_len()
            ),
            (3, 77, "POST", "/upload?x=1", want)
        );
        assert!(bytes.as_ptr_range().contains(&got.target().as_ptr()));
    }
}

#[test]
fn a_served_request_tells_the_body_by_its_flag_and_never_prints_its_target() {
    let long = "/".repeat(1024);
    let mut bytes = [0; 1400];
    let cases = [(0, 99, None), (1, 0, Some(0)), (1 | 0b110, 12, Some(12))];
    for (flags, body, want) in cases {
        let record = Record::EventServedRequest(EventServedRequest {
            now_ms: 7,
            server: 3,
            request: 0,
            method: "GET",
            target: &long,
            flags,
            body_len: body,
        });
        let len = records_encode(&[record], &mut bytes).unwrap();
        let Event::ServedRequest(got) = decode(&bytes[..len]).unwrap() else {
            panic!("not a served request")
        };
        assert_eq!(
            (got.request(), got.target().len(), got.body_len()),
            (0, 1024, want)
        );
        let shown = format!("{got:?}");
        assert!(
            shown.contains("target_len: 1024") && !shown.contains("//"),
            "{shown}"
        );
    }
    let key = Record::EventServedRequest(EventServedRequest {
        now_ms: 7,
        server: 3,
        request: 1,
        method: "GET",
        target: "/?k=secretkey",
        flags: 0,
        body_len: 0,
    });
    let len = records_encode(&[key], &mut bytes).unwrap();
    let got = decode(&bytes[..len]).unwrap();
    assert!(!format!("{got:?}").contains("secretkey"));
}

#[test]
fn every_failed_reason_of_a_server_is_told_as_the_http_reasons() {
    use crate::native::net::{Reason, ServerState};
    let mut bytes = [0; 256];
    for (code, reason) in [
        (1, Reason::NoNetwork),
        (11, Reason::Offline),
        (12, Reason::Saving),
        (13, Reason::NetworkOff),
        (5, Reason::Refused),
    ] {
        let record = Record::EventServer(EventServer {
            now_ms: 7,
            server: 3,
            state: 1,
            reason: code,
            address: "",
        });
        let len = records_encode(&[record], &mut bytes).unwrap();
        let Event::Server(got) = decode(&bytes[..len]).unwrap() else {
            panic!("not a server event")
        };
        assert_eq!(got.state(), ServerState::Failed { reason });
    }
}
