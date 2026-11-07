use super::*;
use crate::native_host::HostResources;
use core::pin::pin;
use paperpaw_native::host_glue::{ERR_FULL, ERR_UNSUPPORTED, ERR_UTF8};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Font, FontSpec, Id};

fn bytes(text: &[u8]) -> PpBytes {
    PpBytes {
        ptr: text.as_ptr(),
        len: text.len(),
    }
}

fn add_font(kernel: &mut crate::Kernel, id: &'static str, advance: f32) {
    let mut pack = paperpaw_text::PackBuilder::new();
    pack.add_strike(
        10.0,
        paperpaw_text::LineMetrics::new(8.0, 2.0, 12.0),
        alloc::vec![('a', paperpaw_text::Glyph::empty(advance))],
    );
    kernel
        .shared
        .fonts
        .add(FontSpec::new(
            Id::parse(id).unwrap(),
            Font::from_data(pack.build(), &[]).unwrap(),
        ))
        .unwrap();
}

fn kernel() -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    add_font(&mut kernel, "pack.face", 5.0);
    kernel
}

fn scoped(kernel: &crate::Kernel, call: impl FnOnce(&EnvironmentScope)) {
    let env = kernel.shared.env(Instant(0));
    let frame = crate::abi7::core_frame::CoreFrame {
        env: &env,
        meter: &kernel.shared.meter,
        font_removals: &kernel.shared.font_removals,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
    };
    let scope = pin!(EnvironmentScope::new());
    scope.as_ref().scope_core(&frame, || call(&scope)).unwrap();
}

fn get(scope: &EnvironmentScope, owner: &OwnerResources, key: &str) -> u32 {
    let mut handle = 0;
    // SAFETY: live readable input and exclusive, disjoint scalar output.
    assert_eq!(
        unsafe { acquire(scope, owner, bytes(key.as_bytes()), &mut handle) },
        OK
    );
    assert_ne!(handle, 0);
    handle
}

#[test]
fn named_metrics_measure_release_and_invalid_inputs_preserve_outputs() {
    let kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    scoped(&kernel, |scope| {
        let handle = get(scope, &owner, "pack.face");
        let mut width = -91.0;
        let mut values = [-91.0; 3];
        // SAFETY: initialized text and disjoint exclusive scalar outputs.
        unsafe {
            assert_eq!(
                measure(scope, &owner, handle, 10.0, bytes(b"aa"), &mut width),
                OK
            );
            assert_eq!(width, 10.0);
            let out = values.as_mut_ptr();
            assert_eq!(
                metrics(scope, &owner, handle, 10.0, out, out.add(1), out.add(2)),
                OK
            );
            assert_eq!(values, [8.0, 2.0, 12.0]);
            for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                width = -91.0;
                assert_eq!(
                    measure(scope, &owner, handle, size, bytes(b"a"), &mut width),
                    ERR_INVALID
                );
                assert_eq!(width, -91.0);
            }
            assert_eq!(
                measure(scope, &owner, handle, 10.0, bytes(&[0xff]), &mut width),
                ERR_UTF8
            );
            assert_eq!(
                metrics(scope, &owner, handle, 10.0, out, out, out.add(2)),
                ERR_INVALID
            );
            assert_eq!(values, [8.0, 2.0, 12.0]);
            assert_eq!(
                metrics(
                    scope,
                    &owner,
                    handle,
                    10.0,
                    core::ptr::null_mut(),
                    out.add(1),
                    out.add(2)
                ),
                ERR_INVALID
            );
            let unaligned = out.cast::<u8>().add(1).cast();
            assert_eq!(
                metrics(
                    scope,
                    &owner,
                    handle,
                    10.0,
                    unaligned,
                    out.add(1),
                    out.add(2)
                ),
                ERR_INVALID
            );
        }
        assert_eq!(release(&owner, handle), OK);
        // SAFETY: valid text and exclusive output, handle now deliberately stale.
        assert_eq!(
            unsafe { measure(scope, &owner, handle, 10.0, bytes(b"a"), &mut width) },
            ERR_STATE
        );
    });
}

#[test]
fn same_id_replacement_stales_named_handles_but_roles_follow_the_new_font() {
    let mut kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    let mut named = 0;
    let mut role = 0;
    scoped(&kernel, |scope| {
        named = get(scope, &owner, "pack.face");
        role = get(scope, &owner, "role:reading-bold-italic");
    });
    assert_eq!(kernel.remove_plugin_fonts(&Id::new_static("pack")), 1);
    add_font(&mut kernel, "pack.face", 7.0);
    scoped(&kernel, |scope| {
        let mut width = -91.0;
        // SAFETY: live text and exclusive scalar output.
        unsafe {
            assert_eq!(
                measure(scope, &owner, named, 10.0, bytes(b"a"), &mut width),
                ERR_STATE
            );
            assert_eq!(width, -91.0);
            assert_eq!(
                measure(scope, &owner, role, 10.0, bytes(b"a"), &mut width),
                OK
            );
            assert_eq!(width, 7.0);
            let fresh = get(scope, &owner, "pack.face");
            assert_eq!(
                measure(scope, &owner, fresh, 10.0, bytes(b"a"), &mut width),
                OK
            );
        }
    });
}

#[test]
fn icons_acquire_unbound_and_all_font_keys_share_the_eight_font_cap() {
    let kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    scoped(&kernel, |scope| {
        let icons = get(scope, &owner, "role:icons");
        let mut width = -91.0;
        // SAFETY: live text, exclusive scalar output, unbound role is intentional.
        unsafe {
            assert_eq!(
                measure(scope, &owner, icons, 10.0, bytes(b"a"), &mut width),
                ERR_NOT_FOUND
            );
            assert_eq!(width, -91.0);
            let mut out = 91;
            for key in ["role:mono", "role:ui-bold", "role:unknown"] {
                assert_eq!(
                    acquire(scope, &owner, bytes(key.as_bytes()), &mut out),
                    ERR_NOT_FOUND
                );
                assert_eq!(out, 91);
            }
            for _ in 1..8 {
                get(scope, &owner, "pack.face");
            }
            assert_eq!(
                acquire(scope, &owner, bytes(b"role:ui"), &mut out),
                ERR_FULL
            );
            assert_eq!(out, 91);
        }
    });
}

#[test]
fn env_only_named_access_fails_closed_while_roles_work_and_release_needs_no_frame() {
    let kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    let env = kernel.shared.env(Instant(0));
    let scope = pin!(EnvironmentScope::new());
    let mut role_handle = 0;
    scope
        .as_ref()
        .scope(&env, || {
            role_handle = get(&scope, &owner, "role:ui");
            let mut out = 91;
            // SAFETY: valid key and exclusive output, no trusted removal tracker.
            assert_eq!(
                unsafe { acquire(&scope, &owner, bytes(b"pack.face"), &mut out) },
                ERR_STATE
            );
            assert_eq!(out, 91);
        })
        .unwrap();
    assert_eq!(release(&owner, role_handle), OK);
    assert_eq!(release(&owner, role_handle), ERR_STATE);
}

#[test]
fn ink_checks_all_outputs_and_staleness_before_the_pending_probe() {
    let kernel = kernel();
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    scoped(&kernel, |scope| {
        let handle = get(scope, &owner, "pack.face");
        let mut values = [91; 4];
        let out = values.as_mut_ptr();
        // SAFETY: live text, disjoint writable scalar outputs.
        unsafe {
            assert_eq!(
                ink(
                    scope,
                    &owner,
                    handle,
                    10.0,
                    bytes(b"a"),
                    out,
                    out.add(1),
                    out.add(2),
                    out.add(3)
                ),
                ERR_UNSUPPORTED
            );
            assert_eq!(values, [91; 4]);
            assert_eq!(
                ink(
                    scope,
                    &owner,
                    handle,
                    10.0,
                    bytes(b"a"),
                    out,
                    out.add(1),
                    core::ptr::null_mut(),
                    out.add(3)
                ),
                ERR_INVALID
            );
            assert_eq!(values, [91; 4]);
            release(&owner, handle);
            assert_eq!(
                ink(
                    scope,
                    &owner,
                    handle,
                    10.0,
                    bytes(b"a"),
                    out,
                    out.add(1),
                    out.add(2),
                    out.add(3)
                ),
                ERR_STATE
            );
            assert_eq!(values, [91; 4]);
        }
    });
}
