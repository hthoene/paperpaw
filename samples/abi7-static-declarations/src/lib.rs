#![no_std]

pub mod battery {
    include!(concat!(env!("OUT_DIR"), "/battery.rs"));
}

pub mod widgets {
    include!(concat!(env!("OUT_DIR"), "/widgets.rs"));
}

/// Forward the image slices themselves; never copy them through a builder.
/// Stop on the first host refusal, retaining the exact status.
pub fn submit(
    batches: &'static [&'static [u8]],
    mut sink: impl FnMut(&'static [u8]) -> Result<(), i32>,
) -> Result<(), i32> {
    for batch in batches {
        sink(batch)?;
    }
    Ok(())
}
