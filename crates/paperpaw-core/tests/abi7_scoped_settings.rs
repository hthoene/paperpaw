//! The public settings bridge uses a real dispatch Env and the existing codec.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
mod common;
use paperpaw_core::native_host::environment::{EnvironmentScope, SettingValueRef, settings_decode};
use paperpaw_native::host_glue::{ERR_STATE, ERR_UTF8};
use paperpaw_sdk::{Manifest, Plugin, RegisterError, Registrar, SettingSpec, builtin};
use std::{cell::Cell, pin::pin, rc::Rc};

static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::switch("probe.flag", "flag", false, builtin::groups::OTHER),
    SettingSpec::number("probe.number", "number", -1, 1, 0, builtin::groups::OTHER),
    SettingSpec::text("probe.text", "text", "", builtin::groups::OTHER),
];
fn read(scope: &EnvironmentScope, key: &[u8], out: &mut [u8]) -> i32 {
    // SAFETY: borrowed slices are live, exclusive/disjoint and not host state.
    unsafe { scope.settings_read(key.as_ptr(), key.len(), out.as_mut_ptr(), out.len()) }
}
struct Probe(Rc<Cell<u32>>);
impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        common::manifest("probe")
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        let count = self.0.clone();
        r.subscribe(
            builtin::events::STARTED,
            Box::new(move |env, _, _| {
                let scope = pin!(EnvironmentScope::new());
                let mut bytes = [0xa5; 64];
                assert_eq!(read(&scope, b"probe.flag", &mut bytes), ERR_STATE);
                scope
                    .as_ref()
                    .scope(env, || {
                        for (key, expected) in [
                            ("probe.flag", SettingValueRef::Bool(false)),
                            ("probe.number", SettingValueRef::Int(0)),
                            ("probe.text", SettingValueRef::Text("")),
                            ("probe.absent", SettingValueRef::Absent),
                        ] {
                            bytes.fill(0xa5);
                            let needed =
                                usize::try_from(read(&scope, key.as_bytes(), &mut bytes)).unwrap();
                            assert_eq!(settings_decode(&bytes[..needed]), Ok(expected));
                            assert!(bytes[needed..].iter().all(|b| *b == 0xa5));
                            assert_eq!(
                                read(&scope, key.as_bytes(), &mut []),
                                i32::try_from(needed).unwrap()
                            );
                            let mut short = [0xa5; 1];
                            assert_eq!(
                                read(&scope, key.as_bytes(), &mut short),
                                i32::try_from(needed).unwrap()
                            );
                            assert!(settings_decode(&short).is_err());
                        }
                        bytes.fill(0xa5);
                        assert_eq!(read(&scope, &[0xff], &mut bytes), ERR_UTF8);
                        assert_eq!(bytes, [0xa5; 64]);
                        assert!(scope.as_ref().scope(env, || ()).is_err());
                    })
                    .unwrap();
                assert_eq!(read(&scope, b"probe.flag", &mut bytes), ERR_STATE);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    scope
                        .as_ref()
                        .scope(env, || panic!("fixture unwind"))
                        .unwrap();
                }));
                assert!(result.is_err());
                assert_eq!(read(&scope, b"probe.flag", &mut bytes), ERR_STATE);
                scope
                    .as_ref()
                    .scope(env, || assert!(read(&scope, b"probe.flag", &mut bytes) > 0))
                    .unwrap();
                count.set(count.get() + 1);
            }),
        )
    }
}
#[test]
fn typed_settings_query_and_cleanup_use_actual_dispatch_frame() {
    let count = Rc::new(Cell::new(0));
    let mut kernel = paperpaw_core::Kernel::builder(common::DEVICE)
        .plugin(Probe(count.clone()))
        .build()
        .unwrap();
    kernel.start(paperpaw_platform::Instant(0));
    assert_eq!(count.get(), 1);
}
