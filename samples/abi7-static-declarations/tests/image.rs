use paperpaw_sample_static_declarations::{battery, submit, widgets};

#[path = "support/allocations.rs"]
mod allocations;
#[global_allocator]
static ALLOCATOR: allocations::Counting = allocations::Counting;

#[test]
fn image_submission_matches_goldens_without_allocating() {
    for (batches, golden) in [
        (
            battery::DECLARATIONS,
            include_bytes!("../../../plugins/battery/testdata/declarations.bin.v1").as_slice(),
        ),
        (
            widgets::DECLARATIONS,
            include_bytes!("../../../plugins/widgets/testdata/declarations.bin.v1").as_slice(),
        ),
    ] {
        let mut offset = 0;
        let mut index = 0;
        let (resident, peak) = allocations::measure(|| {
            submit(batches, |batch| {
                assert_eq!(batch.as_ptr(), batches[index].as_ptr());
                let length =
                    u32::from_le_bytes(golden[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                assert_eq!(batch, &golden[offset..offset + length]);
                offset += length;
                index += 1;
                Ok(())
            })
            .unwrap();
        });
        assert_eq!(offset, golden.len());
        assert_eq!((resident, peak), (0, 0));
        eprintln!(
            "{} batches: resident {resident} B, peak {peak} B (submission only)",
            batches.len()
        );
    }
    assert_eq!(widgets::DECLARATIONS.len(), 50);
}

#[test]
fn host_refusal_stops_submission() {
    let mut calls = 0;
    assert_eq!(
        submit(widgets::DECLARATIONS, |_| {
            calls += 1;
            Err(-4)
        }),
        Err(-4)
    );
    assert_eq!(calls, 1);
}
