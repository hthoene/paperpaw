use super::*;

fn batch(start: u32, kinds: &[u32]) -> BatchStart {
    let mut batch = BatchStart::new(start);
    for kind in kinds {
        batch.add_kind(*kind);
    }
    batch
}

#[test]
fn batches_near_each_other_share_one_start_and_one_mask() {
    let batches = [
        batch(1000, &[3]),
        batch(1040, &[5]),
        batch(66_535, &[3, 40]),
    ];
    let starts = Starts::try_new(&batches).unwrap();
    assert!(starts.is_packed());
    assert_eq!(starts.len(), 3);
    let at: Vec<_> = (0..4).map(|batch| starts.start(batch)).collect();
    assert_eq!(at, [Some(1000), Some(1040), Some(66_535), None]);
    let owner = 1 << 3 | 1 << 5 | 1 << 40;
    assert_eq!(
        (starts.kinds(0), starts.kinds(2), starts.kinds(3)),
        (owner, owner, 0)
    );
    // 14 bytes and 2 a batch, where wide starts take 12 a batch (and a word).
    assert_eq!(starts.bytes(), 14 + 2 * 3);
}

#[test]
fn one_batch_or_batches_far_apart_keep_a_start_and_a_mask_each() {
    let one = Starts::try_new(&[batch(7, &[63])]).unwrap();
    assert!(!one.is_packed());
    assert_eq!(
        (one.len(), one.start(0), one.kinds(0)),
        (1, Some(7), 1 << 63)
    );
    assert_eq!(one.bytes(), 2 + 12);
    let far = [batch(u32::MAX, &[1]), batch(0, &[2]), batch(65_536, &[])];
    let starts = Starts::try_new(&far).unwrap();
    assert!(
        !starts.is_packed(),
        "65,536 apart: no 16-bit offset holds it"
    );
    let at: Vec<_> = (0..4).map(|batch| starts.start(batch)).collect();
    assert_eq!(at, [Some(u32::MAX), Some(0), Some(65_536), None]);
    let kinds: Vec<_> = (0..4).map(|batch| starts.kinds(batch)).collect();
    assert_eq!(kinds, [1 << 1, 1 << 2, 0, 0]);
}

#[test]
fn no_batches_make_no_starts_and_a_kind_past_the_mask_is_none() {
    assert!(Starts::try_new(&[]).is_none());
    let mut start = BatchStart::new(0);
    start.add_kind(64);
    assert_eq!(Starts::try_new(&[start]).unwrap().kinds(0), 0);
}
