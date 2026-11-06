#![no_std]

pub mod battery {
    include!(concat!(env!("OUT_DIR"), "/battery.rs"));
}

pub mod widgets {
    include!(concat!(env!("OUT_DIR"), "/widgets.rs"));
}

/// Register widgets directly from its image during the plugin make callback.
pub fn register_widgets(
    host: &paperpaw_sdk::native::Host<'_>,
) -> Result<(), paperpaw_sdk::native::Error> {
    for batch in widgets::DECLARATIONS {
        paperpaw_sdk::native::registrations::submit(host, batch)?;
    }
    Ok(())
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
