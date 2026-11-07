//! UIX records at their caps (`docs/abi/ui.md` section 8.1): every case
//! encodes with each field at its largest and stays within both the size the
//! table states and its limit (a service input or reply 4,096 bytes, a
//! command batch 4,096 bytes, the event stack buffer 1,100 bytes).
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]

use paperpaw_core::native_host::commands::{WireCommand, commands_encode};
use paperpaw_core::native_host::registrations::{
    EventScreenOpened, Record, ServiceEnd, ServiceUiAsk, ServiceUiParam, ServiceUiQuestion,
    ServiceUiScreen, ServiceUiStatusAsk, ServiceUiStatusValue, ServiceUiTarget, ServiceUiText,
    records_encode,
};

const SERVICE_LIMIT: usize = 4_096;
const STAGE_LIMIT: usize = 3_840;
const EVENT_STACK: usize = 1_100;

fn text(len: usize) -> String {
    "a".repeat(len)
}

/// One record framed (tag, length and body; the batch count byte not
/// counted), as the table counts it.
fn framed(record: Record<'_>) -> usize {
    let mut out = vec![0; SERVICE_LIMIT];
    records_encode(&[record], &mut out).unwrap() - 1
}

/// `records` as one reply or input: a count byte (fewer than 128 rows) and
/// each record framed; at most `stated` bytes and `limit`.
fn fits(records: &[Record<'_>], stated: usize, limit: usize) -> usize {
    let len = 1 + records.iter().map(|r| framed(*r)).sum::<usize>();
    let single = records.len() == 1;
    let measured = if single { len - 1 } else { len };
    assert!(measured <= stated, "{measured} over the stated {stated}");
    assert!(
        stated < limit,
        "stated {stated} not within the limit {limit}"
    );
    len
}

fn end() -> Record<'static> {
    Record::ServiceEnd(ServiceEnd {
        flags: u32::MAX,
        total: u32::MAX,
        generation: u32::MAX,
        next: u32::MAX,
    })
}

#[test]
fn question_and_answer_records_fit_at_their_caps() {
    let (slot, title, body, choices) = (text(128), text(256), text(1_024), text(2_048));
    let ask = Record::ServiceUiAsk(ServiceUiAsk {
        kind: u32::MAX,
        flags: u32::MAX,
        slot: &slot,
        title: &title,
        text: &body,
        choices: &choices,
    });
    fits(&[ask], 3_478, SERVICE_LIMIT);
    let question = Record::ServiceUiQuestion(ServiceUiQuestion {
        kind: u32::MAX,
        flags: u32::MAX,
        max_bytes: u32::MAX,
        title: &title,
        text: &body,
        choices: &choices,
    });
    fits(&[question], 3_351, SERVICE_LIMIT);
    let data = text(3_072);
    let part = Record::ServiceUiText(ServiceUiText {
        text: u32::MAX,
        offset: u32::MAX,
        total: u32::MAX,
        flags: u32::MAX,
        data: &data,
    });
    fits(&[part], 3_096, SERVICE_LIMIT);
}

#[test]
fn status_records_fit_at_their_caps() {
    let items = text(1_032);
    let context = vec![0xab; 1_024];
    let ask = Record::ServiceUiStatusAsk(ServiceUiStatusAsk {
        items: &items,
        context: &context,
    });
    fits(&[ask], 2_066, SERVICE_LIMIT);
    let (item, value) = (text(128), text(128));
    let row = Record::ServiceUiStatusValue(ServiceUiStatusValue {
        item: &item,
        flags: u32::MAX,
        handle: u32::MAX,
        text: &value,
    });
    let mut page = vec![row; 8];
    page.push(end());
    fits(&page, 2_214, SERVICE_LIMIT);
}

#[test]
fn stack_and_target_pages_fit_at_their_caps() {
    let (screen, slot, owner) = (text(128), text(128), text(64));
    let row = Record::ServiceUiScreen(ServiceUiScreen {
        screen: &screen,
        instance: u32::MAX,
        slot: &slot,
        owner: &owner,
        flags: u32::MAX,
    });
    let mut page = vec![row; 8];
    page.push(end());
    fits(&page, 2_742, SERVICE_LIMIT);
    let target = Record::ServiceUiTarget(ServiceUiTarget {
        target: u32::MAX,
        x: i32::MIN,
        y: i32::MIN,
        width: u32::MAX,
        height: u32::MAX,
    });
    let mut page = vec![target; 8];
    page.push(end());
    fits(&page, 230, SERVICE_LIMIT);
    // The owner's full answer of the `core.widget` stage: 32 targets.
    fits(&vec![target; 32], 800, STAGE_LIMIT);
}

#[test]
fn hand_over_params_fit_at_their_caps() {
    // 32 params whose keys and values hold 1,024 bytes in all (section 3.1).
    let keys: Vec<String> = (0..32).map(|i| format!("{i:02}")).collect();
    let values: Vec<String> = (0..32).map(|_| text(30)).collect();
    let total: usize = keys.iter().chain(&values).map(String::len).sum();
    assert_eq!(total, 1_024);
    let mut params: Vec<Record<'_>> = keys
        .iter()
        .zip(&values)
        .map(|(key, value)| Record::ServiceUiParam(ServiceUiParam { key, value }))
        .collect();
    fits(&params, 1_313, SERVICE_LIMIT);
    params.push(end());
    fits(&params, 1_313 + 30, SERVICE_LIMIT);
}

#[test]
fn open_command_and_screen_opened_event_fit_at_their_caps() {
    let (target, slot) = (text(128), text(128));
    let open = WireCommand::Open {
        target: &target,
        mode: u32::MAX,
        slot: &slot,
        handover: u32::MAX,
    };
    let mut out = vec![0; SERVICE_LIMIT];
    let len = commands_encode(&[open], &mut out).unwrap();
    assert!(len <= 274, "command.open {len}");
    let opened = Record::EventScreenOpened(EventScreenOpened {
        now_ms: u64::MAX,
        screen: &target,
        instance: u32::MAX,
        slot: &slot,
        flags: u32::MAX,
        handover: u32::MAX,
        params: u32::MAX,
        text_bytes: u32::MAX,
    });
    fits(&[opened], 293, EVENT_STACK);
}
