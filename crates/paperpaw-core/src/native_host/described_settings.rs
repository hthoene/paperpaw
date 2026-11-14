//! A native plugin's settings described from its own records (#248): the
//! kernel keeps where each `setting.spec` record lies in the plugin's image
//! and makes a spec from it when one is asked for
//! (`paperpaw_sdk::Described`), so no spec, text or id of them stays on the
//! heap. The image is read through the plugin's lease each time, with every
//! check a first read gets: nothing of it is borrowed or trusted to stay.

use alloc::vec::Vec;

use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_sdk::{DescribeError, Described, DescribedSettings, Id, SettingSpec};

use super::native_instances::NativeLease;
use super::registrations::{self as wire, Record};
use super::{declarations, native_values};

/// The settings of the plugin `owner` whose records lie at `places` of its
/// image (each the start of a record's frame), in that order.
struct ImageSettings {
    lease: NativeLease,
    owner: Id,
    places: Vec<u32>,
}

impl ImageSettings {
    /// Gives `visit` the record of setting `at`, read and checked anew.
    fn read(
        &self,
        at: usize,
        visit: &mut dyn FnMut(wire::SettingSpec<'_>),
    ) -> Result<(), DescribeError> {
        let place = *self.places.get(at).ok_or(DescribeError::Gone)?;
        let mut found = false;
        self.lease.with_image(&mut |image| {
            let bytes = usize::try_from(place)
                .ok()
                .and_then(|place| image?.get(place..));
            let Some(Ok((record, _))) = bytes.map(wire::host_decode_prefix) else {
                return;
            };
            if declarations::validate_record(self.owner.as_str(), &record).is_err() {
                return;
            }
            if let Record::SettingSpec(spec) = record {
                found = true;
                visit(spec);
            }
        });
        // No image (a stopped plugin) or no such record there any more.
        found.then_some(()).ok_or(DescribeError::Gone)
    }
}

impl Described for ImageSettings {
    fn count(&self) -> usize {
        self.places.len()
    }

    fn name(&self, at: usize, visit: &mut dyn FnMut(&str)) -> Result<(), DescribeError> {
        self.read(at, &mut |spec| visit(spec.name))
    }

    fn make(&self, at: usize) -> Result<SettingSpec, DescribeError> {
        let mut made = Err(DescribeError::Gone);
        self.read(at, &mut |spec| {
            // Every id and text a copy of its own: none is the image's.
            let mut ids = native_values::checked_id;
            made = native_values::setting(self.owner.as_str(), spec, &mut ids).map_err(|status| {
                match status {
                    ERR_NO_MEMORY => DescribeError::NoMemory,
                    _ => DescribeError::Gone,
                }
            });
        })?;
        made
    }
}

/// Where the records `placed` start, in the order they were declared in, as
/// a table's settings are: the index gave them in the order of their names'
/// hashes.
pub(super) fn declared_order(
    mut placed: Vec<super::raw_declarations::Place>,
) -> Result<Vec<u32>, i32> {
    paperpaw_sort::sort_unstable(&mut placed);
    let mut places = Vec::new();
    places
        .try_reserve_exact(placed.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    places.extend(placed.iter().map(|place| place.at));
    Ok(places)
}

/// The settings of `owner` at `places` of the image `lease` gives, ready to
/// be defined: each record is read once here, its default and flags kept.
///
/// # Errors
/// `ERR_NO_MEMORY` where the heap has no room; `ERR_INVALID` where a record
/// cannot be read or is no setting that can be kept this way (the caller
/// mapped each of them before, so this is a plugin that changed its data).
pub(super) fn described(
    owner: &Id,
    lease: &NativeLease,
    places: Vec<u32>,
) -> Result<DescribedSettings, i32> {
    use paperpaw_native::host_glue::ERR_INVALID;
    let source = ImageSettings {
        lease: lease.try_clone()?,
        owner: owner.try_clone().ok_or(ERR_NO_MEMORY)?,
        places,
    };
    let source: alloc::boxed::Box<dyn Described> =
        super::shared::try_box(source).ok_or(ERR_NO_MEMORY)?;
    let plugin = owner.try_clone().ok_or(ERR_NO_MEMORY)?;
    DescribedSettings::new(plugin, source).map_err(|error| match error {
        DescribeError::NoMemory => ERR_NO_MEMORY,
        _ => ERR_INVALID,
    })
}

#[cfg(all(test, target_arch = "x86_64", target_os = "linux"))]
#[path = "described_settings_tests.rs"]
pub(super) mod tests;

#[cfg(all(test, target_arch = "x86_64", target_os = "linux"))]
#[path = "widget_fonts_tests.rs"]
mod widget_fonts_tests;
