#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use crate::{
    abi7::core_frame::CoreFrame,
    native_host::{
        HostResources,
        resources::{FontKey, FontRole},
    },
};
use core::{pin::pin, ptr};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_platform::Instant;

struct Access<'a> {
    scope: &'a EnvironmentScope,
    owner: &'a OwnerResources,
    declared: &'a DeclaredServices,
}
impl Access<'_> {
    fn query(&self, text: &str, major: u16, minor: u16, out: &mut u32) -> i32 {
        let key = PpBytes {
            ptr: text.as_ptr(),
            len: text.len(),
        };
        // SAFETY: all fixture storage is live and disjoint.
        unsafe {
            query(
                self.scope,
                self.owner,
                self.declared,
                key,
                major,
                minor,
                out,
            )
        }
    }
    fn read(&self, handle: u32, offset: u64, bytes: &mut [u8]) -> i32 {
        let out = PpBuffer {
            ptr: bytes.as_mut_ptr(),
            cap: bytes.len(),
        };
        // SAFETY: output is live, exclusive and disjoint from host state.
        unsafe { read(self.scope, self.owner, handle, offset, out) }
    }
    fn release(&self, handle: u32) -> i32 {
        release(self.scope, self.owner, handle)
    }
}

fn check_live(access: &Access<'_>, font: u32) -> u32 {
    let mut handle = 0x1234;
    for (text, major, minor, status) in [
        ("core.unknown", 1, 0, ERR_NOT_FOUND),
        ("core.usage/x", 1, 0, ERR_INVALID),
        ("core.usage", 2, 0, ERR_UNSUPPORTED),
        ("core.usage", 1, 1, ERR_UNSUPPORTED),
    ] {
        assert_eq!(access.query(text, major, minor, &mut handle), status);
        assert_eq!(handle, 0x1234);
    }
    let empty = DeclaredServices::none();
    let undeclared = Access {
        declared: &empty,
        ..*access
    };
    assert_eq!(
        undeclared.query("core.usage", 1, 0, &mut handle),
        ERR_UNSUPPORTED
    );
    let text = "core.usage";
    // SAFETY: readable key; null scalar output must be rejected before use.
    unsafe {
        assert_eq!(
            query(
                access.scope,
                access.owner,
                access.declared,
                PpBytes {
                    ptr: text.as_ptr(),
                    len: text.len()
                },
                1,
                0,
                ptr::null_mut()
            ),
            ERR_INVALID
        );
    }
    assert_eq!(access.query("core.usage", 1, 0, &mut handle), OK);
    let mut second = 0x9876;
    assert_eq!(access.query("core.usage", 1, 0, &mut second), ERR_BUSY);
    assert_eq!(second, 0x9876);
    let mut bytes = [0xab; 128];
    let count = access.read(handle, 0, &mut bytes);
    assert!(count > 0);
    assert_eq!(bytes[count as usize], 0xab);
    let mut suffix = [0; 7];
    assert_eq!(access.read(handle, 3, &mut suffix), 7);
    assert_eq!(&suffix, &bytes[3..10]);
    assert_eq!(access.read(handle, u64::MAX, &mut suffix), 0);
    assert_eq!(access.read(font, 0, &mut suffix), ERR_STATE);
    assert_eq!(access.read(handle, 0, &mut []), 0);
    assert_eq!(access.release(handle), OK);
    assert_eq!(access.release(handle), ERR_STATE);
    assert_eq!(access.query("core.usage", 1, 0, &mut handle), OK);
    handle
}

#[test]
fn usage_slots_preserve_failures_read_positionally_and_expire_at_core_callback_end() {
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let view = owner.try_view().unwrap();
    let scope = pin!(EnvironmentScope::new());
    // SAFETY: the view outlives the scope's callbacks.
    unsafe { scope.expire_view(&view) };
    let declared = DeclaredServices::try_from_iter(
        [paperpaw_native::metadata::Declared {
            key: "core.usage",
            major: 1,
            semantic_revision: 0,
        }]
        .into_iter(),
    )
    .unwrap();
    let access = Access {
        scope: &scope,
        owner: &owner,
        declared: &declared,
    };
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let frame = CoreFrame {
        callback: None,
        instance: None,
        env: &env,
        meter: &kernel.shared.meter,
        depth: None,
        directory: None,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
    };
    let mut handle = 0x1234;
    let font = owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
    assert_eq!(access.query("core.usage", 1, 0, &mut handle), ERR_STATE);
    let handle = scope
        .as_ref()
        .scope_core(&frame, || check_live(&access, font))
        .unwrap();
    scope
        .as_ref()
        .scope_core(&frame, || {
            assert_eq!(access.release(handle), ERR_STATE);
            owner.with_font(font, |_| Ok(())).unwrap();
        })
        .unwrap();
}
