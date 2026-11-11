//! The order of statuses `classify` reports, against the decoded batch's.
use super::super::registrations::*;
use super::*;
use alloc::vec;
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

/// `classify` checks one record at a time, yet reports what checking the
/// decoded batch did: a decoding error anywhere in the batch before any
/// record's own status, and the first refused record's status otherwise.
#[test]
fn classify_reports_the_statuses_of_a_decoded_batch_in_the_same_order() {
    let action = |name, flags| {
        Record::ActionSpec(ActionSpec {
            name,
            title: "Action",
            token: 1,
            flags,
        })
    };
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let refused = action("refused", 1 << 31);
    let records = [
        action("first", 0),
        refused,
        action("third", 0),
        action("fourth", 0),
        action("fifth", 0),
    ];
    let len = records_encode(&records, &mut bytes).unwrap();
    bytes.truncate(len);
    let whole = |bytes: &[u8]| {
        host_records_decode(bytes).and_then(|batch| declarations::validate_batch("fixture", &batch))
    };
    // Record 2 refused, the batch decodes: record 2's status.
    assert_eq!(whole(&bytes), Err(ERR_UNSUPPORTED));
    assert_eq!(classify("fixture", &bytes).map(|_| ()), whole(&bytes));
    // Record 2 refused and record 5 cut short: the decoding status first.
    let cut = &bytes[..bytes.len() - 1];
    assert_eq!(whole(cut), Err(ERR_INVALID));
    assert_eq!(classify("fixture", cut).map(|_| ()), whole(cut));
    // Without the refused record, each record is classified in its place.
    let mut good = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(&[records[0], records[2]], &mut good).unwrap();
    good.truncate(len);
    let (kinds, counts) = classify("fixture", &good).unwrap();
    assert!(kinds[0].is_some() && kinds[1].is_some() && kinds[2].is_none());
    assert_eq!(counts.iter().sum::<usize>(), 2);
}
