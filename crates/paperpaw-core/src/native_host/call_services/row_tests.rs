//! Generated service row budgets apply before dispatch and publication.
use super::*;
use crate::native_host::host_services::Caller;
use alloc::vec::Vec;
use registrations::{Record, ServiceEnd, ServiceUiParam, ServiceUiTarget};

fn params(count: usize) -> Vec<u8> {
    let rows =
        std::vec![Record::ServiceUiParam(ServiceUiParam { key: "name", value: "value" }); count];
    let mut out = std::vec![0; 4096];
    let len = registrations::records_encode_counted(&rows, 64, 4096, &mut out).unwrap();
    out.truncate(len);
    out
}

#[test]
fn generated_input_row_limit_accepts_exact_and_refuses_one_more() {
    let schema = services_wire::service_schema("core.ui", 1).unwrap();
    let op = schema.op(16).unwrap();
    let key = ServiceKey {
        schema,
        minor: 0,
        features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
    };
    let limit = usize::from(op.input_rows);
    assert_eq!(limit, 32);
    assert!(validate_op(key, op.id, &params(limit), op.max_reply).is_ok());
    assert_eq!(
        validate_op(key, op.id, &params(limit + 1), op.max_reply).unwrap_err(),
        ERR_NO_SPACE
    );
    for malformed in [&[0x80, 0][..], &[0xff, 0xff, 0xff, 0xff, 0x10], &[1]] {
        assert_eq!(
            validate_op(key, op.id, malformed, op.max_reply).unwrap_err(),
            ERR_INVALID
        );
    }
}

std::thread_local! {
    static BACKEND_CALLS: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}
fn counted_echo(
    frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    BACKEND_CALLS.with(|calls| calls.set(calls.get() + 1));
    echo(frame, who, op, input, out)
}

#[test]
fn input_over_limit_never_calls_backend_or_changes_output() {
    BACKEND_CALLS.with(|calls| calls.set(0));
    let pool = HostResources::with_test_service(counted_echo).unwrap();
    let owner = pool.try_owner().unwrap();
    let view = owner.try_view().unwrap();
    let scope = pin!(EnvironmentScope::new());
    // SAFETY: the view outlives the scope's callbacks.
    unsafe { scope.expire_view(&view) };
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let input = batch("one");
    let mut two = input.clone();
    two[0] = 2;
    two.extend_from_slice(&input[1..]);
    scope
        .as_ref()
        .scope(&env, || {
            let mut handle = 0;
            let mut out = [0xa5; 512];
            // SAFETY: checked fixture inputs and exclusive output are live and disjoint.
            unsafe {
                assert_eq!(
                    open(
                        &scope,
                        &owner,
                        &declared(),
                        pair(b"core.test-echo"),
                        1,
                        0,
                        &mut handle
                    ),
                    OK
                );
                assert_eq!(
                    call(
                        &scope,
                        &owner,
                        "files",
                        handle,
                        1,
                        pair(&two),
                        output(&mut out)
                    ),
                    ERR_NO_SPACE
                );
                assert_eq!(out, [0xa5; 512]);
                assert_eq!(BACKEND_CALLS.with(core::cell::Cell::get), 0);
                assert_eq!(
                    call(
                        &scope,
                        &owner,
                        "files",
                        handle,
                        1,
                        pair(&input),
                        output(&mut out)
                    ),
                    i32::try_from(input.len() - 1).unwrap()
                );
                assert_eq!(BACKEND_CALLS.with(core::cell::Cell::get), 1);
            }
        })
        .unwrap();
}

fn targets(count: usize, end: ServiceEnd) -> Vec<u8> {
    let row = Record::ServiceUiTarget(ServiceUiTarget {
        target: 1,
        x: 0,
        y: 0,
        width: 1,
        height: 1,
    });
    let mut out = std::vec![0; 4096];
    let mut at = 0;
    for _ in 0..count {
        at += registrations::record_encode_one(&row, &mut out[at..]).unwrap();
    }
    at += registrations::record_encode_one(&Record::ServiceEnd(end), &mut out[at..]).unwrap();
    out.truncate(at);
    out
}
fn end(rows: usize) -> ServiceEnd {
    ServiceEnd {
        total: u32::try_from(rows).unwrap(),
        flags: 0,
        generation: 0,
        next: 0,
        cursor: None,
    }
}
fn targets_op() -> &'static ServiceOp {
    services_wire::service_schema("core.ui", 1)
        .unwrap()
        .op(7)
        .unwrap()
}

#[test]
fn unpaged_reply_row_limit_accepts_exact_and_refuses_one_more() {
    let op = targets_op();
    assert!(!op.paged);
    let limit = usize::from(op.reply_rows);
    assert_eq!(validate_reply(&targets(limit, end(limit)), op), Ok(()));
    assert_eq!(
        validate_reply(&targets(limit + 1, end(limit + 1)), op),
        Err(ERR_INVALID)
    );
    assert_eq!(validate_reply(&targets(0, end(0)), op), Ok(()));
}

/// One frame by hand: tag and length as LEB128, then `body`.
fn frame(tag: u32, body: &[u8]) -> Vec<u8> {
    fn leb(mut value: u32, out: &mut Vec<u8>) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }
    let mut out = Vec::new();
    leb(tag, &mut out);
    leb(u32::try_from(body.len()).unwrap(), &mut out);
    out.extend_from_slice(body);
    out
}

#[test]
fn a_malformed_non_end_row_is_invalid_like_any_other() {
    let op = targets_op();
    // A listed row whose framing is sound but whose body ends inside its fields.
    let mut reply = frame(registrations::SERVICE_UI_TARGET_TAG, &[0]);
    reply.extend_from_slice(&targets(0, end(1)));
    assert_eq!(validate_reply(&reply, op), Err(ERR_INVALID));
    // The same reply with the row whole is accepted.
    assert_eq!(validate_reply(&targets(1, end(1)), op), Ok(()));
}

#[test]
fn a_row_of_an_unlisted_tag_is_invalid_before_its_body_is_read() {
    let op = targets_op();
    // `echo`: a known record the op does not reply with; and a tag no record has.
    for tag in [registrations::SERVICE_ECHO_TAG, 0x3fff_ff00] {
        let mut reply = frame(tag, &[0]);
        reply.extend_from_slice(&targets(0, end(1)));
        assert_eq!(validate_reply(&reply, op), Err(ERR_INVALID));
    }
}

fn lookup_op() -> &'static ServiceOp {
    services_wire::service_schema("core.lookup", 1)
        .unwrap()
        .op(1)
        .unwrap()
}

fn lookup_entry() -> Vec<u8> {
    let row = Record::ServiceLookupEntry(registrations::ServiceLookupEntry {
        provider: "dict",
        title: "word",
        text: "meaning",
        flags: 0,
    });
    let mut out = std::vec![0; 256];
    let len = registrations::record_encode_one(&row, &mut out).unwrap();
    out.truncate(len);
    out
}

#[test]
fn release_framing_check_reads_framing_and_the_end_row_only() {
    let op = targets_op();
    // A listed row whose body ends early: only the full check refuses it.
    let mut bad_row = frame(registrations::SERVICE_UI_TARGET_TAG, &[0]);
    bad_row.extend_from_slice(&targets(0, end(1)));
    assert_eq!(validate_reply_with(&bad_row, op, Rows::Framing), Ok(()));
    assert_eq!(
        validate_reply_with(&bad_row, op, Rows::All),
        Err(ERR_INVALID)
    );
    // Framing, the row list, the row cap and the end row are checked as before.
    let whole = targets(1, end(1));
    assert_eq!(validate_reply_with(&whole, op, Rows::Framing), Ok(()));
    for len in 0..whole.len() {
        assert_eq!(
            validate_reply_with(&whole[..len], op, Rows::Framing),
            Err(ERR_INVALID)
        );
    }
    for tail in [
        end(0),
        ServiceEnd { next: 1, ..end(1) },
        ServiceEnd { flags: 1, ..end(1) },
    ] {
        assert_eq!(
            validate_reply_with(&targets(1, tail), op, Rows::Framing),
            Err(ERR_INVALID)
        );
    }
    let limit = usize::from(op.reply_rows);
    assert_eq!(
        validate_reply_with(&targets(limit + 1, end(limit + 1)), op, Rows::Framing),
        Err(ERR_INVALID)
    );
}

#[test]
fn release_lookup_check_reads_the_providers_entry_row_by_type() {
    let op = lookup_op();
    let entry = lookup_entry();
    for check in [Rows::Lookup, Rows::Framing, Rows::All] {
        assert_eq!(validate_reply_with(&entry, op, check), Ok(()));
    }
    // A provider's entry whose body ends early: refused by the lookup check
    // and the full one; the framing check does not read it.
    let bad = frame(registrations::SERVICE_LOOKUP_ENTRY_TAG, &[0]);
    assert_eq!(
        validate_reply_with(&bad, op, Rows::Lookup),
        Err(ERR_INVALID)
    );
    assert_eq!(validate_reply_with(&bad, op, Rows::All), Err(ERR_INVALID));
    assert_eq!(validate_reply_with(&bad, op, Rows::Framing), Ok(()));
    // A tag the lookup check does not read is refused before any body.
    let other = frame(registrations::SERVICE_ECHO_TAG, &[0]);
    assert_eq!(
        validate_reply_with(&other, op, Rows::Lookup),
        Err(ERR_INVALID)
    );
}

#[test]
fn list_reply_requires_one_terminal_end_and_canonical_frames() {
    let op = targets_op();
    let complete = targets(1, end(1));
    for len in 0..complete.len() {
        assert_eq!(validate_reply(&complete[..len], op), Err(ERR_INVALID));
    }
    let mut extra = complete.clone();
    extra.extend_from_slice(&targets(0, end(0)));
    assert_eq!(validate_reply(&extra, op), Err(ERR_INVALID));
    for tail in [
        end(0),
        ServiceEnd { next: 1, ..end(1) },
        ServiceEnd {
            generation: 1,
            ..end(1)
        },
        ServiceEnd { flags: 1, ..end(1) },
    ] {
        assert_eq!(validate_reply(&targets(1, tail), op), Err(ERR_INVALID));
    }
}

#[test]
fn paged_reply_uses_generated_row_cap_and_allows_paging_metadata() {
    let op = ServiceOp {
        paged: true,
        ..*targets_op()
    };
    let limit = usize::from(op.reply_rows);
    let tail = ServiceEnd {
        total: 100,
        generation: 1,
        next: 32,
        flags: 1,
        cursor: None,
    };
    assert_eq!(validate_reply(&targets(limit, tail), &op), Ok(()));
    assert_eq!(
        validate_reply(&targets(limit + 1, tail), &op),
        Err(ERR_INVALID)
    );
}

#[test]
fn nonlist_replies_are_bare_and_repeated_or_counted_replies_are_invalid() {
    let op = services_wire::service_schema("core.test-echo", 1)
        .unwrap()
        .op(1)
        .unwrap();
    let input = batch("echo");
    assert_eq!(validate_reply(&input[1..], op), Ok(()));
    assert_eq!(validate_reply(&input, op), Err(ERR_INVALID));
    // Required-record presence is checked by the family backend, not this table.
    assert_eq!(validate_reply(&[], op), Ok(()));
    let mut repeated = input[1..].to_vec();
    repeated.extend_from_slice(&input[1..]);
    assert_eq!(validate_reply(&repeated, op), Err(ERR_INVALID));
    assert_eq!(validate_reply(&targets(0, end(0)), op), Err(ERR_INVALID));
}

#[test]
fn usage_producer_and_echo_backend_share_the_dispatch_reply_validator() {
    use crate::abi7::core_frame::CoreFrame;
    use crate::native_host::data_services::{self, Query};
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let frame = CoreFrame {
        callback: None,
        instance: None,
        env: &env,
        meter: &kernel.shared.meter,
        depth: None,
        directory: None,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
    };
    let echo_op = services_wire::service_schema("core.test-echo", 1)
        .unwrap()
        .op(1)
        .unwrap();
    let mut out = [0; 512];
    let input = batch("shared validator");
    let hosts = crate::native_host::host_services::HostServices::new();
    let who = Caller {
        owner: 1,
        plugin: "files",
        features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
        hosts: &hosts,
    };
    let len = echo(&frame, &who, echo_op, &input, &mut out).unwrap();
    assert_eq!(validate_reply(&out[..len], echo_op), Ok(()));
    let usage_op = ServiceOp {
        reply: &[registrations::SERVICE_USAGE_TAG],
        ..*echo_op
    };
    let len = data_services::encode(&frame, Query::Usage, &mut out).unwrap();
    assert_eq!(validate_reply(&out[..len], &usage_op), Ok(()));
    let rows_op = ServiceOp {
        reply: &[
            registrations::SERVICE_PLUGIN_USAGE_TAG,
            registrations::SERVICE_END_TAG,
        ],
        paged: true,
        reply_rows: 32,
        ..*echo_op
    };
    let len = data_services::encode(&frame, Query::PluginUsage(0), &mut out).unwrap();
    assert_eq!(validate_reply(&out[..len], &rows_op), Ok(()));
}

#[test]
fn nonlist_generic_checks_order_and_duplicates_without_guessing_required_rows() {
    let echo_op = services_wire::service_schema("core.test-echo", 1)
        .unwrap()
        .op(1)
        .unwrap();
    let op = ServiceOp {
        reply: &[
            registrations::SERVICE_ECHO_TAG,
            registrations::SERVICE_UI_TARGET_TAG,
        ],
        ..*echo_op
    };
    let mut out = [0; 128];
    let target = Record::ServiceUiTarget(ServiceUiTarget {
        target: 1,
        x: 0,
        y: 0,
        width: 1,
        height: 1,
    });
    let len = registrations::record_encode_one(&target, &mut out).unwrap();
    assert_eq!(validate_reply(&out[..len], &op), Ok(()));
    let input = batch("out of order");
    let mut wrong = out[..len].to_vec();
    wrong.extend_from_slice(&input[1..]);
    assert_eq!(validate_reply(&wrong, &op), Err(ERR_INVALID));
    let empty = ServiceOp {
        reply: &[],
        ..*echo_op
    };
    assert_eq!(validate_reply(&[], &empty), Ok(()));
    assert_eq!(validate_reply(&input[1..], &empty), Err(ERR_INVALID));
}

#[test]
fn list_reply_rejects_rows_or_trailing_bytes_after_end() {
    let op = targets_op();
    let mut trailing = targets(0, end(0));
    trailing.push(0);
    assert_eq!(validate_reply(&trailing, op), Err(ERR_INVALID));
    let mut after_end = targets(0, end(0));
    let mut row = [0; 128];
    let len = registrations::record_encode_one(
        &Record::ServiceUiTarget(ServiceUiTarget {
            target: 1,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }),
        &mut row,
    )
    .unwrap();
    after_end.extend_from_slice(&row[..len]);
    assert_eq!(validate_reply(&after_end, op), Err(ERR_INVALID));
}

fn schema_limit(schema: &'static str, id: u32, expected: usize, mut records: Vec<Record<'_>>) {
    let op = services_wire::service_schema(schema, 1)
        .unwrap()
        .op(id)
        .unwrap();
    assert_eq!(usize::from(op.input_rows), expected);
    assert_eq!(records.len(), expected);
    let mut out = std::vec![0; op.max_input];
    for extra in [false, true] {
        if extra {
            records.push(*records.last().unwrap());
        }
        let len =
            registrations::records_encode_counted(&records, expected + 1, op.max_input, &mut out)
                .unwrap();
        // LIB remains feature-gated. Exercise the generic input validator with
        // the real generated schema, without pretending to grant that feature.
        assert_eq!(
            validate_batch(&out[..len], op.input, usize::from(op.input_rows)),
            if extra { Err(ERR_NO_SPACE) } else { Ok(()) }
        );
    }
}

#[test]
fn document_open_accepts_13_records_and_refuses_14() {
    let mut records = std::vec![Record::ServiceDocOpen(registrations::ServiceDocOpen {
        root: 1,
        path: "book.epub",
        format: "epub",
        flags: 0,
        image_side: 0,
        cover_side: 0,
        css_root: 0,
        css_path: "",
    })];
    for _ in 0..12 {
        records.push(Record::ServiceDocOption(registrations::ServiceDocOption {
            key: "option",
            value: "value",
        }));
    }
    schema_limit("core.document", 1, 13, records);
}

fn library_ref() -> Record<'static> {
    Record::ServiceLibRef(registrations::ServiceLibRef {
        path: "book.epub",
        generation: 0,
        key: None,
    })
}

#[test]
fn library_generated_2_and_3_record_limits_refuse_one_more() {
    schema_limit(
        "core.library",
        4,
        2,
        std::vec![
            library_ref(),
            Record::ServiceLibTextAt(registrations::ServiceLibTextAt {
                kind: 0,
                index: 0,
                offset: 0
            })
        ],
    );
    schema_limit(
        "core.library",
        14,
        3,
        std::vec![
            library_ref(),
            Record::ServiceLibPosition(registrations::ServiceLibPosition {
                section: 0,
                offset: 0,
                insert: 0,
                progress_ppm: 0,
                conversion: 0,
                updated: 0,
                xpointer: "",
                flags: 0,
            }),
            Record::ServiceLibPositionSet(registrations::ServiceLibPositionSet { flags: 0 })
        ],
    );
}

#[test]
fn library_edit_add_accepts_4_records_and_refuses_5() {
    let edit = Record::ServiceLibEdit(registrations::ServiceLibEdit {
        action: 0,
        id: 0,
        kind: 0,
        start_section: 0,
        start_offset: 0,
        end_section: 0,
        end_offset: 0,
        flags: 0,
        percent: 0,
        conversion: 0,
        created: 0,
        updated: 0,
        client_key: "edit",
        text: "",
        note: "",
        text_bytes: 0,
        note_bytes: 0,
    });
    schema_limit(
        "core.library",
        18,
        4,
        std::vec![
            Record::ServiceLibTransactionRef(registrations::ServiceLibTransactionRef {
                transaction: 1
            }),
            edit,
            edit,
            edit
        ],
    );
}

mod gate {
    use super::*;
    use crate::native_host::admitted_features::AdmittedFeatures;
    use paperpaw_native::host_glue::ERR_DENIED;
    use paperpaw_native::metadata::Feature;

    const WIRED: &[Feature<'static>] = &[Feature {
        key: "uses.library",
        revision: 1,
    }];

    fn state_key(features: AdmittedFeatures) -> ServiceKey {
        ServiceKey {
            schema: services_wire::service_schema("core.library", 1).unwrap(),
            minor: 0,
            features,
        }
    }

    fn state_input() -> Vec<u8> {
        let rows = std::vec![Record::ServiceLibRef(registrations::ServiceLibRef {
            path: "book.epub",
            generation: 0,
            key: None,
        })];
        let mut out = std::vec![0; 256];
        let len = registrations::records_encode_counted(&rows, 4, 256, &mut out).unwrap();
        out.truncate(len);
        out
    }

    #[test]
    fn gated_op_is_unsupported_without_host_denied_without_grant_and_runs_with_it() {
        let schema = services_wire::service_schema("core.library", 1).unwrap();
        let op = schema.op(3).unwrap();
        assert_eq!(op.feature, Some("uses.library"));
        let input = state_input();
        let status =
            |features| validate_op(state_key(features), op.id, &input, op.max_reply).map(|_| ());
        assert_eq!(status(AdmittedFeatures::NONE), Err(ERR_UNSUPPORTED));
        assert_eq!(status(AdmittedFeatures::test(0, WIRED)), Err(ERR_DENIED));
        assert_eq!(status(AdmittedFeatures::test(1, WIRED)), Ok(()));
        // Features are asked before sizes: a denied caller learns nothing about them.
        assert_eq!(
            validate_op(
                state_key(AdmittedFeatures::test(0, WIRED)),
                op.id,
                &input,
                0
            )
            .map(|_| ()),
            Err(ERR_DENIED)
        );
    }
}
