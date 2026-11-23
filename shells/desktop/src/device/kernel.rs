//! The kernel of the emulated device: the standard app built as the device's
//! shell builds it (`shells/xteink/src/main.rs` `kernel` for the Xteink
//! profiles), counted against the profile's heap budget.

use paperpaw_core::{Kernel, KernelBuilder, KernelError};
use paperpaw_platform::Rotation;

use super::{AddPlugins, CODE_PLUGINS, Card, LinkedInputs, turned};
use crate::budget;
use crate::profiles::Profile;

/// The standard app for `profile` (with the plugins `add` adds), counted
/// against its heap budget.
pub(super) fn build_kernel(
    profile: &'static Profile,
    user_config: Option<&str>,
    storage: Card,
    rotation: Rotation,
    add: Option<AddPlugins>,
    linked: &LinkedInputs,
) -> Result<Kernel, KernelError> {
    #[cfg(not(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64")))]
    let _ = linked;
    budget::track(|| {
        // What this kernel's card plugins and linked plugins share.
        #[cfg(not(target_arch = "wasm32"))]
        let pool = crate::native::DevicePool::default();
        let fonts = if profile.font_packs {
            paperpaw_app::Fonts::Packs
        } else {
            paperpaw_app::Fonts::Outline
        };
        let builder = device_builder(profile, rotation, storage);
        // Every plugin, those the store offers as files too switched on,
        // as if the reader had installed them (the emulator loads no
        // plugin files): `--set plugins.<id>=off` shows a device without.
        let mut builder = paperpaw_app::install(builder, fonts);
        if let Some(light) = profile.light {
            builder = builder.items(crate::light::manifest(), crate::light::emulated(light));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            builder = crate::native::code_plugins(
                builder,
                CODE_PLUGINS.load(std::sync::atomic::Ordering::Relaxed),
                profile.device.memory,
                &pool,
            );
        }
        // Plugin installs from the card share the firmware's queue: the
        // emulated device has one, with no firmware to install
        // (`crate::linked::card_plugins`).
        #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
        if let Some(family) = crate::linked::card_plugins(profile) {
            builder =
                builder.firmware_host(Box::new(paperpaw_x4::host_jobs::NoFirmware::new(family)));
        }
        if let Some(add) = add {
            builder = add(builder);
        }
        if let Some(rows) = profile.frame_rows {
            builder = builder.frame_rows(rows);
        }
        builder = builder.row_align(profile.row_align);
        if let Some(name) = profile.catalog_device {
            builder = builder.catalog_device(name);
        }
        for (id, value) in profile.settings {
            builder = builder.setting_default_text(id, value);
        }
        if let Some(config) = user_config {
            builder = builder.user_config(config);
        }
        // What is wired is known before the kernel is built: a card plugin is
        // admitted in `build`, and is offered what the linked ones are.
        #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
        let prepared = crate::linked::prepare(profile, linked, &pool);
        #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
        let builder = builder.library_walk(linked.walk.clone());
        let kernel = builder.build()?;
        // The first-party plugins that are interface-7 plugins (the app built
        // without their interface-6 form), before the first start.
        #[cfg(all(feature = "native-linked", target_os = "linux", target_arch = "x86_64"))]
        let kernel = {
            let mut kernel = kernel;
            match prepared.and_then(|prepared| {
                crate::linked::register(&mut kernel, profile, linked.clone(), &pool, prepared)
            }) {
                Ok(_) => {}
                Err(why) => eprintln!("native plugins: none registered ({why:?})"),
            }
            kernel
        };
        Ok(kernel)
    })
}

/// The kernel's builder for the device of `profile` turned by `rotation`,
/// on `storage`: what the device is and has, before its plugins.
fn device_builder(profile: &'static Profile, rotation: Rotation, storage: Card) -> KernelBuilder {
    let mut device = turned(profile.device, rotation);
    device.input.keys = profile.buttons;
    let gray_levels = device.display.format.gray_levels();
    let grays = gray_levels.map_or(255, |levels| levels.min(255) as u8);
    // A start after a switch-off is one after a sleep, by the mark it
    // left on the card (as the firmware's `kernel`, `paperpaw_x4::slept`).
    let after_sleep = paperpaw_x4::slept::take(&storage);
    let mut builder = Kernel::builder(device)
        .after_sleep(after_sleep)
        .storage(crate::trace::Traced(storage))
        .clock(crate::host::micros)
        .random(crate::host::random)
        .device_log(crate::host::device_log)
        .memory(budget::memory_use)
        .glyph_cache(profile.glyph_cache)
        .busy_mark(profile.busy_mark)
        // Its grays; frames show at once here (see `execute`).
        .panel(paperpaw_core::sdk::realtime::PanelInfo::new(grays))
        .colors(profile.colors)
        .network(true)
        // Plugins' secure sockets (rustls; none in the browser), as many
        // at once as the device has.
        .tls_sessions(profile.tls_sessions())
        .wifi(profile.wifi)
        .bluetooth(profile.bluetooth)
        .speech(profile.speech)
        .sleep(profile.sleep)
        .power_off(profile.power_off)
        .button_row(profile.button_row)
        .rotation(true);
    if profile.refresh_asleep {
        builder = builder.capability(paperpaw_core::sdk::Capability::REFRESH_ASLEEP);
    }
    // As the Xteink firmware says: every device has the USB serial unit
    // the developer link talks over, the ESP32-S3 ones show the card to
    // a computer (here refused as with no cable, `execute`).
    if let Some(chip) = profile.chip {
        builder = builder.capability(paperpaw_core::sdk::Capability::USB_LINK);
        if chip.usb_drive() {
            builder = builder.capability(paperpaw_core::sdk::Capability::USB_DRIVE);
        }
    }
    // Scalable fonts only where the emulated device renders them.
    if !profile.font_packs {
        builder = builder.font_dir("fonts");
    }
    builder
}
