//! A native plugin built apart from the kernel that loads it
//! (`../native_plugin.rs`): the SDK's sample plugin, and a service whose
//! type is the SDK's, which the kernel's build finds by its name (and,
//! published the old way, not by its `TypeId`).

#![no_std]

extern crate alloc;

mod probe;

use paperpaw_sample_hello::Hello;
use paperpaw_sdk::{Id, Manifest, Plugin, RegisterError, Registrar, Version};

/// The sample, with one more thing: its version as a service.
struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Hello.manifest()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        Hello.register(r)?;
        r.provide(
            Id::new_static("hello.version"),
            HelloVersion(Version::new(1, 2, 3)),
            Version::new(1, 0, 0),
        )?;
        // The text calls of `probe.rs`: through the shell's table in
        // table mode, the plugin's own copies otherwise.
        r.provide(
            Id::new_static("hello.text-probe"),
            probe::probe(),
            Version::new(1, 0, 0),
        )
    }
}

paperpaw_sdk::plugin!(Probe);

/// Its version as a service, the type the test that loads it names
/// too, under the same key.
#[repr(C)]
pub struct HelloVersion(pub Version);
paperpaw_sdk::service_type!(HelloVersion, "hello.version/1");
